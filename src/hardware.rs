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
mod bluetooth; 
mod flash;

use std::sync::mpsc::Sender;
use std::collections::HashMap;

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
pub use bluetooth::api as bluetooth_api;
pub use bluetooth::DeviceInfo;


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

    /// Toggles the device's camera flash/torch on or off.
    /// This controls the LED flash typically found on mobile devices with cameras.
    ///
    /// ```rust
    /// // Turn flash on
    /// ctx.hardware.toggle_flash(true);
    /// 
    /// // Turn flash off
    /// ctx.hardware.toggle_flash(false);
    /// ```
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn toggle_flash(&self, on: bool) {
        flash::toggle_flash(on);
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
    /// hardware_context.cloud_save("username", "alice");
    /// ```
    pub fn cloud_save(&self, key: &str, value: &str) -> Result<(), String> {
        CloudStorage::save(key, value);
        Ok(())
    }

    /// Retrieves a value from cloud storage for the given key.
    ///
    /// ```rust
    /// let username = hardware_context.cloud_get("username").expect("No username existed");
    /// ```
    pub fn cloud_get(&self, key: &str) -> Option<String> {
        CloudStorage::get(key)
    }

    /// Removes the value associated with the given key from cloud storage.
    ///
    /// ```rust
    /// hardware_context.cloud_remove("username");
    /// ```
    pub fn cloud_remove(&self, key: &str) -> Result<(), String> {
        CloudStorage::remove(key);
        Ok(())
    }

    /// Clears all key–value pairs from cloud storage.
    ///
    /// ```rust
    /// hardware_context.cloud_clear();
    /// ```
    pub fn cloud_clear(&self) -> Result<(), String> {
        CloudStorage::clear();
        Ok(())
    }

    // ========== Bluetooth Functions ==========

    /// Start the Bluetooth central manager.
    /// 
    /// This automatically:
    /// - Scans for devices advertising the target service UUID
    /// - Connects to discovered devices
    /// - Discovers services and characteristics
    /// - Subscribes to notify/indicate characteristics
    /// - Reads incoming data automatically
    /// - Handles reconnections
    ///
    /// ```rust
    /// ctx.hardware.bluetooth_start_central();
    /// ```
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn bluetooth_start_central(&self) {
        bluetooth_api::start_central();
    }

    /// Stop the Bluetooth central manager and cleanup all connections.
    /// 
    /// This will:
    /// - Stop scanning for devices
    /// - Unsubscribe from all characteristics
    /// - Disconnect from all connected devices
    /// - Clean up resources
    ///
    /// ```rust
    /// ctx.hardware.bluetooth_stop_central();
    /// ```
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn bluetooth_stop_central(&self) {
        bluetooth_api::stop_central();
    }

    /// Get all discovered Bluetooth devices.
    /// Returns a HashMap where keys are device identifiers and values are DeviceInfo.
    /// Useful for monitoring which devices have been discovered and their connection status.
    ///
    /// ```rust
    /// let devices = ctx.hardware.bluetooth_get_devices();
    /// for (id, info) in devices {
    ///     println!("Device: {} ({})", info.name, id);
    ///     println!("  Connected: {}", info.is_connected);
    /// }
    /// ```
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn bluetooth_get_devices(&self) -> HashMap<String, DeviceInfo> {
        bluetooth_api::get_discovered_devices()
    }

    /// Get information about a specific discovered device.
    ///
    /// ```rust
    /// if let Some(device) = ctx.hardware.bluetooth_get_device("device-id-123") {
    ///     println!("Device: {}, RSSI: {}", device.name, device.rssi);
    /// }
    /// ```
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn bluetooth_get_device(&self, identifier: &str) -> Option<DeviceInfo> {
        bluetooth_api::get_device_info(identifier)
    }

    /// Initialize Bluetooth peripheral manager (advertiser).
    /// This must be called before advertising.
    ///
    /// ```rust
    /// ctx.hardware.bluetooth_init_peripheral();
    /// ```
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn bluetooth_init_peripheral(&self) {
        bluetooth_api::init_peripheral();
    }

    /// Start advertising as a Bluetooth peripheral.
    /// The peripheral manager must be initialized first.
    ///
    /// ```rust
    /// ctx.hardware.bluetooth_start_advertising()?;
    /// ```
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn bluetooth_start_advertising(&self) -> Result<(), String> {
        bluetooth_api::start_advertising()
    }

    /// Stop advertising as a Bluetooth peripheral.
    ///
    /// ```rust
    /// ctx.hardware.bluetooth_stop_advertising();
    /// ```
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn bluetooth_stop_advertising(&self) {
        bluetooth_api::stop_advertising();
    }

    /// Check if currently advertising as a peripheral.
    ///
    /// ```rust
    /// if ctx.hardware.bluetooth_is_advertising() {
    ///     println!("Currently advertising");
    /// }
    /// ```
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn bluetooth_is_advertising(&self) -> bool {
        bluetooth_api::is_advertising()
    }

    /// Set the data to be sent to centrals that connect to this peripheral.
    ///
    /// ```rust
    /// ctx.hardware.bluetooth_set_peripheral_data("Device ready for pairing");
    /// ```
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn bluetooth_set_peripheral_data(&self, text: &str) {
        bluetooth_api::set_peripheral_data(text);
    }

    /// Get all messages received from a specific peripheral.
    /// Returns None if the peripheral hasn't sent any messages.
    /// Messages persist until explicitly cleared or cleanup is called.
    ///
    /// ```rust
    /// if let Some(messages) = ctx.hardware.bluetooth_get_peripheral_messages("device-id-123") {
    ///     for msg in messages {
    ///         println!("Message: {}", msg);
    ///     }
    /// }
    /// ```
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn bluetooth_get_peripheral_messages(&self, identifier: &str) -> Option<Vec<String>> {
        bluetooth_api::get_peripheral_messages(identifier)
    }

    /// Get the most recent message from a specific peripheral.
    /// Returns None if no messages have been received.
    ///
    /// ```rust
    /// if let Some(latest) = ctx.hardware.bluetooth_get_latest_message("device-id-123") {
    ///     println!("Latest message: {}", latest);
    /// }
    /// ```
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn bluetooth_get_latest_message(&self, identifier: &str) -> Option<String> {
        bluetooth_api::get_latest_peripheral_message(identifier)
    }

    /// Get all messages from all connected peripherals.
    /// Returns a HashMap with peripheral identifiers as keys and message vectors as values.
    ///
    /// ```rust
    /// let all_messages = ctx.hardware.bluetooth_get_all_messages();
    /// for (device_id, messages) in all_messages {
    ///     println!("Device {}: {} messages", device_id, messages.len());
    /// }
    /// ```
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn bluetooth_get_all_messages(&self) -> HashMap<String, Vec<String>> {
        bluetooth_api::get_all_peripheral_messages()
    }

    /// Clear all stored messages for a specific peripheral.
    ///
    /// ```rust
    /// ctx.hardware.bluetooth_clear_peripheral_messages("device-id-123");
    /// ```
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn bluetooth_clear_peripheral_messages(&self, identifier: &str) {
        bluetooth_api::clear_peripheral_messages(identifier)
    }

    /// Clean up all Bluetooth resources.
    /// Should be called before application shutdown.
    ///
    /// ```rust
    /// ctx.hardware.bluetooth_cleanup();
    /// ```
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn bluetooth_cleanup(&self) {
        bluetooth_api::cleanup();
    }
}