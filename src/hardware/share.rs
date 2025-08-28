#[cfg(target_os = "ios")]
use objc2::rc::autoreleasepool;
#[cfg(target_os = "ios")]
use objc2::{AnyThread, class, msg_send};
#[cfg(target_os = "ios")]
use objc2_foundation::{NSArray, NSObject, NSString};
#[cfg(target_os = "ios")]
use std::ffi::c_void;
#[cfg(target_os = "ios")]
use objc2::rc::{Retained, Allocated};
#[cfg(target_os = "ios")]
use objc2::runtime::{AnyObject, Object};
#[cfg(target_os = "ios")]
use objc2_foundation::{NSPoint, NSRect, NSSize};
#[cfg(target_os = "ios")]
use objc2_ui_kit::{
    UIActivityViewController, UIImage, UIUserInterfaceIdiom, UIPopoverPresentationController, UIView,
    UIViewController, UIImageOrientation, UIDevice, UIScreen
};
#[cfg(target_os = "ios")]
use std::ptr;
#[cfg(target_os = "ios")]
use objc2::declare_class;
#[cfg(target_os = "ios")]
use objc2_foundation::{MainThreadMarker};

#[cfg(target_os = "android")]
use jni::objects::{GlobalRef, JObject, JValue};
#[cfg(target_os = "android")]
use jni::sys::jobject;
#[cfg(target_os = "android")]
use jni::{JNIEnv, JavaVM};
#[cfg(target_os = "android")]
use ndk_context;
#[cfg(target_os = "android")]
use std::error::Error;
#[cfg(target_os = "android")]
use std::sync::{Once, OnceLock};

use image::RgbaImage;

// This is a Cross platform system for calling the native system shareing sheet.

// System:

// <iOS>>>: Uses the UIActivityViewController to show the native Share Sheet for sharing text or images.

// <Android>>>: Creats and runs an message object also known as Intent with an action ACTION_SEND wrapped in a chooser thing so that users can pick which app te shar with.

// <macOS & Linux>>>: Nothing here yet..


#[cfg(target_os = "android")]
static JAVA_VM: OnceLock<JavaVM> = OnceLock::new();
#[cfg(target_os = "android")]
static APP_CONTEXT: OnceLock<GlobalRef> = OnceLock::new();
#[cfg(target_os = "android")]
static INIT_ONCE: Once = Once::new();

/// Share content via the system share dialog.
#[derive(Clone)]
pub struct Share;

impl Share {
    #[cfg(target_os = "android")]
    pub fn initialize() -> Result<(), Box<dyn Error>> {
        let jvm = unsafe { JavaVM::from_raw(ndk_context::android_context().vm().cast())? };

        let global_context = {
            let mut env = jvm.attach_current_thread()?;

            let ctx_ptr = ndk_context::android_context().context();
            if ctx_ptr.is_null() {
                return Err("Failed to get Android context".into());
            }

            let context_obj = unsafe { JObject::from_raw(ctx_ptr as jobject) };
            env.new_global_ref(context_obj)?
        };

        JAVA_VM.set(jvm).map_err(|_| "JavaVM already initialized")?;
        APP_CONTEXT.set(global_context).map_err(|_| "App context already initialized")?;

        Ok(())
    }

    #[cfg(target_os = "ios")]
    pub fn share(text: &str) {
        autoreleasepool(|_| {
            let ns_string = NSString::from_str(text);
            let items = NSArray::from_slice(&[&*ns_string]);

            let cls = class!(UIActivityViewController);
            let activity_controller: *mut NSObject = unsafe { msg_send![cls, alloc] };

            let activity_controller: *mut NSObject = unsafe {
                msg_send![activity_controller, initWithActivityItems:&*items applicationActivities: std::ptr::null_mut::<NSArray<NSObject>>()]
            };

            let ui_app = class!(UIApplication);
            let shared_app: *mut NSObject = unsafe { msg_send![ui_app, sharedApplication] };
            let key_window: *mut NSObject = unsafe { msg_send![shared_app, keyWindow] };
            let root_vc: *mut NSObject = unsafe { msg_send![key_window, rootViewController] };

            let _: () = unsafe {
                msg_send![
                    root_vc,
                    presentViewController:activity_controller
                    animated:true
                    completion: std::ptr::null_mut::<objc2::runtime::Object>()
                ]
            };
        });
    }

    #[cfg(target_os = "ios")]
    pub fn share_image(rgba_image: RgbaImage) {
        autoreleasepool(|_| {
            use objc2_core_graphics::{
                // kCGImageAlphaPremultipliedLast, 
                CGImageAlphaInfo,
                CGColorSpaceCreateDeviceRGB,
                CGBitmapContextCreate,
                CGBitmapContextCreateImage,
                // CGBitmapContextCreate,
                CGImage,
            };

            let width = rgba_image.width();
            let height = rgba_image.height();
            let bytes_per_pixel = 4;
            let bytes_per_row = bytes_per_pixel * width;
            let bits_per_component = 8;

            let buffer = rgba_image.as_raw();
            let color_space = unsafe { CGColorSpaceCreateDeviceRGB() };

            // let bitmap_info: u32 = CGImageAlphaInfo::PremultipliedLast.0 | kCGBitmapByteOrder32Big;

            let context = unsafe {
                CGBitmapContextCreate(
                    buffer.as_ptr() as *mut _,
                    width as usize,
                    height as usize,
                    bits_per_component as usize,
                    bytes_per_row as usize,
                    color_space.as_deref(),
                    CGImageAlphaInfo::PremultipliedLast.0, //CGImageAlphaInfo::PremultipliedLast,
                )
            }
            .expect("Failed to create bitmap context");

            // Create CGImage from context
            let cg_image: Retained<CGImage> = unsafe {
                CGBitmapContextCreateImage(Some(&context))
                    .expect("Could not create image from context.").into()
            };


            // Convert CGImage to UIImage
            let scale: *mut UIScreen = unsafe { msg_send![class!(UIScreen), mainScreen] };
            let scale: f64 = unsafe { msg_send![scale, scale] };
            let ui_image: Allocated<UIImage> = unsafe { UIImage::alloc() };
            let ui_image = unsafe {
                UIImage::initWithCGImage_scale_orientation(ui_image, &cg_image, scale, UIImageOrientation::Up)
            };

            // Create an array of items to share
            let items = NSArray::from_slice(&[&*ui_image]);

            // Initialize UIActivityViewController
            let cls = class!(UIActivityViewController);
            let activity_controller: *mut NSObject = unsafe { msg_send![cls, alloc] };
            let activity_controller: *mut NSObject = unsafe {
                msg_send![activity_controller, initWithActivityItems:&*items applicationActivities: ptr::null_mut::<NSArray<NSObject>>()]
            };

            // For iPad, configure popover presentation
            let device: *mut UIDevice = unsafe {msg_send![class!(UIDevice), currentDevice]};
            if UIUserInterfaceIdiom::Pad == unsafe { msg_send![device, userInterfaceIdiom] }{
                let popover: Retained<UIPopoverPresentationController> = unsafe {
                    msg_send![activity_controller, popoverPresentationController]
                };
                // Get the root view controller's view
                let ui_app = class!(UIApplication);
                let shared_app: *mut NSObject = unsafe { msg_send![ui_app, sharedApplication] };
                let key_window: *mut NSObject = unsafe { msg_send![shared_app, keyWindow] };
                let root_vc: *mut NSObject = unsafe { msg_send![key_window, rootViewController] };
                let view: Retained<UIView> = unsafe { msg_send![root_vc, view] };
                unsafe {
                    let _: () = unsafe { msg_send![&*popover, setSourceView: &*view] };
                    let _: () = unsafe {
                        msg_send![
                            &*popover,
                            setSourceRect: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0)),
                        ]
                    };
                        
                }
            }

            // Present the share sheet
            let ui_app = class!(UIApplication);
            let shared_app: *mut NSObject = unsafe { msg_send![ui_app, sharedApplication] };
            let key_window: *mut NSObject = unsafe { msg_send![shared_app, keyWindow] };
            let root_vc: *mut NSObject = unsafe { msg_send![key_window, rootViewController] };
            let _: () = unsafe {
                msg_send![
                    root_vc,
                    presentViewController:activity_controller
                    animated:true
                    completion: ptr::null_mut::<objc2::runtime::Object>()
                ]
            };
        });
    }


    #[cfg(target_os = "macos")]
    pub fn share_image(_rgba: image::RgbaImage) {}
    #[cfg(target_os = "linux")]
    pub fn share_image(_rgba: image::RgbaImage) {}
    #[cfg(target_os = "android")]
    pub fn share_image(_rgba: image::RgbaImage) {}

    #[cfg(target_os = "macos")]
    pub fn share(_text: &str) {}
    #[cfg(target_os = "linux")]
    pub fn share(_text: &str) {}

    #[cfg(target_os = "android")]
    pub fn share(&self, text: &str) {
        if JAVA_VM.get().is_none() {
            if let Err(e) = Self::initialize() {
                eprintln!("Failed to initialize Share: {}", e);
                return;
            }
        }

        if let Some(vm) = JAVA_VM.get() {
            if let Ok(mut env) = vm.attach_current_thread() {
                if let Err(e) = self.share_with_jni(&mut env, text) {
                    eprintln!("Failed to share on Android: {}", e);
                }
            } else {
                eprintln!("Failed to attach to current thread");
            }
        } else {
            eprintln!("JavaVM not initialized. Make sure to call Share::initialize() first.");
        }
    }

    #[cfg(target_os = "android")]
    fn share_with_jni(&self, env: &mut JNIEnv, text: &str) -> Result<(), Box<dyn Error>> {

        let chooser_intent = self.create_share_intent(env, text)?;

        self.start_share_activity(env, chooser_intent)?;

        Ok(())
    }

    #[cfg(target_os = "android")]
    fn create_share_intent<'a>(&self, env: &mut JNIEnv<'a>, text: &str) -> Result<JObject<'a>, Box<dyn Error>> {
        let intent_class = env.find_class("android/content/Intent")?;
        let intent = env.new_object(intent_class, "()V", &[])?;

        let action_send = env.new_string("android.intent.action.SEND")?;
        env.call_method(
            &intent,
            "setAction",
            "(Ljava/lang/String;)Landroid/content/Intent;",
            &[JValue::Object(&action_send)],
        )?;

        let mime_type = env.new_string("text/plain")?;
        env.call_method(
            &intent,
            "setType",
            "(Ljava/lang/String;)Landroid/content/Intent;",
            &[JValue::Object(&mime_type)],
        )?;

        let extra_text = env.new_string("android.intent.extra.TEXT")?;
        let share_text = env.new_string(text)?;
        env.call_method(
            &intent,
            "putExtra",
            "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/Intent;",
            &[JValue::Object(&extra_text), JValue::Object(&share_text)],
        )?;

        let flags = env.get_static_field("android/content/Intent", "FLAG_ACTIVITY_NEW_TASK", "I")?;
        let flag_value = flags.i()?;
        env.call_method(
            &intent,
            "addFlags",
            "(I)Landroid/content/Intent;",
            &[JValue::Int(flag_value)],
        )?;

        let chooser_title = env.new_string("Share via")?;
        let intent_class_static = env.find_class("android/content/Intent")?;
        let chooser = env.call_static_method(
            intent_class_static,
            "createChooser",
            "(Landroid/content/Intent;Ljava/lang/CharSequence;)Landroid/content/Intent;",
            &[JValue::Object(&intent), JValue::Object(&chooser_title)],
        )?;

        let chooser_obj = chooser.l()?;
        env.call_method(
            &chooser_obj,
            "addFlags",
            "(I)Landroid/content/Intent;",
            &[JValue::Int(flag_value)],
        )?;

        Ok(chooser_obj)
    }

    #[cfg(target_os = "android")]
    fn start_share_activity<'a>(&self, env: &mut JNIEnv<'a>, chooser_intent: JObject<'a>) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(global_context) = APP_CONTEXT.get() {
            let context = env.new_local_ref(global_context)?;

            env.call_method(
                &context,
                "startActivity",
                "(Landroid/content/Intent;)V",
                &[JValue::Object(&chooser_intent)],
            )?;
            Ok(())
        } else {
            Err("App context not initialized. Call Share::initialize() first.".into())
        }
    }
}

