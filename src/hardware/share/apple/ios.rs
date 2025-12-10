 use image::RgbaImage;
use objc2::rc::autoreleasepool;
use objc2::{class, msg_send};
use objc2_foundation::{NSArray, NSObject, NSString, NSPoint, NSRect, NSSize};
use objc2::rc::{Retained, Allocated};

use objc2_ui_kit::{
    UIImage, UIUserInterfaceIdiom, UIPopoverPresentationController, UIView,
    UIImageOrientation, UIDevice, UIScreen
};

use objc2::AnyThread;
use std::ptr;

#[derive(Clone)]
pub struct OsShare;

impl OsShare {
    pub fn new() -> Self {
        Self
    }

    pub fn share(&self, text: &str) {
        autoreleasepool(|_| {
            let ns_string = NSString::from_str(text);
            let items = NSArray::from_slice(&[&*ns_string]);

            let cls = class!(UIActivityViewController);
            let activity_controller: *mut NSObject = unsafe { msg_send![cls, alloc] };

            let activity_controller: *mut NSObject = unsafe {
                msg_send![activity_controller, initWithActivityItems:&*items, applicationActivities: std::ptr::null_mut::<NSArray<NSObject>>()]
            };

            let ui_app = class!(UIApplication);
            let shared_app: *mut NSObject = unsafe { msg_send![ui_app, sharedApplication] };
            let key_window: *mut NSObject = unsafe { msg_send![shared_app, keyWindow] };
            let root_vc: *mut NSObject = unsafe { msg_send![key_window, rootViewController] };

            let _: () = unsafe {
                msg_send![
                    root_vc,
                    presentViewController:activity_controller,
                    animated:true,
                    completion: std::ptr::null_mut::<objc2::runtime::AnyObject>()
                ]
            };
        });
    }

    pub fn share_image(&self, rgba_image: RgbaImage) {
        autoreleasepool(|_| {
            use objc2_core_graphics::{
                CGImageAlphaInfo,
                CGBitmapContextCreate,
                CGBitmapContextCreateImage,
                CGImage,
            };

            let width = rgba_image.width();
            let height = rgba_image.height();
            let bytes_per_pixel = 4;
            let bytes_per_row = bytes_per_pixel * width;
            let bits_per_component = 8;

            let buffer = rgba_image.as_raw();
            let color_space = unsafe { objc2_core_graphics::CGColorSpace::new_device_rgb() };

            let context = unsafe {
                CGBitmapContextCreate(
                    buffer.as_ptr() as *mut _,
                    width as usize,
                    height as usize,
                    bits_per_component as usize,
                    bytes_per_row as usize,
                    color_space.as_deref(),
                    CGImageAlphaInfo::PremultipliedLast.0,
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
            let ui_image: Allocated<UIImage> = UIImage::alloc();
            let ui_image = unsafe {
                UIImage::initWithCGImage_scale_orientation(ui_image, &cg_image, scale, UIImageOrientation::Up)
            };

            // Create an array of items to share
            let items = NSArray::from_slice(&[&*ui_image]);

            // Initialize UIActivityViewController
            let cls = class!(UIActivityViewController);
            let activity_controller: *mut NSObject = unsafe { msg_send![cls, alloc] };
            let activity_controller: *mut NSObject = unsafe {
                msg_send![activity_controller, initWithActivityItems:&*items, applicationActivities: ptr::null_mut::<NSArray<NSObject>>()]
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
                let _: () = unsafe { msg_send![&*popover, setSourceView: &*view] };
                let _: () = unsafe {
                    msg_send![
                        &*popover,
                        setSourceRect: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0)),
                    ]
                };
            }

            // Present the share sheet
            let ui_app = class!(UIApplication);
            let shared_app: *mut NSObject = unsafe { msg_send![ui_app, sharedApplication] };
            let key_window: *mut NSObject = unsafe { msg_send![shared_app, keyWindow] };
            let root_vc: *mut NSObject = unsafe { msg_send![key_window, rootViewController] };
            let _: () = unsafe {
                msg_send![
                    root_vc,
                    presentViewController:activity_controller,
                    animated:true,
                    completion: ptr::null_mut::<objc2::runtime::AnyObject>()
                ]
            };
        });
    }
}