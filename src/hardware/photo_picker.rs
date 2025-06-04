use std::sync::mpsc::Sender;

#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2::{msg_send, class};
#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2::runtime::{AnyObject, AnyClass};
#[cfg(target_os = "ios")]
use block::{ConcreteBlock, RcBlock};
#[cfg(target_os = "ios")]
use dispatch2;
#[cfg(target_os = "ios")]
use objc2::{
    sel, rc::autoreleasepool,
    runtime::{ClassBuilder, Sel},
};
#[cfg(target_os = "ios")]
use objc2_foundation::NSArray;
#[cfg(target_os = "ios")]
use objc2::__framework_prelude::NSObject;
#[cfg(target_os = "ios")]
use objc2::ffi::objc_retain;
#[cfg(target_os = "ios")]
use std::ffi::c_void;
#[cfg(target_os = "ios")]
use std::ffi::{CStr, CString};

#[cfg(target_os = "macos")]
use objc2_foundation::{NSString, NSArray, NSObject};
#[cfg(target_os = "macos")]
use std::path::PathBuf;
#[cfg(target_os = "macos")]
use std::fs;
use std::f64::consts::{FRAC_PI_2, PI};
use objc2::rc::{Retained, autoreleasepool};

#[derive(Clone)]
pub struct PhotoPicker;

#[cfg(target_os = "ios")]
#[derive(Clone, Copy)]
struct SenderPtr(usize);

#[cfg(target_os = "ios")]
unsafe impl Send for SenderPtr {}
#[cfg(target_os = "ios")]
unsafe impl Sync for SenderPtr {}

impl PhotoPicker {
    #[cfg(target_os = "macos")]
    pub fn open(sender: Sender<(Vec<u8>, ImageOrientation)>) {
        dispatch2::DispatchQueue::main().exec_async(move || {
            autoreleasepool(|_| unsafe {

                let cls: *const AnyClass = class!(NSOpenPanel);
                if cls.is_null() {
                    eprintln!("NSOpenPanel class not found");
                    // let _ = sender.send((Vec::new(), ImageOrientation::Up));
                    return;
                }

                let panel: *mut AnyObject = msg_send![cls, openPanel];
                if panel.is_null() {
                    eprintln!("Failed to create NSOpenPanel");
                    // let _ = sender.send((Vec::new(), ImageOrientation::Up));
                    return;
                }

                let () = msg_send![panel, setCanChooseFiles: true];
                let () = msg_send![panel, setAllowsMultipleSelection: false];
                let () = msg_send![panel, setCanChooseDirectories: false];

                let png_str: Retained<NSString> = NSString::from_str("png");
                let file_types: Retained<NSArray<NSString>> = NSArray::from_slice(&[png_str.as_ref()]);
                let () = msg_send![panel, setAllowedFileTypes: &*file_types];

                const NS_MODAL_RESPONSE_OK: i64 = 1;
                let response: i64 = msg_send![panel, runModal];
                if response != NS_MODAL_RESPONSE_OK {
                    // let _ = sender.send((Vec::new(), ImageOrientation::Up));
                    return;
                }

                let url: *mut AnyObject = msg_send![panel, URL];
                if url.is_null() {
                    eprintln!("URL was null");
                    // let _ = sender.send((Vec::new(), ImageOrientation::Up));
                    return;
                }

                let nsstring: *mut NSString = msg_send![url, path];
                if nsstring.is_null() {
                    eprintln!("Path string was null");
                    // let _ = sender.send((Vec::new(), ImageOrientation::Up));
                    return;
                }

                let rust_path = (*nsstring).to_string();
                let path = PathBuf::from(rust_path);

                match fs::read(&path) {
                    Ok(png_data) => {sender.send((png_data, ImageOrientation::Up));},
                    Err(err) => eprintln!("Failed to read file: {err}")
                }
            });
        });
    }

    #[cfg(target_os = "linux")]
    pub fn open(_sender: Sender<(Vec<u8>, ImageOrientation)>) {}
    #[cfg(target_os = "android")]
    pub fn open(_sender: Sender<(Vec<u8>, ImageOrientation)>) {}

    #[cfg(target_os = "ios")]
    pub fn open(sender: Sender<(Vec<u8>, ImageOrientation)>) {
        println!("STARTED");
        println!("ATTEMPTING TO OPEN PHOTO PICKER");
        let sender_box = Box::new(sender);
        let sender_ptr = SenderPtr(Box::into_raw(sender_box) as usize);

        dispatch2::DispatchQueue::main().exec_async(move || {
            // Now we cast it back into a raw pointer safely
            let sender_ptr = sender_ptr.0 as *mut c_void;
            println!("Started dispatcher");
            autoreleasepool(|_| unsafe {
                println!("Inside autorelease pool");

                let config_cls = class!(PHPickerConfiguration);
                let config: *mut AnyObject = msg_send![config_cls, new];

                let filter_cls = class!(PHPickerFilter);
                let images_filter: *mut AnyObject = msg_send![filter_cls, imagesFilter];
                let _: () = msg_send![config, setFilter: images_filter];

                let picker_cls = class!(PHPickerViewController);
                let picker: *mut AnyObject = msg_send![picker_cls, alloc];
                let picker: *mut AnyObject = msg_send![picker, initWithConfiguration: config];

                let delegate = create_photo_picker_delegate(sender_ptr);
                let _: () = msg_send![picker, setDelegate: delegate];

                let ui_app = class!(UIApplication);
                let shared_app: *mut AnyObject = msg_send![ui_app, sharedApplication];
                let windows: *mut AnyObject = msg_send![shared_app, windows];
                let window: *mut AnyObject = msg_send![windows, firstObject];
                let root_vc: *mut AnyObject = msg_send![window, rootViewController];

                println!("Presenting picker from: {:p}", root_vc);

                let null_block: *mut AnyObject = std::ptr::null_mut();
                let _: () = msg_send![
                    root_vc,
                    presentViewController: picker,
                    animated: true,
                    completion: null_block,
                ];
            });
        });

        println!("OK HERE NOW");
    }
}

#[cfg(target_os = "ios")]
fn create_photo_picker_delegate(sender_ptr: *mut c_void) -> *mut AnyObject {
    static mut DELEGATE_CLASS: *const AnyClass = std::ptr::null();

    unsafe {
        if DELEGATE_CLASS.is_null() {
            let superclass = class!(NSObject);
            let name = CStr::from_bytes_with_nul(b"RustPHPickerDelegate\0").unwrap();
            let mut decl = ClassBuilder::new(name, superclass).unwrap();

            decl.add_ivar::<*mut c_void>(CStr::from_bytes_with_nul(b"rustSenderPtr\0").unwrap());

            extern "C" fn picker_did_finish_picking(
                this: &AnyObject,
                _cmd: Sel,
                picker: *mut AnyObject,
                results: *mut AnyObject,
            ) {
                unsafe {
                    let null_block: *mut AnyObject = std::ptr::null_mut();

                    let _: () = msg_send![picker, dismissViewControllerAnimated: true, completion: null_block];

                    let results_array: &NSArray<NSObject> = &*(results as *const NSArray<NSObject>);
                    if results_array.count() == 0 {
                        return;
                    }

                    let result: *mut NSObject = msg_send![results_array, objectAtIndex: 0usize];
                    let item_provider: *mut AnyObject = msg_send![result, itemProvider];

                    let ivar_name = CStr::from_bytes_with_nul(b"rustSenderPtr\0").unwrap();
                    let ivar = this.class().instance_variable(ivar_name).unwrap();
                    let sender_ptr = *ivar.load::<*mut c_void>(this);

                    if sender_ptr.is_null() {
                        return;
                    }

                    let sender_box: Box<Sender<(Vec<u8>, ImageOrientation)>> = Box::from_raw(sender_ptr as *mut _);

                    let uiimage_class = class!(UIImage);
                    let can_load: bool = msg_send![item_provider, canLoadObjectOfClass: uiimage_class];
                    if !can_load {
                        let _ = sender_box.send((Vec::new(), ImageOrientation::Up));
                        return;
                    }

                    let block = ConcreteBlock::new(move |image_obj: *mut AnyObject, _error: *mut AnyObject| {
                        let (data, orientation) = if !image_obj.is_null() {
                            let orientation: i64 = unsafe { msg_send![image_obj, imageOrientation] };
                            // let image_obj = rotated_from(orientation, image_obj);

                            let symbol_name = CString::new("UIImagePNGRepresentation").unwrap();
                            let func_ptr = libc::dlsym(libc::RTLD_DEFAULT, symbol_name.as_ptr());
                            if func_ptr.is_null() {
                                (Vec::new(), orientation)
                            } else {
                                let uiimage_png_rep_fn: extern "C" fn(*mut AnyObject) -> *mut AnyObject =
                                    std::mem::transmute(func_ptr);
                                let nsdata: *mut AnyObject = uiimage_png_rep_fn(image_obj);
                                if !nsdata.is_null() {
                                    let bytes_ptr: *const c_void = msg_send![nsdata, bytes];
                                    let length: usize = msg_send![nsdata, length];
                                    (std::slice::from_raw_parts(bytes_ptr as *const u8, length).to_vec(), orientation)
                                } else {
                                    (Vec::new(), orientation)
                                }
                            }
                        } else {
                            (Vec::new(), 0)
                        };

                        let _ = sender_box.send((data, ImageOrientation::get(orientation)));
                    });

                    let rc_block: RcBlock<(*mut AnyObject, *mut AnyObject), ()> = block.copy();
                    let block_ptr: *mut AnyObject = (&*rc_block) as *const _ as *mut AnyObject;
                    objc_retain(block_ptr);

                    let _: *mut AnyObject = msg_send![
                        item_provider,
                        loadObjectOfClass: uiimage_class,
                        completionHandler: block_ptr
                    ];
                }
            }

            decl.add_method(
                sel!(picker:didFinishPicking:),
                picker_did_finish_picking as extern "C" fn(&'static AnyObject, Sel, *mut AnyObject, *mut AnyObject),
            );

            DELEGATE_CLASS = decl.register();
        }

        let delegate: &mut AnyObject = msg_send![DELEGATE_CLASS, new];

        let ivar_name = CStr::from_bytes_with_nul(b"rustSenderPtr\0").unwrap();
        let ivar = (*DELEGATE_CLASS).instance_variable(ivar_name).unwrap();
        let ivar_ref: &mut *mut c_void = ivar.load_mut(delegate);
        *ivar_ref = sender_ptr;

        delegate
    }
}

#[derive(Debug)]
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
    fn get(orientation: i64) -> Self {
        match orientation {
            0 => return ImageOrientation::Up,
            1 => return ImageOrientation::Down,
            2 => return ImageOrientation::Left,
            3 => return ImageOrientation::Right,
            4 => return ImageOrientation::UpMirrored,
            5 => return ImageOrientation::DownMirrored,
            6 => return ImageOrientation::LeftMirrored,
            7 => return ImageOrientation::RightMirrored,
            _ => return ImageOrientation::Up,
        };
    }

    pub fn apply_to(&self, image: image::DynamicImage) -> image::DynamicImage {
        match self {
            ImageOrientation::Up => return image,
            ImageOrientation::Down => return image.rotate180(),
            ImageOrientation::Left => return image.rotate270(),
            ImageOrientation::Right => return image.rotate90(),
            ImageOrientation::UpMirrored => return image.fliph(),
            ImageOrientation::DownMirrored => return image.fliph().rotate180(),
            ImageOrientation::LeftMirrored => return image.fliph().rotate90(),
            ImageOrientation::RightMirrored => return image.fliph().rotate270()
        };
    }
}
