// bluetooth/mod.rs

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub mod central;

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub mod peripheral;

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