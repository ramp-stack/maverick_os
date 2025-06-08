use std::sync::mpsc::Sender;

#[cfg(target_os = "ios")]
use block::{ConcreteBlock, RcBlock};
#[cfg(target_os = "ios")]
use dispatch2;
#[cfg(target_os = "ios")]
use objc2::{
    class, msg_send, sel,
    rc::autoreleasepool,
    runtime::{AnyClass, AnyObject, ClassBuilder, Sel},
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

#[cfg(any(target_os = "linux", target_os = "windows"))]
use std::process::Command;
#[cfg(any(target_os = "linux", target_os = "windows"))]
use std::path::Path;
#[cfg(any(target_os = "linux", target_os = "windows"))]
use std::fs;
#[cfg(any(target_os = "linux", target_os = "windows"))]
use std::thread;

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
    pub fn open(_sender: Sender<(Vec<u8>, ImageOrientation)>) {
        // TODO: Implement macOS native file picker
    }

    #[cfg(target_os = "linux")]
    pub fn open(sender: Sender<(Vec<u8>, ImageOrientation)>) {
        thread::spawn(move || {
            // Try different Linux file dialog options in order of preference
            let result = Self::try_zenity()
                .or_else(|| Self::try_kdialog())
                .or_else(|| Self::try_xdg_open());

            match result {
                Some(file_path) => {
                    if let Ok(image_data) = fs::read(&file_path) {
                        // Try to determine orientation from EXIF data or default to Up
                        let orientation = Self::get_image_orientation(&file_path);
                        let _ = sender.send((image_data, orientation));
                    } else {
                        let _ = sender.send((Vec::new(), ImageOrientation::Up));
                    }
                }
                None => {
                    let _ = sender.send((Vec::new(), ImageOrientation::Up));
                }
            }
        });
    }

    #[cfg(target_os = "windows")]
    pub fn open(sender: Sender<(Vec<u8>, ImageOrientation)>) {
        thread::spawn(move || {
            let result = Command::new("powershell")
                .args(&[
                    "-Command",
                    r#"
                    Add-Type -AssemblyName System.Windows.Forms;
                    $dialog = New-Object System.Windows.Forms.OpenFileDialog;
                    $dialog.Filter = 'Image Files|*.jpg;*.jpeg;*.png;*.gif;*.bmp;*.tiff;*.webp|All Files|*.*';
                    $dialog.Title = 'Select an Image';
                    $dialog.Multiselect = $false;
                    if ($dialog.ShowDialog() -eq 'OK') {
                        Write-Output $dialog.FileName
                    }
                    "#,
                ])
                .output();

            match result {
                Ok(output) if output.status.success() => {
                    let file_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !file_path.is_empty() && Path::new(&file_path).exists() {
                        if let Ok(image_data) = fs::read(&file_path) {
                            let orientation = Self::get_image_orientation(&file_path);
                            let _ = sender.send((image_data, orientation));
                        } else {
                            let _ = sender.send((Vec::new(), ImageOrientation::Up));
                        }
                    } else {
                        let _ = sender.send((Vec::new(), ImageOrientation::Up));
                    }
                }
                _ => {
                    let _ = sender.send((Vec::new(), ImageOrientation::Up));
                }
            }
        });
    }

    #[cfg(target_os = "android")]
    pub fn open(_sender: Sender<(Vec<u8>, ImageOrientation)>) {
        // TODO: Implement Android photo picker
    }

    #[cfg(target_os = "linux")]
    fn try_zenity() -> Option<String> {
        let result = Command::new("zenity")
            .args(&[
                "--file-selection",
                "--file-filter=Image files | *.jpg *.jpeg *.png",
                "--file-filter=All files | *",
                "--title=Select an Image",
            ])
            .output();

        match result {
            Ok(output) if output.status.success() => {
                let file_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !file_path.is_empty() && Path::new(&file_path).exists() {
                    Some(file_path)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    #[cfg(target_os = "linux")]
    fn try_kdialog() -> Option<String> {
        let result = Command::new("kdialog")
            .args(&[
                "--getopenfilename",
                ".",
                "*.jpg *.jpeg *.png *.webp|Image files",
                "--title",
                "Select an Image",
            ])
            .output();

        match result {
            Ok(output) if output.status.success() => {
                let file_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !file_path.is_empty() && Path::new(&file_path).exists() {
                    Some(file_path)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    #[cfg(target_os = "linux")]
    fn try_xdg_open() -> Option<String> {
        // As a fallback, we could try to open a simple file manager
        // This won't return a selected file, so it's not ideal
        // You might want to implement a simple terminal-based file picker here
        None
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    fn get_image_orientation(file_path: &str) -> ImageOrientation {
        let path = Path::new(file_path);
        if let Some(extension) = path.extension() {
            match extension.to_str().unwrap_or("").to_lowercase().as_str() {
                "jpg" | "jpeg" => {
                    ImageOrientation::Up
                }
                _ => ImageOrientation::Up,
            }
        } else {
            ImageOrientation::Up
        }
    }

    #[cfg(target_os = "ios")]
    pub fn open(sender: Sender<(Vec<u8>, ImageOrientation)>) {
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
    /// Convert iOS UIImageOrientation value to ImageOrientation enum
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