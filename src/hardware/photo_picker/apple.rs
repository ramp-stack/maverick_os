use std::sync::mpsc::Sender;
use super::ImageOrientation;
use std::ffi::c_void;
use std::ffi::CString;
use block::{ConcreteBlock, RcBlock};
use objc2::{class, msg_send, runtime::{AnyClass, AnyObject}};
use objc2::{sel, runtime::{ClassBuilder, Sel}};
use objc2_foundation::{NSArray, NSString};
use objc2::__framework_prelude::NSObject;
use objc2::ffi::objc_retain;
use objc2::rc::{Retained, autoreleasepool};
use std::path::PathBuf;
use std::fs;

#[derive(Clone)]
pub struct OsPhotoPicker;

#[derive(Clone, Copy)]
struct SenderPtr(usize);

unsafe impl Send for SenderPtr {}
unsafe impl Sync for SenderPtr {}

impl OsPhotoPicker {
    // macOS implementation
    pub fn open(sender: Sender<(Vec<u8>, ImageOrientation)>) {
        // Detect if we're on macOS or iOS at runtime
        #[cfg(target_os = "macos")]
        Self::open_macos(sender);
        
        #[cfg(target_os = "ios")]
        Self::open_ios(sender);
    }
    
    #[cfg(target_os = "macos")]
    fn open_macos(sender: Sender<(Vec<u8>, ImageOrientation)>) {
        dispatch2::DispatchQueue::main().exec_async(move || {
            autoreleasepool(|_| unsafe {
                let cls: *const AnyClass = class!(NSOpenPanel);
                if cls.is_null() {
                    eprintln!("NSOpenPanel class not found");
                    let _ = sender.send((Vec::new(), ImageOrientation::Up));
                    return;
                }

                let panel: *mut AnyObject = msg_send![cls, openPanel];
                if panel.is_null() {
                    eprintln!("Failed to create NSOpenPanel");
                    let _ = sender.send((Vec::new(), ImageOrientation::Up));
                    return;
                }

                let () = msg_send![panel, setCanChooseFiles: true];
                let () = msg_send![panel, setAllowsMultipleSelection: false];
                let () = msg_send![panel, setCanChooseDirectories: false];

                let png_str: Retained<NSString> = NSString::from_str("png");
                let jpg_str: Retained<NSString> = NSString::from_str("jpg");
                let jpeg_str: Retained<NSString> = NSString::from_str("jpeg");
                let file_types: Retained<NSArray<NSString>> = NSArray::from_slice(&[
                    png_str.as_ref(),
                    jpg_str.as_ref(),
                    jpeg_str.as_ref()
                ]);
                let () = msg_send![panel, setAllowedFileTypes: &*file_types];

                const NS_MODAL_RESPONSE_OK: i64 = 1;
                let response: i64 = msg_send![panel, runModal];
                if response != NS_MODAL_RESPONSE_OK {
                    let _ = sender.send((Vec::new(), ImageOrientation::Up));
                    return;
                }

                let url: *mut AnyObject = msg_send![panel, URL];
                if url.is_null() {
                    eprintln!("URL was null");
                    let _ = sender.send((Vec::new(), ImageOrientation::Up));
                    return;
                }

                let nsstring: *mut NSString = msg_send![url, path];
                if nsstring.is_null() {
                    eprintln!("Path string was null");
                    let _ = sender.send((Vec::new(), ImageOrientation::Up));
                    return;
                }

                let rust_path = (*nsstring).to_string();
                let path = PathBuf::from(rust_path);

                match fs::read(&path) {
                    Ok(image_data) => {
                        let _ = sender.send((image_data, ImageOrientation::Up));
                    },
                    Err(err) => {
                        eprintln!("Failed to read file: {err}");
                        let _ = sender.send((Vec::new(), ImageOrientation::Up));
                    }
                }
            });
        });
    }

    #[cfg(target_os = "ios")]
    fn open_ios(sender: Sender<(Vec<u8>, ImageOrientation)>) {
        println!("STARTED");
        println!("ATTEMPTING TO OPEN PHOTO PICKER");
        let sender_box = Box::new(sender);
        let sender_ptr = SenderPtr(Box::into_raw(sender_box) as usize);

        dispatch2::DispatchQueue::main().exec_async(move || {
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
            let name = c"RustPHPickerDelegate";
            let mut decl = ClassBuilder::new(name, superclass).unwrap();

            decl.add_ivar::<*mut c_void>(c"rustSenderPtr");

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

                    let ivar_name = c"rustSenderPtr";
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
                            let orientation: i64 = msg_send![image_obj, imageOrientation];

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

                        let _ = sender_box.send((data, ImageOrientation::from_ios_value(orientation)));
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

        let ivar_name = c"rustSenderPtr";
        let ivar = (*DELEGATE_CLASS).instance_variable(ivar_name).unwrap();
        let ivar_ref: &mut *mut c_void = ivar.load_mut(delegate);
        *ivar_ref = sender_ptr;

        delegate
    }
}