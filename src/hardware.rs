mod cache;
mod camera;
mod share;
mod clipboard;
mod app_support;
mod cloud;
mod haptics;
mod photo_picker;
mod safe_area;

#[cfg(target_os = "android")]
use jni::{JNIEnv, objects::JObject};

use std::sync::mpsc::Sender;

pub use cache::Cache;
pub use clipboard::Clipboard;
pub use camera::{Camera, CameraError};
pub use share::Share;
pub use app_support::ApplicationSupport;
pub use cloud::CloudStorage;
pub use photo_picker::{PhotoPicker, ImageOrientation};

/// Hardware context contains interfaces to various hardware.
/// All interfaces should be clonable or internally synchronized and safe to call from multiple places.
#[derive(Clone)]
pub struct Context {
    pub cache: Cache,
    #[cfg(target_os = "android")]
    pub clipboard: Clipboard,
    #[cfg(not(target_os = "android"))]
    pub clipboard: Clipboard,
    pub share: Share,
    pub app_support: ApplicationSupport,
    pub cloud: CloudStorage,
    pub photo_picker: PhotoPicker,
}

impl Context {
    #[cfg(target_os = "android")]
    pub(crate) fn new(env: &mut JNIEnv, context: JObject) -> Result<Self, jni::errors::Error> {
        Ok(Self {
            cache: Cache::new(),
            clipboard: Clipboard::new(env, context)?,
            share: Share::new(),
            app_support: ApplicationSupport,
            cloud: CloudStorage::default(),
            photo_picker: PhotoPicker,
        })
    }

    /// Creates a new hardware context for non-Android platforms
    #[cfg(not(target_os = "android"))]
    pub(crate) fn new() -> Self {
        Self {
            cache: Cache::new(),
            clipboard: Clipboard::new(),
            share: Share::new(),
            app_support: ApplicationSupport,
            cloud: CloudStorage::default(),
            photo_picker: PhotoPicker,
        }
    }



    pub fn create_camera(&self) -> Camera {
        Camera::new()
    }

    pub fn paste(&self) -> String {
        Clipboard::get()
    }

    pub fn copy(&self, text: String) {
        Clipboard::set(text);
    }

    pub fn share(&self, text: &str) {
        #[cfg(target_os = "ios")]
        {
            Share::share(text);
        }

        #[cfg(target_os = "android")]
        {
            self.share.share(text);
        }

        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        {
            // Explicitly use the parameter to avoid unused variable warning
            let _ = text;
            // Could log or handle unsupported platform here
        }
    }

    pub fn get_app_support_path(&self) -> Option<std::path::PathBuf> {
        ApplicationSupport::get()
    }

    pub fn open_photo_picker(&self, sender: Sender<(Vec<u8>, ImageOrientation)>) {
        PhotoPicker::open(sender);
    }

    pub fn cloud_save(&self, key: &str, value: &str) -> Result<(), Box<dyn std::error::Error>> {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            CloudStorage::save(key, value).map_err(|e| e.into())
        }

        #[cfg(target_os = "android")]
        {
            CloudStorage::save(key, value).map_err(|e| format!("{:?}", e).into())
        }

        #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
        {
            Err("CloudStorage not supported on this platform".into())
        }
    }

    pub fn cloud_get(&self, key: &str) -> Result<Option<String>, Box<dyn std::error::Error>> {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            CloudStorage::get(key).map_err(|e| e.into())
        }

        #[cfg(target_os = "android")]
        {
            CloudStorage::get(key).map_err(|e| format!("{:?}", e).into())
        }

        #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
        {
            Err("CloudStorage not supported on this platform".into())
        }
    }

    pub fn cloud_remove(&self, key: &str) -> Result<(), Box<dyn std::error::Error>> {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            CloudStorage::remove(key).map_err(|e| e.into())
        }

        #[cfg(target_os = "android")]
        {
            CloudStorage::remove(key).map_err(|e| format!("{:?}", e).into())
        }

        #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
        {
            Err("CloudStorage not supported on this platform".into())
        }
    }

    pub fn cloud_clear(&self) -> Result<(), Box<dyn std::error::Error>> {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            CloudStorage::clear().map_err(|e| e.into())
        }

        #[cfg(target_os = "android")]
        {
            CloudStorage::clear().map_err(|e| format!("{:?}", e).into())
        }

        #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
        {
            Err("CloudStorage not supported on this platform".into())
        }
    }

    #[cfg(target_os = "android")]
    pub fn initialize(env: &mut JNIEnv, context: JObject) -> Result<(), jni::errors::Error> {
        Clipboard::initialize(env, context)?;

        Share::initialize().map_err(|e| {
            jni::errors::Error::JavaException // Convert the error appropriately
        })?;

        if let Ok(vm) = unsafe { jni::JavaVM::from_raw(env.get_java_vm()?.get_java_vm_pointer()) } {
            if let Err(e) = CloudStorage::init_java_vm(vm) {
                eprintln!("Warning: Failed to initialize CloudStorage JavaVM: {}", e);
                // Don't fail the entire initialization if cloud storage fails
            }
        }

        Ok(())
    }
}