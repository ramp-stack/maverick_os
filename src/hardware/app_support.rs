//! Platform-specific application support directory access.
//!
//! This module provides cross-platform functionality to access application support
//! directories on iOS, macOS, Linux, Windows, and Android.

#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2_foundation::{NSString, NSURL};
#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2::msg_send;

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

#[cfg(target_os = "android")]
use jni::{
    JNIEnv,
    objects::{JObject, JString},
    JavaVM,
};
#[cfg(target_os = "android")]
use std::sync::OnceLock;

#[cfg(target_os = "ios")]
const NS_APPLICATION_SUPPORT_DIRECTORY: usize = 14;
#[cfg(target_os = "ios")]
const NS_USER_DOMAIN_MASK: usize = 1;

#[cfg(target_os = "android")]
static JAVA_VM: OnceLock<JavaVM> = OnceLock::new();

/// Access the application support directory.
///
/// Provides platform-specific methods to retrieve application support directories
/// where applications can store user data, preferences, and other files.
#[derive(Clone)]
pub struct ApplicationSupport;

impl ApplicationSupport {
    #[cfg(target_os = "android")]
    pub fn init_android(vm: &JavaVM) {
        if let Ok(env) = vm.attach_current_thread() {
            if let Ok(new_vm) = env.get_java_vm() {
                JAVA_VM.set(new_vm).expect("JavaVM already initialized");
            }
        }
    }

    /// Get the general application support directory path.
    ///
    /// Returns the base application support directory for the current platform:
    /// - **iOS**: Uses `NSFileManager` to get the Application Support directory
    /// - **macOS**: Returns current directory (fallback implementation)
    /// - **Linux**: Uses `XDG_DATA_HOME` or `~/.local/share/org.ramp.orange`
    /// - **Windows**: Uses `%APPDATA%\org.ramp.orange`
    /// - **Android**: Uses app's internal files directory via JNI
    ///
    /// # Returns
    ///
    /// `Some(PathBuf)` with the application support directory path, or `None` if unavailable.
    ///
    /// # Examples
    ///
    /// ```
    /// use application_support::ApplicationSupport;
    ///
    /// if let Some(path) = ApplicationSupport::get() {
    ///     println!("App support directory: {:?}", path);
    /// }
    /// ```
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
        #[cfg(target_os = "android")]
        {
            Self::get_android()
        }
        #[cfg(not(any(target_os = "ios", target_os = "macos", target_os = "linux", target_os = "windows", target_os = "android")))]
        {
            // Fallback for unsupported platforms
            Some(PathBuf::from("./app_data"))
        }
    }

    /// Get application support directory for a specific app name.
    ///
    /// Returns a subdirectory within the application support directory
    /// specifically for the given application name.
    ///
    /// # Arguments
    ///
    /// * `app_name` - The name/identifier of the application
    ///
    /// # Returns
    ///
    /// `Some(PathBuf)` with the app-specific directory path, or `None` if unavailable.
    ///
    /// # Examples
    ///
    /// ```
    /// use application_support::ApplicationSupport;
    ///
    /// if let Some(path) = ApplicationSupport::get_app_name("MyApp") {
    ///     println!("MyApp data directory: {:?}", path);
    /// }
    /// ```
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
        #[cfg(target_os = "android")]
        {
            Self::get_app_name_android(app_name)
        }
        #[cfg(not(any(target_os = "ios", target_os = "macos", target_os = "linux", target_os = "windows", target_os = "android")))]
        {
            // Fallback for unsupported platforms
            let path = PathBuf::from("./app_data").join(app_name);
            std::fs::create_dir_all(&path).ok()?;
            Some(path)
        }
    }

    /// Get the application support directory on iOS.
    ///
    /// Uses Objective-C runtime to access `NSFileManager` and retrieve the
    /// Application Support directory for the user domain.
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

    /// Get the application support directory on macOS.
    ///
    /// Currently returns the current directory as a fallback.
    /// TODO: Implement proper macOS Application Support directory access.
    #[cfg(target_os = "macos")]
    fn get_macos() -> Option<PathBuf> {
        // For now, using the simple fallback since the original implementation was commented out
        // You can uncomment and fix the original implementation if needed
        Some(PathBuf::from("./"))
    }

    /// Get the application support directory on Linux.
    ///
    /// Follows XDG Base Directory specification, checking:
    /// 1. `$XDG_DATA_HOME/org.ramp.orange`
    /// 2. `$HOME/.local/share/org.ramp.orange`
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

    /// Get the application support directory on Windows.
    ///
    /// Checks the following locations in order:
    /// 1. `%APPDATA%\org.ramp.orange`
    /// 2. `%USERPROFILE%\AppData\Roaming\org.ramp.orange`
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

    /// Get the application support directory on Android using JNI.
    ///
    /// Returns the internal app data directory by calling Android Context.getFilesDir()
    /// through JNI. This requires the JavaVM to be initialized via `init_android()`.
    ///
    /// The path returned is typically `/data/data/<package_name>/files`
    #[cfg(target_os = "android")]
    fn get_android() -> Option<PathBuf> {
        let vm = JAVA_VM.get()?;
        let mut env = vm.attach_current_thread().ok()?;
        
        Self::get_android_files_dir(&mut env, "org.ramp.orange")
    }

    /// Helper function to get Android files directory via JNI
    #[cfg(target_os = "android")]
    fn get_android_files_dir(env: &mut JNIEnv, subdir: &str) -> Option<PathBuf> {
        // Get the application context
        // This assumes you have a way to get the context - typically stored globally
        // or passed through JNI. You may need to adjust this based on your app structure.
        
        // Try to get the activity class
        let activity_thread_class = env.find_class("android/app/ActivityThread").ok()?;
        
        // Get the current activity thread
        let current_activity_thread = env
            .call_static_method(
                activity_thread_class,
                "currentActivityThread",
                "()Landroid/app/ActivityThread;",
                &[],
            )
            .ok()?
            .l()
            .ok()?;
        
        // Get the application context
        let context = env
            .call_method(
                current_activity_thread,
                "getApplication",
                "()Landroid/app/Application;",
                &[],
            )
            .ok()?
            .l()
            .ok()?;
        
        // Call getFilesDir() on the context
        let files_dir = env
            .call_method(&context, "getFilesDir", "()Ljava/io/File;", &[])
            .ok()?
            .l()
            .ok()?;
        
        // Get the absolute path
        let path_jstring = env
            .call_method(&files_dir, "getAbsolutePath", "()Ljava/lang/String;", &[])
            .ok()?
            .l()
            .ok()?;
        
        // Convert JString to Rust String
        let path_string: String = env
            .get_string(&JString::from(path_jstring))
            .ok()?
            .into();
        
        let mut path = PathBuf::from(path_string);
        
        // Append subdirectory if specified
        if !subdir.is_empty() {
            path.push(subdir);
            std::fs::create_dir_all(&path).ok()?;
        }
        
        Some(path)
    }

    /// Get app-specific support directory on macOS.
    ///
    /// Creates a subdirectory within the macOS Application Support directory
    /// for the specified application name.
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

    /// Get app-specific support directory on Linux.
    ///
    /// Creates a subdirectory within the XDG data directory
    /// for the specified application name.
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

    /// Get app-specific support directory on Windows.
    ///
    /// Creates a subdirectory within the Windows AppData directory
    /// for the specified application name.
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


    #[cfg(target_os = "android")]
    fn get_app_name_android(app_name: &str) -> Option<PathBuf> {
        let vm = JAVA_VM.get().or_else(|| {
            eprintln!("ERROR: Android JavaVM not initialized! Call ApplicationSupport::init_android() first or ensure JNI_OnLoad is properly set up.");
            None
        })?;
        
        let mut env = vm.attach_current_thread().ok()?;
        Self::get_android_files_dir(&mut env, app_name)
    }
}