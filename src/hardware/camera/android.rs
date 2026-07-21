use crate::hardware::android_util::JNIUtil;
use image::RgbaImage;
use jni::objects::{GlobalRef, JByteBuffer, JClass, JObject, JObjectArray, JString, JValue};
use jni::sys::{jint, jlong, jobject};
use jni::{JNIEnv, JavaVM};
use ndk_context;

use std::error::Error;
use std::sync::{Arc, Mutex};

pub struct OsCamera {
    java_vm: Arc<JavaVM>,
    app_context: Option<GlobalRef>,
    camera_helper: Option<GlobalRef>,
    latest_frame: Arc<Mutex<Option<RgbaImage>>>,
    permission_requested: bool,
    camera_opened: bool,
}

impl OsCamera {
    pub fn new() -> Self {
        println!("constructing new OsCamera");
        // Try to get JavaVM
        let java_vm = match unsafe { JavaVM::from_raw(ndk_context::android_context().vm().cast()) }
        {
            Ok(vm) => std::sync::Arc::new(vm),
            Err(e) => {
                log::error!("Failed to get JavaVM: {}", e);
                // Return a non-functional camera object
                return Self {
                    java_vm: std::sync::Arc::new(unsafe {
                        JavaVM::from_raw(ndk_context::android_context().vm().cast())
                            .expect("Critical: JavaVM unavailable for AndroidCamera")
                    }),
                    app_context: None,
                    camera_helper: None,
                    latest_frame: Arc::new(Mutex::new(None)),
                    permission_requested: false,
                    camera_opened: false,
                };
            }
        };

        // Try to get Context
        println!("getting app context");
        let app_context = {
            let env = match java_vm.attach_current_thread() {
                Ok(env) => env,
                Err(e) => {
                    log::error!("Failed to attach to JVM: {}", e);
                    return Self {
                        java_vm: java_vm.clone(),
                        app_context: None,
                        camera_helper: None,
                        latest_frame: Arc::new(Mutex::new(None)),
                        permission_requested: false,
                        camera_opened: false,
                    };
                }
            };

            let ctx_ptr = ndk_context::android_context().context();
            if ctx_ptr.is_null() {
                log::error!("Android context is null");
                return Self {
                    java_vm: java_vm.clone(),
                    app_context: None,
                    camera_helper: None,
                    latest_frame: Arc::new(Mutex::new(None)),
                    permission_requested: false,
                    camera_opened: false,
                };
            }

            let context = unsafe { JObject::from_raw(ctx_ptr as jobject) };
            match env.new_global_ref(context) {
                Ok(global) => Some(global),
                Err(e) => {
                    log::error!("Failed to create global ref to Context: {}", e);
                    None
                }
            }
        };

        let mut camera = Self {
            java_vm: java_vm.clone(),
            app_context,
            camera_helper: None,
            latest_frame: Arc::new(Mutex::new(None)),
            permission_requested: false,
            camera_opened: false,
        };

        camera.start();
        camera
    }

    // Public API
    pub fn start(&mut self) {
        if self.camera_opened {
            return;
        }

        if self.app_context.is_none() {
            log::error!("Cannot start camera: app_context is None");
            return;
        }

        if self.camera_helper.is_none() {
            if let Err(e) = unsafe { self.load_embedded_dex() } {
                log::error!("Failed to load CameraHelper from embedded Dex: {}", e);
                return;
            }
        }

        let granted = {
            let mut env = match self.java_vm.attach_current_thread() {
                Ok(env) => env,
                Err(e) => {
                    log::error!("Failed to attach to JVM: {}", e);
                    return;
                }
            };

            let granted = self.has_permission(&mut env).unwrap_or(false);
            if !granted && !self.permission_requested {
                if let Err(e) = self.request_permission(&mut env) {
                    log::error!("Failed to request camera permission: {}", e);
                }
                self.permission_requested = true;
            }
            granted
        }; // env dropped here, releasing its borrow before open_camera() needs &mut self

        if !granted {
            // Permission dialog is async and user-paced - check again next tick
            // rather than blocking the render loop.
            return;
        }

        match self.open_camera() {
            Ok(_) => self.camera_opened = true,
            Err(e) => log::error!("Failed to open camera: {}", e),
        }
    }

    pub fn stop(&mut self) {
        if !self.camera_opened {
            return;
        }
        println!("stopping camera");
        if let Some(helper) = &self.camera_helper {
            if let Ok(mut env) = self.java_vm.attach_current_thread() {
                let _ = env.call_method(helper.as_obj(), "closeCamera", "()V", &[]);
            }
        }
        self.camera_opened = false;
    }

    pub fn frame(&self) -> Option<RgbaImage> {
        self.latest_frame.lock().unwrap().take()
    }

    fn open_camera(&mut self) -> Result<(), Box<dyn Error>> {
        println!("open_camera function");
        let helper = self.camera_helper.as_ref().ok_or("camera helper is None")?;

        let mut env = self.java_vm.attach_current_thread()?;

        let camera_ids = env
            .call_method(
                helper.as_obj(),
                "getCameraIdList",
                "()[Ljava/lang/String;",
                &[],
            )?
            .l()?;

        let id_array = JObjectArray::from(camera_ids);
        if env.get_array_length(&id_array)? == 0 {
            return Err("No cameras found".into());
        }

        let first_id = env.get_object_array_element(&id_array, 0)?;
        let camera_id: String = env.get_string(&JString::from(first_id))?.into();
        let camera_id_jstr = env.new_string(&camera_id)?;

        // Passed through to Java and back on every ImageAvailableListener callback so the
        // native side can write completed frames straight into this Arc's Mutex without any
        // per-instance JNI object lookup. Safe as long as `self.latest_frame` (owned by this
        // OsCamera, which lives for the app's lifetime) outlives the callback, since
        // CameraHelper.closeCamera() tears down the ImageReader/session before that could race.
        let frame_ptr = Arc::as_ptr(&self.latest_frame) as jlong;

        env.call_method(
            helper.as_obj(),
            "openCamera",
            "(Ljava/lang/String;J)V",
            &[JValue::Object(&camera_id_jstr), JValue::Long(frame_ptr)],
        )?;

        Ok(())
    }

    unsafe fn load_embedded_dex(&mut self) -> Result<(), Box<dyn Error>> {
        println!("loading embedded dex");
        let dex_bytes: &[u8] = include_bytes!("../camera/android/classes.dex");

        let class_name = "com.maverick.camera.CameraHelper";

        let helper = unsafe {
            JNIUtil::instantiate_class_from_embedded_dex(
                &self.java_vm,
                self.app_context.as_ref().ok_or("App context is None")?,
                dex_bytes,
                class_name,
            )
        }?;

        let mut env = self.java_vm.attach_current_thread()?;
        JNIUtil::register_native_methods(
            &mut env,
            &helper,
            vec![jni::NativeMethod {
                name: "nativeOnFrameAvailable".into(),
                sig: "(JLandroid/media/Image;I)V".into(),
                fn_ptr: Java_com_maverick_camera_CameraHelper_nativeOnFrameAvailable
                    as *mut std::ffi::c_void,
            }],
        )?;

        self.camera_helper = Some(helper);
        Ok(())
    }

    // Permission Helpers
    fn has_permission(&self, env: &mut JNIEnv) -> Result<bool, Box<dyn Error>> {
        println!("checking permissions");
        let helper = self.camera_helper.as_ref().ok_or("camera helper is None")?;

        let result = env
            .call_method(helper.as_obj(), "hasCameraPermission", "()Z", &[])?
            .z()?;
        Ok(result)
    }

    fn request_permission(&self, env: &mut JNIEnv) -> Result<(), Box<dyn Error>> {
        println!("requesting camera permission");
        let helper = self.camera_helper.as_ref().ok_or("camera helper is None")?;
        env.call_method(helper.as_obj(), "requestCameraPermission", "()V", &[])?;
        Ok(())
    }
}

// Called directly by CameraHelper.ImageAvailableListener, which Camera2 always invokes on
// the ImageReader's backgroundHandler thread - so this conversion work runs off the app's
// main/render thread by construction, never blocking rendering or input dispatch.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_maverick_camera_CameraHelper_nativeOnFrameAvailable(
    mut env: JNIEnv,
    _class: JClass,
    frame_ptr: jlong,
    image: JObject,
    rotation_degrees: jint,
) {
    if frame_ptr == 0 {
        return;
    }
    let latest_frame = unsafe { &*(frame_ptr as *const Mutex<Option<RgbaImage>>) };

    match process_image(&mut env, &image, rotation_degrees) {
        Ok(img) => *latest_frame.lock().unwrap() = Some(img),
        Err(e) => log::error!("Failed to process camera image: {}", e),
    }
}

// Converts the YUV_420_888 image straight into a rotated RgbaImage in a single pass -
// SENSOR_ORIENTATION is the clockwise rotation (0/90/180/270) Camera2 says the raw sensor
// image needs to match the device's upright orientation, applied here as a change of
// destination index rather than a separate post-processing rotation pass.
fn process_image(
    env: &mut JNIEnv,
    image: &JObject,
    rotation_degrees: i32,
) -> Result<RgbaImage, Box<dyn Error>> {
    let width = env.call_method(image, "getWidth", "()I", &[])?.i()?;
    let height = env.call_method(image, "getHeight", "()I", &[])?.i()?;

    let planes: JObjectArray = env
        .call_method(image, "getPlanes", "()[Landroid/media/Image$Plane;", &[])?
        .l()?
        .into();

    let plane_count = env.get_array_length(&planes)?;
    if plane_count < 3 {
        return Err("Image does not have the expected YUV planes".into());
    }

    let mut extract = |idx| -> Result<(Vec<u8>, i32, i32), Box<dyn Error>> {
        let plane = env.get_object_array_element(&planes, idx)?;
        let buffer = env
            .call_method(&plane, "getBuffer", "()Ljava/nio/ByteBuffer;", &[])?
            .l()?;
        let byte_buffer = JByteBuffer::from(buffer);

        let len = env.get_direct_buffer_capacity(&byte_buffer)?;
        let ptr = env.get_direct_buffer_address(&byte_buffer)?;
        let data = unsafe { std::slice::from_raw_parts(ptr, len).to_vec() };

        let row_stride = env.call_method(&plane, "getRowStride", "()I", &[])?.i()?;
        let pixel_stride = env.call_method(&plane, "getPixelStride", "()I", &[])?.i()?;

        Ok((data, row_stride, pixel_stride))
    };

    let (y, y_rs, y_ps) = extract(0)?;
    let (u, u_rs, u_ps) = extract(1)?;
    let (v, v_rs, v_ps) = extract(2)?;

    let rotation = ((rotation_degrees % 360) + 360) % 360;
    let (out_w, out_h) = if rotation == 90 || rotation == 270 {
        (height as u32, width as u32)
    } else {
        (width as u32, height as u32)
    };

    let mut buffer = vec![0u8; (out_w * out_h * 4) as usize];

    for row in 0..height {
        for col in 0..width {
            let yi = (row * y_rs + col * y_ps) as usize;
            let ui = ((row / 2) * u_rs + (col / 2) * u_ps) as usize;
            let vi = ((row / 2) * v_rs + (col / 2) * v_ps) as usize;

            let y_val = y.get(yi).copied().unwrap_or(0) as i32;
            let u_val = u.get(ui).copied().unwrap_or(128) as i32;
            let v_val = v.get(vi).copied().unwrap_or(128) as i32;

            let c = y_val - 16;
            let d = u_val - 128;
            let e = v_val - 128;

            let r = ((298 * c + 409 * e + 128) >> 8).clamp(0, 255) as u8;
            let g = ((298 * c - 100 * d - 208 * e + 128) >> 8).clamp(0, 255) as u8;
            let b = ((298 * c + 516 * d + 128) >> 8).clamp(0, 255) as u8;

            // Destination index for a `rotation`-degree clockwise rotation of the source.
            let (dst_x, dst_y) = match rotation {
                90 => (height - 1 - row, col),
                180 => (width - 1 - col, height - 1 - row),
                270 => (row, width - 1 - col),
                _ => (col, row),
            };

            let idx = ((dst_y as u32 * out_w + dst_x as u32) * 4) as usize;
            buffer[idx] = r;
            buffer[idx + 1] = g;
            buffer[idx + 2] = b;
            buffer[idx + 3] = 255;
        }
    }

    RgbaImage::from_raw(out_w, out_h, buffer)
        .ok_or_else(|| "Failed to construct RgbaImage from raw buffer".into())
}
