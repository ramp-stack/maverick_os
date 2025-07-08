#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2_foundation::{NSString, NSURL};
#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2::msg_send;
use std::path::PathBuf;
#[cfg(target_os = "ios")]
use objc2::__framework_prelude::AnyObject;
#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2::rc::Retained;
#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2::runtime::Bool;

#[cfg(target_os = "ios")]
use std::ffi::CStr;
#[cfg(target_os = "ios")]
use objc2::runtime::AnyClass;

#[cfg(target_os = "macos")]
use objc2_foundation::{NSError, NSDictionary, NSAutoreleasePool, NSFileManager, NSSearchPathDirectory, NSSearchPathDomainMask};

#[cfg(any(target_os = "linux", target_os = "windows"))]
use std::env;
#[cfg(any(target_os = "linux", target_os = "windows"))]
use std::fs;

#[cfg(target_os = "ios")]
const NS_APPLICATION_SUPPORT_DIRECTORY: usize = 14;
#[cfg(target_os = "ios")]
const NS_USER_DOMAIN_MASK: usize = 1;

#[derive(Clone)]
pub struct ApplicationSupport;

impl ApplicationSupport {
    #[cfg(target_os = "ios")]
    pub fn get() -> Option<PathBuf> {
        unsafe {
            let file_manager_class = AnyClass::get(c"NSFileManager").unwrap();
            let file_manager: *mut AnyObject = msg_send![file_manager_class, defaultManager];

            let mut error: *mut AnyObject = std::ptr::null_mut();

            let url: *mut NSURL = msg_send![
                file_manager,
                URLForDirectory: NS_APPLICATION_SUPPORT_DIRECTORY,
                inDomain: NS_USER_DOMAIN_MASK,
                appropriateForURL: std::ptr::null::<AnyObject>(),
                create: true,
                error: &mut error,
            ];

            if url.is_null() {
                return None;
            } 

            let path_nsstring: *mut NSString = msg_send![url, path];
            if path_nsstring.is_null() {
                return None;
            }

            let c_str: *const std::os::raw::c_char = msg_send![path_nsstring, UTF8String];
            if c_str.is_null() {
                return None;
            }

            let path = CStr::from_ptr(c_str).to_string_lossy().into_owned();
            Some(PathBuf::from(path))
        }
    }

    #[cfg(target_os = "macos")]
    pub fn get() -> Option<PathBuf> {
        // unsafe {
        //     let _pool = NSAutoreleasePool::new();
        //     let file_manager = NSFileManager::defaultManager();
        //     let url: Result<Retained<NSURL>, Retained<NSError>> = file_manager.URLForDirectory_inDomain_appropriateForURL_create_error(
        //         NSSearchPathDirectory::ApplicationSupportDirectory,
        //         NSSearchPathDomainMask::UserDomainMask,
        //         None,
        //         true
        //     );
        //     if let Ok(mut url) = url {
        //         let bundle: *mut AnyObject = msg_send![objc2::class!(NSBundle), mainBundle];
        //         let identifier: *mut NSString = msg_send![bundle, bundleIdentifier];
        //         let identifier = if !identifier.is_null() {
        //             Retained::retain(identifier).unwrap()
        //         } else {
        //             NSString::from_str("org.ramp.orange")
        //         };
        //         let subpath: Retained<NSURL> = msg_send![&*url, URLByAppendingPathComponent: Retained::<NSString>::as_ptr(&identifier)];
        //         url = subpath;
        //         let _: Bool = msg_send![&*file_manager,
        //             createDirectoryAtURL: &*url,
        //             withIntermediateDirectories: true,
        //             attributes: std::ptr::null::<NSDictionary>(),
        //             error: std::ptr::null_mut::<*mut NSError>()
        //         ];
        //         let path: *mut NSString = msg_send![&*url, path];
        //         if !path.is_null() {
        //             let str_path = (*path).to_string();
        //             return Some(PathBuf::from(str_path));
        //         }
        //     }

        //     None
        // }
        Some(PathBuf::from("./"))
    }

    #[cfg(target_os = "linux")]
    pub fn get() -> Option<PathBuf> {
        let app_name = "org.ramp.orange";

        if let Ok(xdg_data_home) = env::var("XDG_DATA_HOME") {
            let path = PathBuf::from(xdg_data_home).join(app_name);
            if let Ok(()) = fs::create_dir_all(&path) {
                return Some(path);
            }
        }

        if let Ok(home) = env::var("HOME") {
            let path = PathBuf::from(home)
                .join(".local")
                .join("share")
                .join(app_name);

            if let Ok(()) = fs::create_dir_all(&path) {
                return Some(path);
            }
        }

        None
    }

    #[cfg(target_os = "windows")]
    pub fn get() -> Option<PathBuf> {
        let app_name = "org.ramp.orange";

        if let Ok(appdata) = env::var("APPDATA") {
            let path = PathBuf::from(appdata).join(app_name);
            if let Ok(()) = fs::create_dir_all(&path) {
                return Some(path);
            }
        }

        if let Ok(userprofile) = env::var("USERPROFILE") {
            let path = PathBuf::from(userprofile)
                .join("AppData")
                .join("Roaming")
                .join(app_name);

            if let Ok(()) = fs::create_dir_all(&path) {
                return Some(path);
            }
        }

        None
    }

    #[cfg(target_os = "macos")]
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

    #[cfg(target_os = "linux")]
    pub fn get_app_name(app_name: &str) -> Option<PathBuf> {
        if let Ok(xdg_data_home) = env::var("XDG_DATA_HOME") {
            let path = PathBuf::from(xdg_data_home).join(app_name);
            if let Ok(()) = fs::create_dir_all(&path) {
                return Some(path);
            }
        }

        if let Ok(home) = env::var("HOME") {
            let path = PathBuf::from(home)
                .join(".local")
                .join("share")
                .join(app_name);

            if let Ok(()) = fs::create_dir_all(&path) {
                return Some(path);
            }
        }

        None
    }

    #[cfg(target_os = "windows")]
    pub fn get_app_name(app_name: &str) -> Option<PathBuf> {
        if let Ok(appdata) = env::var("APPDATA") {
            let path = PathBuf::from(appdata).join(app_name);
            if let Ok(()) = fs::create_dir_all(&path) {
                return Some(path);
            }
        }

        if let Ok(userprofile) = env::var("USERPROFILE") {
            let path = PathBuf::from(userprofile)
                .join("AppData")
                .join("Roaming")
                .join(app_name);

            if let Ok(()) = fs::create_dir_all(&path) {
                return Some(path);
            }
        }

        None
    }
}