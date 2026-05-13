use image::RgbaImage;
use objc2::rc::autoreleasepool;
use objc2::{class, msg_send};
use objc2_foundation::{NSArray, NSObject, NSString, NSRect, NSPoint, NSSize};
use objc2::rc::Retained;

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

            let cls = class!(NSSharingServicePicker);
            let picker: *mut NSObject = unsafe { msg_send![cls, alloc] };
            let picker: *mut NSObject = unsafe {
                msg_send![picker, initWithItems: &*items]
            };

            let app: *mut NSObject = unsafe { msg_send![class!(NSApplication), sharedApplication] };
            let key_window: *mut NSObject = unsafe { msg_send![app, keyWindow] };
            let content_view: *mut NSObject = unsafe { msg_send![key_window, contentView] };

            let anchor_rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0));

            let _: () = unsafe {
                msg_send![
                    picker,
                    showRelativeToRect: anchor_rect,
                    ofView: content_view,
                    preferredEdge: 2u64
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
            let bytes_per_pixel = 4usize;
            let bytes_per_row = bytes_per_pixel * width as usize;
            let bits_per_component = 8usize;

            let buffer = rgba_image.as_raw();
            let color_space = unsafe{objc2_core_graphics::CGColorSpace::new_device_rgb()};

            let context = unsafe {
                CGBitmapContextCreate(
                    buffer.as_ptr() as *mut _,
                    width as usize,
                    height as usize,
                    bits_per_component,
                    bytes_per_row,
                    color_space.as_deref(),
                    CGImageAlphaInfo::PremultipliedLast.0,
                )
            }
            .expect("Failed to create bitmap context");

            let cg_image: Retained<CGImage> = unsafe{CGBitmapContextCreateImage(Some(&context))
                .expect("Could not create image from context")
                .into()};

            let ns_image_cls = class!(NSImage);
            let ns_image: *mut NSObject = unsafe { msg_send![ns_image_cls, alloc] };
            let ns_image: *mut NSObject = unsafe {
                msg_send![ns_image, initWithCGImage: &*cg_image, size: NSSize::new(width as f64, height as f64)]
            };

            let items = NSArray::from_slice(unsafe {
                std::slice::from_raw_parts(&ns_image as *const _ as *const &NSObject, 1)
            });

            let cls = class!(NSSharingServicePicker);
            let picker: *mut NSObject = unsafe { msg_send![cls, alloc] };
            let picker: *mut NSObject = unsafe {
                msg_send![picker, initWithItems: &*items]
            };

            let app: *mut NSObject = unsafe { msg_send![class!(NSApplication), sharedApplication] };
            let key_window: *mut NSObject = unsafe { msg_send![app, keyWindow] };
            let content_view: *mut NSObject = unsafe { msg_send![key_window, contentView] };

            let anchor_rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0));

            let _: () = unsafe {
                msg_send![
                    picker,
                    showRelativeToRect: anchor_rect,
                    ofView: content_view,
                    preferredEdge: 2u64
                ]
            };
        });
    }
}
