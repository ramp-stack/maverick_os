#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2_foundation::{NSString, NSURL};
#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2::msg_send;
#[cfg(any(target_os = "ios", target_os = "macos"))]
use std::path::PathBuf;
#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2::runtime::AnyObject;

#[cfg(target_os = "macos")]
use objc2_foundation::{NSError, NSDictionary, NSAutoreleasePool, NSFileManager, NSSearchPathDirectory,NSSearchPathDomainMask};

#[cfg(target_os = "ios")]
const NS_APPLICATION_SUPPORT_DIRECTORY: usize = 14;
#[cfg(target_os = "ios")]
const NS_USER_DOMAIN_MASK: usize = 1;

#[derive(Clone)]
pub struct ApplicationSupport;

impl ApplicationSupport {
    #[cfg(target_os = "ios")]
    pub fn get() -> Option<PathBuf> {
        use objc2::runtime::AnyClass;
        use std::ffi::CStr;

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
            let str_path = (*path).to_string();
            Some(PathBuf::from(str_path))
        }
    }

    #[cfg(target_os = "macos")]
    pub fn get() -> Option<PathBuf> {
        unsafe {
            use objc2::class;
            use objc2::rc::Retained;
            use objc2::runtime::Bool;

            let _pool = NSAutoreleasePool::new();

            let file_manager = NSFileManager::defaultManager();

            let url: Result<Retained<NSURL>, Retained<NSError>> = file_manager.URLForDirectory_inDomain_appropriateForURL_create_error(
                NSSearchPathDirectory::ApplicationSupportDirectory,
                NSSearchPathDomainMask::UserDomainMask,
                None,
                true
            );

            println!("URL: {:?}", url);

            if let Ok(mut url) = url {
                let bundle: *mut AnyObject = msg_send![class!(NSBundle), mainBundle];
                println!("Bundle {:?}", bundle);
                let identifier: *mut NSString = msg_send![bundle, bundleIdentifier];
                println!("Retainedentfier {:?}", identifier);


                let identifier = if !identifier.is_null() {
                    Retained::retain(identifier).unwrap()
                } else {
                    println!("Running outside .app bundle — using fallback identifier");
                    NSString::from_str("org.ramp.orange")
                };

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
