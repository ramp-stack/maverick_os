use image::RgbaImage;
use jni::objects::{GlobalRef, JByteArray, JClass, JObject, JValue};
use jni::sys::jlong;
use jni::{JNIEnv, JavaVM};
use ndk_context;
use std::sync::Arc;

use crate::hardware::android_util::JNIUtil;

pub struct OsPhotoPicker {
    vm: Arc<JavaVM>,
    context: GlobalRef,
    photo_helper: Option<GlobalRef>,
}

/// Concrete wrapper for the one-shot callback (thin pointer)x
struct CallbackHolder {
    f: Box<dyn FnOnce(Option<RgbaImage>) + Send + 'static>,
}

impl CallbackHolder {
    fn invoke(self, result: Option<RgbaImage>) {
        (self.f)(result);
    }
}

impl OsPhotoPicker {
    pub fn new() -> Self {
        println!("constructing new OsPhotoPicker");
        let vm = match unsafe { JavaVM::from_raw(ndk_context::android_context().vm().cast()) } {
            Ok(vm) => Arc::new(vm),
            Err(e) => {
                log::error!("Failed to get JavaVM: {}", e);
                // Return a non-functional instance
                return Self {
                    vm: Arc::new(unsafe {
                        JavaVM::from_raw(ndk_context::android_context().vm().cast())
                            .expect("Critical: JavaVM unavailable for AndroidPhotoPicker")
                    }),
                    context: unsafe {
                        // Create a dummy global ref (will fail later if used)
                        let dummy = JObject::null();
                        // This will panic on real use — acceptable for error path
                        std::mem::transmute::<_, GlobalRef>(dummy)
                    },
                    photo_helper: None,
                };
            }
        };
        let context = {
            let env = vm.attach_current_thread().expect("Failed to attach thread");
            let ctx_ptr = ndk_context::android_context().context().cast();
            let context_obj = unsafe { JObject::from_raw(ctx_ptr) };
            env.new_global_ref(context_obj)
                .expect("Failed to create global ref to context")
        };
        {
            let mut env = vm.attach_current_thread().expect("Failed to attach thread");
            match JNIUtil::load_app_class(
                &mut env,
                &context,
                "com.maverick.photo.PhotoPickerActivity",
            ) {
                Ok(activity_class) => {
                    if let Err(e) = JNIUtil::register_native_methods_on_class(
                        &mut env,
                        &activity_class,
                        vec![jni::NativeMethod {
                            name: "nativeOnPhotoPicked".into(),
                            sig: "(J[BII)V".into(),
                            fn_ptr: Java_com_maverick_photo_PhotoPickerActivity_nativeOnPhotoPicked
                                as *mut std::ffi::c_void,
                        }],
                    ) {
                        log::error!("Failed to register PhotoPickerActivity natives: {}", e);
                    }
                }
                Err(e) => log::error!("Failed to load PhotoPickerActivity class: {}", e),
            }
        }

        // let package_name = {
        //     let mut env = vm.attach_current_thread().ok();
        //     if let Some(env) = &mut env {
        //         JNIUtil::get_package_name(env, context.as_obj())
        //     } else {
        //         None
        //     }
        // };
        let photo_helper = {
            println!("loading embedded dex for photo picker");
            let dex_bytes: &[u8] = include_bytes!("android/classes.dex");

            let class_name = "com.maverick.photo.PhotoPickerHelper".to_string();

            match unsafe {
                JNIUtil::load_class_from_embedded_dex(&vm, &context, dex_bytes, &class_name)
            } {
                Ok(helper) => Some(helper),
                Err(e) => {
                    log::error!("Failed to load PhotoPickerHelper from dex: {}", e);
                    None
                }
            }
        };

        Self {
            vm,
            context,
            photo_helper,
        }
    }

    pub fn open(&self, callback: impl FnOnce(Option<RgbaImage>) + Send + 'static) {
        println!("opening photo picker");
        let holder = Box::new(CallbackHolder {
            f: Box::new(callback),
        });
        let callback_ptr = Box::into_raw(holder) as jlong;

        let mut env = match self.vm.attach_current_thread() {
            Ok(env) => env,
            Err(e) => {
                log::error!("Failed to attach to JVM: {}", e);
                unsafe {
                    let holder = Box::from_raw(callback_ptr as *mut CallbackHolder);
                    holder.invoke(None);
                }
                return;
            }
        };
        if let Err(e) = self.launch_picker(&mut env, callback_ptr) {
            log::error!("Failed to launch photo picker: {}", e);
            unsafe {
                let holder = Box::from_raw(callback_ptr as *mut CallbackHolder);
                holder.invoke(None);
            }
        }
    }

    fn launch_picker(
        &self,
        env: &mut JNIEnv,
        callback_ptr: jlong,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let helper_class_ref = self
            .photo_helper
            .as_ref()
            .ok_or("PhotoPickerHelper not loaded")?;
        let helper_class = <&jni::objects::JClass>::from(helper_class_ref.as_obj());
        env.call_static_method(
            helper_class,
            "openPhotoPicker",
            "(Landroid/content/Context;J)V",
            &[
                JValue::Object(self.context.as_obj()),
                JValue::Long(callback_ptr),
            ],
        )?;

        Ok(())
    }
}

// Native callback from Java - declared on PhotoPickerActivity now, not PhotoPickerHelper,
// since that's the class that actually receives onActivityResult and calls back into native code.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_maverick_photo_PhotoPickerActivity_nativeOnPhotoPicked(
    env: JNIEnv,
    _class: JClass,
    callback_ptr: jlong,
    image_data: JByteArray,
    width: i32,
    height: i32,
) {
    unsafe {
        let holder = Box::from_raw(callback_ptr as *mut CallbackHolder);
        let inner = *holder;
        let callback = inner.f;

        if image_data.is_null() {
            callback(None);
            return;
        }

        let bytes = match env.convert_byte_array(image_data) {
            Ok(b) => b,
            Err(e) => {
                log::error!("nativeOnPhotoPicked: failed to convert byte array: {}", e);
                callback(None);
                return;
            }
        };

        let expected_len = (width as usize) * (height as usize) * 4;
        if bytes.len() != expected_len {
            log::error!(
                "nativeOnPhotoPicked: length mismatch, got {} expected {}",
                bytes.len(),
                expected_len
            );
            callback(None);
            return;
        }

        let mut img = RgbaImage::new(width as u32, height as u32);
        for (i, chunk) in bytes.chunks_exact(4).enumerate() {
            let x = (i % (width as usize)) as u32;
            let y = (i / (width as usize)) as u32;
            img.put_pixel(x, y, image::Rgba([chunk[0], chunk[1], chunk[2], chunk[3]]));
        }

        callback(Some(img));
    }
}
