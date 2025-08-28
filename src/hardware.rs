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

/// `HardwareContext` contains interfaces to various hardware.
#[derive(Clone)]
pub struct Context {
    pub cache: Cache
}

impl Context {
    pub(crate) fn new() -> Self {
        Clipboard::new();

        logger::Logger::start(None);
        Self {
            cache: Cache::new(),
        }
    }

    /// Registers notifications so they can be queued for delivery.
    ///
    /// ```rust
    #[doc = include_str!("examples/register_notifs.rs")]
    /// ```
    pub fn register_notifs(&self) {
        Notifications::register();
    }

    /// Queues a new push notification to be sent to the device.
    /// Notifications will only be sent while the app is backgrounded.
    ///
    /// ```rust
    #[doc = include_str!("examples/push_notifs.rs")]
    /// ```
    pub fn push_notification(&self, title: &str, msg: &str) {
        Notifications::push(title, msg);
    }

    // / Queues a new silent notification to be sent to the device.
    // / Notifications will only be sent while the app is backgrounded.
    // / This type of notification will not be seen by the user.
    // pub fn silent_notification(&self, msg: &str) {
    //     Notifications::silent(msg);
    // }

    /// Trigger vibration haptics on the device.
    ///
    /// ```rust
    #[doc = include_str!("examples/haptic.rs")]
    /// ```
    pub fn haptic(&self) {
        Haptics::vibrate()
    }

    /// Retrieves the safe area insets as `(top, right, bottom, left)`.
    /// These values can be used to adjust UI layout to avoid screen cutouts or system UI elements.
    ///
    /// ```rust
    #[doc = include_str!("examples/safe_area.rs")]
    /// ```
    pub fn safe_area_insets(&self) -> (f32, f32, f32, f32) {
        SafeAreaInsets::get()
    }

    /// Opens the device camera.
    /// Uses the back-facing camera on mobile devices and the default camera on desktop.
    ///
    /// ```rust
    #[doc = include_str!("examples/open_camera.rs")]
    /// ```
    pub fn open_camera(&self) -> Result<Camera, CameraError> {
        Camera::new()
    }

    /// Opens the device camera without AI processing on iOS and macOS.
    /// On all other platforms, opens the default camera.
    ///
    /// ```rust
    #[doc = include_str!("examples/open_unprocessed.rs")]
    /// ```
    pub fn open_unprocessed_camera(&self) -> Result<Camera, CameraError> {
        Camera::new_unprocessed()
    }

    /// Returns the contents of the device's clipboard.
    ///
    /// ```rust
    #[doc = include_str!("examples/paste.rs")]
    /// ```
    pub fn paste(&self) -> String {
        Clipboard::get()
    }

    /// Sets the contents of the device's clipboard to the provided `String`.
    ///
    /// ```rust
    #[doc = include_str!("examples/copy.rs")]
    /// ```
    pub fn copy(&self, text: String) {
        Clipboard::set(text);
    }

    /// Opens the system share dialog, allowing the provided string to be shared. 
    ///
    /// ```rust
    #[doc = include_str!("examples/share.rs")]
    /// ```
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn share(&self, text: &str) {
        Share::share(text);
    }

    /// Opens the system share dialog, allowing the provided image to be shared.
    ///
    /// ```rust
    #[doc = include_str!("examples/share_image.rs")]
    /// ```
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn share_image(&self, image: image::RgbaImage) {
        Share::share_image(image);
    }

    /// Opens the system photo picker dialog.
    ///
    /// ```rust
    #[doc = include_str!("examples/open_photo_picker.rs")]
    /// ```
    pub fn open_photo_picker(&self, sender: Sender<(Vec<u8>, ImageOrientation)>) {
        PhotoPicker::open(sender);
    }

    /// Save the key-value pair to cloud storage.
    ///
    /// ```rust
    #[doc = include_str!("examples/cloud_save.rs")]
    /// ```
    pub fn cloud_save(&self, key: &str, value: &str) -> Result<(), String> {
        CloudStorage::save(key, value)
    }

    /// Retrieves a value from cloud storage for the given key.
    ///
    /// ```rust
    #[doc = include_str!("examples/cloud_get.rs")]
    /// ```
    pub fn cloud_get(&self, key: &str) -> Option<String> {
        CloudStorage::get(key).ok().flatten()
    }

    /// Removes the value associated with the given key from cloud storage.
    ///
    /// ```rust
    #[doc = include_str!("examples/cloud_remove.rs")]
    /// ```
    pub fn cloud_remove(&self, key: &str) -> Result<(), String> {
        CloudStorage::remove(key)
    }

    /// Clears all key–value pairs from cloud storage.
    ///
    /// ```rust
    #[doc = include_str!("examples/cloud_clear.rs")]
    /// ```
    pub fn cloud_clear(&self) -> Result<(), String> {
        CloudStorage::clear()
    }
}