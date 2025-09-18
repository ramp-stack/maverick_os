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

pub use cache::{Cache, ActiveCache};
pub use clipboard::Clipboard;
pub use camera::{Camera, CameraSettings, CameraError, ExposureMode, FocusMode, WhiteBalanceMode};
pub use share::Share;
pub use app_support::ApplicationSupport;
pub use cloud::CloudStorage;
pub use photo_picker::{PhotoPicker, ImageOrientation};
pub use safe_area::SafeAreaInsets;
pub use haptics::Haptics;
pub use notifications::Notifications;
pub use logger::Logger;

/// `HardwareContext` contains interfaces to various hardware.
#[derive(Clone)]
pub struct Context {
    pub cache: Cache
}

impl Context {
    pub(crate) fn new() -> Self {
        Logger::start(None);
        Clipboard::new();

        Self {
            cache: Cache::new(),
        }
    }

    /// Registers notifications so they can be queued for delivery.
    ///
    /// ```rust
    /// hardware_context.register_notifs();
    /// ```
    pub fn register_notifs(&self) {
        Notifications::register();
    }

    /// Queues a new push notification to be sent to the device.
    /// Notifications will only be sent while the app is backgrounded.
    ///
    /// ```rust
    /// ctx.hardware.push_notification("Reminder", "Don't forget your meeting at 3 PM today.");
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
    /// ctx.hardware.haptic()
    /// ```
    pub fn haptic(&self) {
        Haptics::vibrate()
    }

    /// Retrieves the safe area insets as `(top, right, bottom, left)`.
    /// These values can be used to adjust UI layout to avoid screen cutouts or system UI elements.
    ///
    /// ```rust
    /// let safe_areas = ctx.hardware.safe_area_insets();
    /// ```
    pub fn safe_area_insets(&self) -> (f32, f32, f32, f32) {
        SafeAreaInsets::get()
    }

    /// Opens the device camera.
    /// Uses the back-facing camera on mobile devices and the default camera on desktop.
    ///
    /// ```rust
    /// let camera = ctx.hardware.open_camera().map(|cam| cam.start());
    /// ```
    pub fn open_camera(&self) -> Result<Camera, CameraError> {
        Camera::new()
    }

    /// Opens the device camera without AI processing on iOS and macOS.
    /// On all other platforms, opens the default camera.
    ///
    /// ```rust
    /// let camera = ctx.hardware.open_unprocessed_camera().map(|cam| cam.start());
    /// ```
    pub fn open_unprocessed_camera(&self) -> Result<Camera, CameraError> {
        Camera::new_unprocessed()
    }

    /// Returns the contents of the device's clipboard.
    ///
    /// ```rust
    /// let clipboard = ctx.hardware.paste();
    /// ```
    pub fn paste(&self) -> String {
        Clipboard::get()
    }

    /// Sets the contents of the device's clipboard to the provided `String`.
    ///
    /// ```rust
    /// ctx.hardware.copy("WiFiPassword123");
    /// ```
    pub fn copy(&self, text: String) {
        Clipboard::set(text);
    }

    /// Opens the system share dialog, allowing the provided string to be shared. 
    ///
    /// ```rust
    /// ctx.hardware.share("WiFiPassword123");
    /// ```
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn share(&self, text: &str) {
        Share::share(text);
    }

    /// Opens the system share dialog, allowing the provided image to be shared.
    ///
    /// ```rust
    /// let my_image: RgbaImage = ImageReader::open("dog.png")
    ///     .expect("Failed to open image")
    ///     .decode()
    ///     .expect("Failed to decode image")
    ///     .into_rgba8();
    ///
    /// ctx.hardware.share_image(my_image);
    /// ```
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn share_image(&self, image: image::RgbaImage) {
        Share::share_image(image);
    }

    /// Opens the system photo picker dialog.
    ///
    /// ```rust
    /// let (tx, rx) = channel::<(Vec<u8>, ImageOrientation)>();
    /// ctx.hardware.open_photo_picker(tx);
    /// ```
    pub fn open_photo_picker(&self, sender: Sender<(Vec<u8>, ImageOrientation)>) {
        PhotoPicker::open(sender);
    }

    /// Save the key-value pair to cloud storage.
    ///
    /// ```rust
    /// hardware_context.save("username", "alice");
    /// ```
    pub fn cloud_save(&self, key: &str, value: &str) -> Result<(), String> {
        CloudStorage::save(key, value);
        Ok(())
    }

    /// Retrieves a value from cloud storage for the given key.
    ///
    /// ```rust
    /// let username = hardware_context.get("username").expect("No username existed");
    /// ```
    pub fn cloud_get(&self, key: &str) -> Option<String> {
        CloudStorage::get(key)
    }

    /// Removes the value associated with the given key from cloud storage.
    ///
    /// ```rust
    /// hardware_context.remove("username");
    /// ```
    pub fn cloud_remove(&self, key: &str) -> Result<(), String> {
        CloudStorage::remove(key);
        Ok(())
    }

    /// Clears all key–value pairs from cloud storage.
    ///
    /// ```rust
    /// hardware_context.clear();
    /// ```
    pub fn cloud_clear(&self) -> Result<(), String> {
        CloudStorage::clear();
        Ok(())
    }
}
