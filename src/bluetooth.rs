// bluetooth.rs - Parent module file
#![cfg(any(target_os = "macos", target_os = "ios"))]

mod central_manager;
mod peripheral_connection;
mod peripheral;

// Common device info struct
use std::collections::HashMap;
use std::time::SystemTime;

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub name: String,
    pub rssi: i32,
    pub is_connected: bool,
    pub last_seen: SystemTime,
    pub services: Vec<String>,
    pub characteristics: HashMap<String, Vec<String>>,
    pub manufacturer_data: Option<Vec<u8>>,
    pub service_data: HashMap<String, Vec<u8>>,
    pub tx_power_level: Option<i32>,
    pub is_connectable: bool,
    pub advertisement_data: HashMap<String, String>,
}

// Thread-safe global store for discovered devices
use std::sync::{Mutex, OnceLock};
pub(crate) static DISCOVERED_DEVICES: OnceLock<Mutex<HashMap<String, DeviceInfo>>> = OnceLock::new();

// Public API
pub mod api {
    use super::*;

    /// Initialize the Bluetooth peripheral (advertiser)
    pub fn init_peripheral() {
        peripheral::create_peripheral();
    }

    /// Initialize the Bluetooth central (scanner/connector)
    pub fn init_central() {
        central_manager::create_central();
    }

    /// Begin advertising as a Bluetooth peripheral
    pub fn start_advertising() -> Result<(), String> {
        peripheral::start_advertising()
    }

    /// Stop advertising as a Bluetooth peripheral
    pub fn stop_advertising() {
        peripheral::stop_advertising();
    }

    /// Check if currently advertising
    pub fn is_advertising() -> bool {
        peripheral::is_advertising()
    }

    /// Set text data to be sent to connecting centrals
    pub fn set_peripheral_data(text: &str) {
        peripheral::set_text_to_send(text);
    }

    /// Start scanning for Bluetooth devices
    pub fn start_scanning() {
        central_manager::start_central_scan();
    }

    /// Stop scanning for Bluetooth devices
    pub fn stop_scanning() {
        central_manager::stop_central_scan();
    }

    /// Connect to a specific device by identifier
    pub fn connect(identifier: &str) -> Result<(), String> {
        central_manager::connect_to_device(identifier)
    }

    /// Disconnect from a peripheral device
    pub fn disconnect(identifier: &str) -> Result<(), String> {
        peripheral_connection::disconnect_from_device(identifier)
    }

    /// Send data to a connected device's characteristic
    pub fn send_data(
        identifier: &str,
        service_uuid: &str,
        characteristic_uuid: &str,
        data: &str,
    ) -> Result<(), String> {
        peripheral_connection::send_data_to_device(identifier, service_uuid, characteristic_uuid, data)
    }

    /// Convenience function to send "Hello World" to a device
    pub fn send_hello_world(identifier: &str, service_uuid: &str, characteristic_uuid: &str) -> Result<(), String> {
        peripheral_connection::send_hello_world(identifier, service_uuid, characteristic_uuid)
    }

    /// Get a snapshot of all discovered devices
    pub fn get_discovered_devices() -> HashMap<String, DeviceInfo> {
        DISCOVERED_DEVICES
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .ok()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    /// Get information about a specific discovered device
    pub fn get_device_info(identifier: &str) -> Option<DeviceInfo> {
        DISCOVERED_DEVICES
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .ok()
            .and_then(|guard| guard.get(identifier).cloned())
    }

    /// Clean up all Bluetooth resources
    pub fn cleanup() {
        peripheral::cleanup_peripheral();
        central_manager::cleanup_central();
        peripheral_connection::cleanup_all_peripherals();
    }
}