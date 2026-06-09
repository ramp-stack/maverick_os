use std::ffi::c_void;
use std::ffi::CString;
use std::sync::OnceLock;
use block2::RcBlock;
use objc2::{class, msg_send, sel};
use objc2::declare::ClassBuilder;
use objc2::rc::autoreleasepool;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2_foundation::{NSArray, NSObject};
use image::RgbaImage;

#[derive(Debug, Clone)]
pub enum ImageOrientation {
    Up,
    Down,
    Left,
    Right,
    UpMirrored,
    DownMirrored,
    LeftMirrored,
    RightMirrored,
}

impl ImageOrientation {
    pub fn from_ios_value(orientation: i64) -> Self {
        match orientation {
            0 => ImageOrientation::Up,
            1 => ImageOrientation::Down,
            2 => ImageOrientation::Left,
            3 => ImageOrientation::Right,
            4 => ImageOrientation::UpMirrored,
            5 => ImageOrientation::DownMirrored,
            6 => ImageOrientation::LeftMirrored,
            7 => ImageOrientation::RightMirrored,
            _ => ImageOrientation::Up,
        }
    }

    pub fn apply_to(&self, image: image::DynamicImage) -> image::DynamicImage {
        match self {
            ImageOrientation::Up => image,
            ImageOrientation::Down => image.rotate180(),
            ImageOrientation::Left => image.rotate270(),
            ImageOrientation::Right => image.rotate90(),
            ImageOrientation::UpMirrored => image.fliph(),
            ImageOrientation::DownMirrored => image.fliph().rotate180(),
            ImageOrientation::LeftMirrored => image.fliph().rotate90(),
            ImageOrientation::RightMirrored => image.fliph().rotate270(),
        }
    }
}

#[derive(Clone)]
pub struct OsPhotoPicker;

impl OsPhotoPicker {
    pub fn open(callback: impl FnOnce(Option<RgbaImage>) + Send + 'static) {
        let callback_box: Box<Box<dyn FnOnce(Option<RgbaImage>) + Send + 'static>> =
            Box::new(Box::new(callback));
        let callback_ptr = Box::into_raw(callback_box) as usize;

        dispatch2::DispatchQueue::main().exec_async(move || {
            let callback_ptr = callback_ptr as *mut c_void;
            autoreleasepool(|_| unsafe {
                let config_cls = class!(PHPickerConfiguration);
                let config: *mut AnyObject = msg_send![config_cls, new];

                let filter_cls = class!(PHPickerFilter);
                let images_filter: *mut AnyObject = msg_send![filter_cls, imagesFilter];
                let _: () = msg_send![config, setFilter: images_filter];

                let picker_cls = class!(PHPickerViewController);
                let picker: *mut AnyObject = msg_send![picker_cls, alloc];
                let picker: *mut AnyObject = msg_send![picker, initWithConfiguration: config];

                let delegate = create_photo_picker_delegate(callback_ptr);
                objc2::ffi::objc_retain(delegate as *mut _);
                let _: () = msg_send![picker, setDelegate: delegate];

                let ui_app = class!(UIApplication);
                let shared_app: *mut AnyObject = msg_send![ui_app, sharedApplication];
                let windows: *mut AnyObject = msg_send![shared_app, windows];
                let window: *mut AnyObject = msg_send![windows, firstObject];
                let root_vc: *mut AnyObject = msg_send![window, rootViewController];

                let null_block: *mut AnyObject = std::ptr::null_mut();
                let _: () = msg_send![
                    root_vc,
                    presentViewController: picker,
                    animated: true,
                    completion: null_block,
                ];
            });
        });
    }
}

fn create_photo_picker_delegate(callback_ptr: *mut c_void) -> *mut AnyObject {
    static DELEGATE_CLASS: OnceLock<usize> = OnceLock::new();

    unsafe {
        let cls = *DELEGATE_CLASS.get_or_init(|| {
            let superclass = class!(NSObject);
            let name = c"RustPHPickerDelegate";
            let mut decl = ClassBuilder::new(name, superclass).unwrap();

            decl.add_ivar::<*mut c_void>(c"rustCallbackPtr");

            extern "C" fn picker_did_finish_picking(
                this: &AnyObject,
                _cmd: Sel,
                picker: *mut AnyObject,
                results: *mut AnyObject,
            ) {
                unsafe {
                    let null_block: *mut AnyObject = std::ptr::null_mut();
                    let _: () = msg_send![picker, dismissViewControllerAnimated: true, completion: null_block];

                    objc2::ffi::objc_release(this as *const _ as *mut _);

                    let ivar_name = c"rustCallbackPtr";
                    let ivar = this.class().instance_variable(ivar_name).unwrap();
                    let callback_ptr = *ivar.load::<*mut c_void>(this);

                    if callback_ptr.is_null() {
                        return;
                    }

                    let callback_box: Box<Box<dyn FnOnce(Option<RgbaImage>) + Send + 'static>> =
                        Box::from_raw(callback_ptr as *mut _);

                    let results_array: &NSArray<NSObject> = &*(results as *const NSArray<NSObject>);
                    if results_array.count() == 0 {
                        callback_box(None);
                        return;
                    }

                    let result: *mut NSObject = msg_send![results_array, objectAtIndex: 0usize];
                    let item_provider: *mut AnyObject = msg_send![result, itemProvider];

                    let uiimage_class = class!(UIImage);
                    let can_load: bool = msg_send![item_provider, canLoadObjectOfClass: uiimage_class];
                    if !can_load {
                        callback_box(None);
                        return;
                    }

                    // Wrap in Option so the Fn closure can take it once
                    let callback_box = std::sync::Mutex::new(Some(callback_box));

                    let rc_block = RcBlock::new(move |image_obj: *mut AnyObject, _error: *mut AnyObject| {
                        // Take the callback out — subsequent calls (if any) will be no-ops
                        let Some(callback_box) = callback_box.lock().unwrap().take() else {
                            return;
                        };

                        let rgba = if !image_obj.is_null() {
                            let orientation: i64 = msg_send![image_obj, imageOrientation];
                            let symbol_name = CString::new("UIImagePNGRepresentation").unwrap();
                            let func_ptr = libc::dlsym(libc::RTLD_DEFAULT, symbol_name.as_ptr());
                            if func_ptr.is_null() {
                                None
                            } else {
                                let uiimage_png_rep_fn: extern "C" fn(*mut AnyObject) -> *mut AnyObject =
                                    std::mem::transmute(func_ptr);
                                let nsdata: *mut AnyObject = uiimage_png_rep_fn(image_obj);
                                if !nsdata.is_null() {
                                    let bytes_ptr: *const c_void = msg_send![nsdata, bytes];
                                    let length: usize = msg_send![nsdata, length];
                                    let bytes = std::slice::from_raw_parts(bytes_ptr as *const u8, length);
                                    image::load_from_memory(bytes)
                                        .map(|img| ImageOrientation::from_ios_value(orientation).apply_to(img).into_rgba8())
                                        .ok()
                                } else {
                                    None
                                }
                            }
                        } else {
                            None
                        };

                        callback_box(rgba);
                    });

                    let _: *mut AnyObject = msg_send![
                        item_provider,
                        loadObjectOfClass: uiimage_class,
                        completionHandler: &*rc_block
                    ];
                }
            }

            decl.add_method(
                sel!(picker:didFinishPicking:),
                picker_did_finish_picking as extern "C" fn(&'static AnyObject, Sel, *mut AnyObject, *mut AnyObject),
            );

            // Fix: double-cast through raw pointer to satisfy the compiler
            decl.register() as *const AnyClass as usize
        }) as *const AnyClass;

        let delegate: *mut AnyObject = msg_send![cls, new];
        let ivar_name = c"rustCallbackPtr";
        let ivar = (*cls).instance_variable(ivar_name).unwrap();
        let ivar_ref: &mut *mut c_void = ivar.load_mut(&mut *delegate);
        *ivar_ref = callback_ptr;

        delegate
    }
}