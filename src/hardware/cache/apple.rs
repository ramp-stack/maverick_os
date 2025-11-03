use std::path::PathBuf;
use objc2_foundation::{NSString, NSURL, NSError, NSDictionary, NSAutoreleasePool, NSFileManager, NSSearchPathDirectory, NSSearchPathDomainMask};
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::Bool;

pub struct OsApplicationSupport;

impl OsApplicationSupport {
    pub fn get() -> Option<PathBuf> {
        Self::get_app_name("org.ramp.orange")
    }

    pub fn get_app_name(app_name: &str) -> Option<PathBuf> {
        unsafe {
            let _pool = NSAutoreleasePool::new();

            let file_manager = NSFileManager::defaultManager();

            let url: Result<Retained<NSURL>, Retained<NSError>> = file_manager.URLForDirectory_inDomain_appropriateForURL_create_error(
                NSSearchPathDirectory::ApplicationSupportDirectory,
                NSSearchPathDomainMask::UserDomainMask,
                None,
                true
            );

            if let Ok(mut url) = url {
                let identifier = NSString::from_str(app_name);
                let subpath: Retained<NSURL> = msg_send![&*url, URLByAppendingPathComponent: Retained::<NSString>::as_ptr(&identifier)];
                url = subpath;

                let _: Bool = msg_send![&*file_manager,
                    createDirectoryAtURL: &*url,
                    withIntermediateDirectories: true,
                    attributes: std::ptr::null::<NSDictionary>(),
                    error: std::ptr::null_mut::<*mut NSError>()
                ];

                let path: *mut NSString = msg_send![&*url, path];
                if !path.is_null() {
                    let str_path = (*path).to_string();
                    return Some(PathBuf::from(str_path));
                }
            }

            None
        }
    }
}