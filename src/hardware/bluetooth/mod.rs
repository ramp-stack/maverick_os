// bluetooth/mod.rs - Bluetooth module for macOS/iOS
#![cfg(any(target_os = "macos", target_os = "ios"))]

pub mod central_manager;
pub mod peripheral_connection;
pub mod peripheral;

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

/// Device information containing BLE advertisement and connection data
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    /// Advertised device name
    pub name: String,
    /// Received signal strength indicator in dBm
    pub rssi: i32,
    /// Whether the device is currently connected
    pub is_connected: bool,
    /// Last time this device was discovered
    pub last_seen: SystemTime,
    /// UUIDs of advertised services
    pub services: Vec<String>,
    /// Characteristics organized by service UUID
    pub characteristics: HashMap<String, Vec<String>>,
    /// Raw manufacturer-specific data from advertisement
    pub manufacturer_data: Option<Vec<u8>>,
    /// Service data organized by service UUID
    pub service_data: HashMap<String, Vec<u8>>,
    /// TX power level from advertisement
    pub tx_power_level: Option<i32>,
    /// Whether the device accepts connections
    pub is_connectable: bool,
    /// Additional advertisement data fields
    pub advertisement_data: HashMap<String, String>,
}

/// Global thread-safe store for all discovered Bluetooth devices
pub(crate) static DISCOVERED_DEVICES: OnceLock<Mutex<HashMap<String, DeviceInfo>>> = OnceLock::new();

/// Public API for Bluetooth operations
pub mod api {
    use super::*;

    // Re-export peripheral connection functions
    pub use super::peripheral_connection::{
        get_peripheral_messages,
        get_latest_message as get_latest_peripheral_message,
        get_all_messages as get_all_peripheral_messages,
        clear_peripheral_messages
    };

    /// Initialize the Bluetooth peripheral (advertiser)
    /// 
    /// This must be called before using peripheral-related functions.
    /// On iOS, ensure you have the appropriate NSBluetoothPeripheralUsageDescription in Info.plist.
    pub fn init_peripheral() {
        peripheral::create_peripheral();
    }

    /// Begin advertising as a Bluetooth peripheral
    /// 
    /// The peripheral must be initialized with `init_peripheral()` first.
    /// The characteristic will advertise the service UUID.
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
    /// 
    /// When a central subscribes to the characteristic, this data will be sent.
    pub fn set_peripheral_data(text: &str) {
        peripheral::set_text_to_send(text);
    }

    /// Start the Bluetooth central manager
    /// 
    /// This will automatically:
    /// - Scan for devices advertising the target service UUID
    /// - Connect to discovered devices
    /// - Discover services and characteristics
    /// - Subscribe to notify/indicate characteristics
    /// - Read incoming data automatically
    /// - Handle reconnections
    /// 
    /// On iOS, ensure you have the appropriate NSBluetoothCentralUsageDescription in Info.plist.
    pub fn start_central() {
        central_manager::start_central();
    }

    /// Stop the central manager and cleanup all connections
    /// 
    /// This will:
    /// - Stop scanning for devices
    /// - Unsubscribe from all characteristics
    /// - Disconnect from all connected devices
    /// - Clean up resources
    pub fn stop_central() {
        central_manager::stop_central();
    }

    /// Get a snapshot of all discovered devices
    /// 
    /// Returns a HashMap of device identifiers to DeviceInfo.
    /// This is useful for monitoring which devices have been discovered and their status.
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
    /// 
    /// This should be called before application shutdown.
    pub fn cleanup() {
        peripheral::cleanup_peripheral();
        central_manager::stop_central();
        peripheral_connection::cleanup_all_peripherals();
    }
}