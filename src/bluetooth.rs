// bluetooth.rs - Parent module file

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod central;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod peripheral;

// Re-export public functions and types
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub use central::{
    create_central, 
    start_central_scan, 
    stop_central_scan,
    connect_to_device,
    disconnect_from_device,
    send_data_to_device,
    send_hello_world,
    cleanup_central,
};

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub use peripheral::{
    create_peripheral,
    start_advertising,
    stop_advertising,
    cleanup_peripheral,
};

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