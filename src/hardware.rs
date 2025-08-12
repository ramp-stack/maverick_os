mod logger;
mod cache;
mod camera;
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
pub use camera::ImageSettings;
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
        // Return the Result instead of panicking for better error handling
        Camera::new()
    }

    pub fn open_camera(&self) -> Result<Camera, CameraError> {
        // Return the Result instead of panicking for better error handling
        self.create_camera()
    }

    // If you need the old behavior that panics on failure, use these methods:
    pub fn create_camera_or_panic(&self) -> Camera {
        // Explicitly use the standard Apple camera (not custom)
        Camera::new().expect("Failed to create camera")
    }

    pub fn open_camera_or_panic(&self) -> Camera {
        // Explicitly use the standard Apple camera (not custom)
        self.create_camera_or_panic()
    }

    // Method to create custom camera (for raw Bayer data)
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn create_custom_camera(&self) -> Result<Camera, CameraError> {
        Camera::new_custom()
    }

    pub fn paste(&self) -> String {
        Clipboard::get()
    }

    pub fn copy(&self, text: String) {
        Clipboard::set(text);
    }

    pub fn share(&self, text: &str) {
        Share::share(text);
    }

    pub fn share_image(&self, image: image::RgbaImage) {
        Share::share_image(image);
    }

    pub fn open_photo_picker(&self, sender: Sender<(Vec<u8>, ImageOrientation)>) {
        PhotoPicker::open(sender);
    }

    pub fn cloud_save(&self, key: &str, value: &str) {
        CloudStorage::save(key, value);
    }

    pub fn cloud_get(&self, key: &str) -> Option<String> {
        CloudStorage::get(key).ok().flatten()
    }

    pub fn cloud_remove(&self, key: &str) {
        CloudStorage::remove(key);
    }

    pub fn cloud_clear(&self, key: &str) {
        CloudStorage::clear();
    }
}