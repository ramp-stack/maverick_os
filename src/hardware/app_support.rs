#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2_foundation::{NSString, NSURL};
#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2::msg_send;
#[cfg(any(target_os = "ios", target_os = "macos"))]
use std::path::PathBuf;

#[cfg(target_os = "ios")]
use objc2::__framework_prelude::AnyObject;
#[cfg(target_os = "ios")]
use std::ffi::CStr;
#[cfg(target_os = "ios")]
use objc2::runtime::AnyClass;

#[cfg(target_os = "macos")]
use objc2_foundation::{NSError, NSDictionary, NSAutoreleasePool, NSFileManager, NSSearchPathDirectory, NSSearchPathDomainMask};
#[cfg(target_os = "macos")]
use objc2::rc::Retained;
#[cfg(target_os = "macos")]
use objc2::runtime::Bool;

#[cfg(any(target_os = "linux", target_os = "windows"))]
use std::env;
#[cfg(any(target_os = "linux", target_os = "windows"))]
use std::fs;

#[cfg(target_os = "ios")]
const NS_APPLICATION_SUPPORT_DIRECTORY: usize = 14;
#[cfg(target_os = "ios")]
const NS_USER_DOMAIN_MASK: usize = 1;

/// Access the application support directory.
#[derive(Clone)]
pub struct ApplicationSupport;

impl ApplicationSupport {
    /// Get the general application support directory path
    pub fn get() -> Option<PathBuf> {
        #[cfg(target_os = "ios")]
        {
            Self::get_ios()
        }
        #[cfg(target_os = "macos")]
        {
            Self::get_macos()
        }
        #[cfg(target_os = "linux")]
        {
            Self::get_linux()
        }
        #[cfg(target_os = "windows")]
        {
            Self::get_windows()
        }
        #[cfg(not(any(target_os = "ios", target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            // Fallback for unsupported platforms
            Some(PathBuf::from("./app_data"))
        }
    }

    /// Get application support directory for a specific app name
    pub fn get_app_name(app_name: &str) -> Option<PathBuf> {
        #[cfg(target_os = "macos")]
        {
            Self::get_app_name_macos(app_name)
        }
        #[cfg(target_os = "linux")]
        {
            Self::get_app_name_linux(app_name)
        }
        #[cfg(target_os = "windows")]
        {
            Self::get_app_name_windows(app_name)
        }
        #[cfg(target_os = "ios")]
        {
            // For iOS, append the app name to the base path
            Self::get_ios().map(|base| base.join(app_name))
        }
        #[cfg(not(any(target_os = "ios", target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            // Fallback for unsupported platforms
            let path = PathBuf::from("./app_data").join(app_name);
            std::fs::create_dir_all(&path).ok()?;
            Some(path)
        }
    }

    #[cfg(target_os = "ios")]
    fn get_ios() -> Option<PathBuf> {
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
    fn get_macos() -> Option<PathBuf> {
        // For now, using the simple fallback since the original implementation was commented out
        // You can uncomment and fix the original implementation if needed
        Some(PathBuf::from("./"))
    }

    #[cfg(target_os = "linux")]
    fn get_linux() -> Option<PathBuf> {
        let app_name = "org.ramp.orange";

        if let Ok(xdg_data_home) = env::var("XDG_DATA_HOME") {
            let path = PathBuf::from(xdg_data_home).join(app_name);
            if fs::create_dir_all(&path).is_ok() {
                return Some(path);
            }
        }

        if let Ok(home) = env::var("HOME") {
            let path = PathBuf::from(home)
                .join(".local")
                .join("share")
                .join(app_name);

            if fs::create_dir_all(&path).is_ok() {
                return Some(path);
            }
        }

        None
    }

    #[cfg(target_os = "windows")]
    fn get_windows() -> Option<PathBuf> {
        let app_name = "org.ramp.orange";

        if let Ok(appdata) = env::var("APPDATA") {
            let path = PathBuf::from(appdata).join(app_name);
            if fs::create_dir_all(&path).is_ok() {
                return Some(path);
            }
        }

        if let Ok(userprofile) = env::var("USERPROFILE") {
            let path = PathBuf::from(userprofile)
                .join("AppData")
                .join("Roaming")
                .join(app_name);

            if fs::create_dir_all(&path).is_ok() {
                return Some(path);
            }
        }

        None
    }

    #[cfg(target_os = "macos")]
    fn get_app_name_macos(app_name: &str) -> Option<PathBuf> {
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
    fn get_app_name_linux(app_name: &str) -> Option<PathBuf> {
        if let Ok(xdg_data_home) = env::var("XDG_DATA_HOME") {
            let path = PathBuf::from(xdg_data_home).join(app_name);
            if fs::create_dir_all(&path).is_ok() {
                return Some(path);
            }
        }

        if let Ok(home) = env::var("HOME") {
            let path = PathBuf::from(home)
                .join(".local")
                .join("share")
                .join(app_name);

            if fs::create_dir_all(&path).is_ok() {
                return Some(path);
            }
        }

        None
    }

    #[cfg(target_os = "windows")]
    fn get_app_name_windows(app_name: &str) -> Option<PathBuf> {
        if let Ok(appdata) = env::var("APPDATA") {
            let path = PathBuf::from(appdata).join(app_name);
            if fs::create_dir_all(&path).is_ok() {
                return Some(path);
            }
        }

        if let Ok(userprofile) = env::var("USERPROFILE") {
            let path = PathBuf::from(userprofile)
                .join("AppData")
                .join("Roaming")
                .join(app_name);

            if fs::create_dir_all(&path).is_ok() {
                return Some(path);
            }
        }

        None
    }
}