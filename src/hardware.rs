mod logger;
mod cache;
pub mod camera;
mod share;
mod clipboard;
mod app_support;
mod cloud;
mod haptics;
mod photo_picker;
mod safe_area;
mod notifications;

use std::sync::mpsc::Sender;

pub use cache::Cache;
pub use clipboard::Clipboard;
pub use camera::{Camera, CameraError};
pub use share::Share;
pub use app_support::ApplicationSupport;
pub use cloud::CloudStorage;
pub use photo_picker::{PhotoPicker, ImageOrientation};
pub use safe_area::SafeAreaInsets;
pub use haptics::Haptics;
pub use notifications::Notifications;

/// Hardware context contains interfaces to various hardware.
/// All interfaces should be clonable or internally synchronized and safe to call from multiple places.
#[derive(Clone)]
pub struct Context {
    pub cache: Cache,
    pub clipboard: Clipboard,
    pub share: Share,
    pub app_support: ApplicationSupport,
    pub cloud: CloudStorage,
    pub photo_picker: PhotoPicker,
}

impl Context {
    #[cfg(target_os = "android")]
    pub(crate) fn new() -> Self {
        logger::Logger::start(None);
        Self {
            cache: Cache::new(),
            clipboard: Clipboard::new().expect("Clipboard must be initialized before Context::new()"),
            share: Share::new(),
            app_support: ApplicationSupport,
            cloud: CloudStorage::default(),
            photo_picker: PhotoPicker,
        }
    }

    #[cfg(not(target_os = "android"))]
    pub(crate) fn new() -> Self {
        logger::Logger::start(None);
        Self {
            cache: Cache::new(),
            clipboard: Clipboard::new(),
            share: Share::new(),
            app_support: ApplicationSupport,
            cloud: CloudStorage,
            photo_picker: PhotoPicker,
        }
    }

    // pub fn silent_notification(&self, msg: &str) {
    //     Notifications::silent(msg);
    // }

    pub fn register_notifs(&self) {
        Notifications::register();
    }

    pub fn push_notification(&self, title: &str, msg: &str) {
        Notifications::push(title, msg);
    }

    pub fn haptic(&self) {
        Haptics::vibrate()
    }

    pub fn safe_area_insets(&self) -> (f32, f32, f32, f32) {
        SafeAreaInsets::get()
    }

    pub fn create_camera(&self) -> Result<Camera, CameraError> {
        Ok(Camera::new())
    }

    pub fn open_camera(&self) -> Result<Camera, CameraError> {
        self.create_camera()
    }

    pub fn paste(&self) -> String {
        Clipboard::get()
    }

    pub fn copy(&self, text: String) {
        Clipboard::set(text);
    }

    pub fn share(&self, text: &str) {
        #[cfg(not(target_os = "android"))]
        {
            Share::share(text);
        }

        #[cfg(target_os = "android")]
        {
            self.share.share(text);
        }
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
}
