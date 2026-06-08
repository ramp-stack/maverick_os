use objc2::{class, msg_send, runtime::{AnyClass, AnyObject}};
use objc2_foundation::{NSArray, NSString};
use objc2::rc::{Retained, autoreleasepool};
use std::path::PathBuf;
use std::fs;
use image::RgbaImage;

#[derive(Clone)]
pub struct OsPhotoPicker;

impl OsPhotoPicker {
    pub fn open(callback: impl FnOnce(Option<RgbaImage>) + Send + 'static) {
        dispatch2::DispatchQueue::main().exec_async(move || {
            autoreleasepool(|_| unsafe {
                let cls: *const AnyClass = class!(NSOpenPanel);
                if cls.is_null() {
                    eprintln!("NSOpenPanel class not found");
                    callback(None);
                    return;
                }

                let panel: *mut AnyObject = msg_send![cls, openPanel];
                if panel.is_null() {
                    eprintln!("Failed to create NSOpenPanel");
                    callback(None);
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
                    jpeg_str.as_ref(),
                ]);
                let () = msg_send![panel, setAllowedFileTypes: &*file_types];

                const NS_MODAL_RESPONSE_OK: i64 = 1;
                let response: i64 = msg_send![panel, runModal];
                if response != NS_MODAL_RESPONSE_OK {
                    callback(None);
                    return;
                }

                let url: *mut AnyObject = msg_send![panel, URL];
                if url.is_null() {
                    eprintln!("URL was null");
                    callback(None);
                    return;
                }

                let nsstring: *mut NSString = msg_send![url, path];
                if nsstring.is_null() {
                    eprintln!("Path string was null");
                    callback(None);
                    return;
                }

                let rust_path = (*nsstring).to_string();
                let path = PathBuf::from(rust_path);

                match fs::read(&path) {
                    Ok(image_data) => {
                        let rgba = image::load_from_memory(&image_data)
                            .map(|img| img.into_rgba8())
                            .ok();
                        callback(rgba);
                    }
                    Err(err) => {
                        eprintln!("Failed to read file: {err}");
                        callback(None);
                    }
                }
            });
        });
    }
}