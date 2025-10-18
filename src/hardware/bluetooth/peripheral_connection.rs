// bluetooth/peripheral_connection.rs - Peripheral Connection and Data Transfer
#![cfg(any(target_os = "macos", target_os = "ios"))]

use std::sync::{Mutex, OnceLock};
use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use objc2::rc::Retained;
use objc2::runtime::{NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send};
use objc2_foundation::{NSObject, NSString};
use objc2_core_bluetooth::*;
use objc2::Message;
use objc2::AnyThread;

use crate::hardware::bluetooth::DISCOVERED_DEVICES;

// Target service UUID - must match central_manager.rs
const TARGET_SERVICE_UUID: &str = "E20A39F4-73F5-4BC4-A12F-17D1AD07A961";

// Connection and write iteration tracking
pub(crate) struct IterationState {
    pub connection_iterations_complete: usize,
    pub write_iterations_complete: usize,
    pub default_iterations: usize,
}

pub(crate) static ITERATION_STATE: OnceLock<Mutex<HashMap<String, IterationState>>> = OnceLock::new();

// Data buffer for incoming data
pub(crate) static DATA_BUFFER: OnceLock<Mutex<HashMap<String, Vec<u8>>>> = OnceLock::new();

// Storage for complete messages received from peripherals
pub(crate) static RECEIVED_MESSAGES: OnceLock<Mutex<HashMap<String, Vec<String>>>> = OnceLock::new();

pub(crate) struct PeripheralHandle {
    pub peripheral: Retained<CBPeripheral>,
    pub delegate: Retained<MyPeripheralConnectionDelegate>,
    pub transfer_characteristic: Option<Retained<CBCharacteristic>>,
}

unsafe impl Send for PeripheralHandle {}
unsafe impl Sync for PeripheralHandle {}

pub(crate) static CONNECTED_PERIPHERALS: OnceLock<Mutex<HashMap<String, PeripheralHandle>>> = OnceLock::new();

/// Retrieves all messages received from a specific peripheral
/// Returns None if the peripheral hasn't sent any messages
pub fn get_peripheral_messages(identifier: &str) -> Option<Vec<String>> {
    let messages = RECEIVED_MESSAGES.get_or_init(|| Mutex::new(HashMap::new()));
    
    if let Ok(guard) = messages.lock() {
        guard.get(identifier).cloned()
    } else {
        None
    }
}

/// Retrieves the most recent message from a specific peripheral
/// Returns None if no messages have been received
pub fn get_latest_message(identifier: &str) -> Option<String> {
    let messages = RECEIVED_MESSAGES.get_or_init(|| Mutex::new(HashMap::new()));
    
    if let Ok(guard) = messages.lock() {
        guard.get(identifier).and_then(|msgs| msgs.last().cloned())
    } else {
        None
    }
}

/// Retrieves all messages from all connected peripherals
/// Returns a HashMap with peripheral identifiers as keys and message vectors as values
pub fn get_all_messages() -> HashMap<String, Vec<String>> {
    let messages = RECEIVED_MESSAGES.get_or_init(|| Mutex::new(HashMap::new()));
    
    if let Ok(guard) = messages.lock() {
        guard.clone()
    } else {
        HashMap::new()
    }
}

/// Clears messages for a specific peripheral
pub fn clear_peripheral_messages(identifier: &str) {
    let messages = RECEIVED_MESSAGES.get_or_init(|| Mutex::new(HashMap::new()));
    
    if let Ok(mut guard) = messages.lock() {
        guard.remove(identifier);
    }
}

fn print_gatt_structure(identifier: &str, peripheral: &CBPeripheral) {
    unsafe {
        let name = peripheral.name().map(|n| n.to_string()).unwrap_or("Unknown".to_string());
        println!("\n=== GATT Structure ===");
        println!("Device: {} | ID: {}", name, identifier);
        
        if let Some(services) = peripheral.services() {
            for i in 0..services.count() {
                let service_obj = services.objectAtIndex(i);
                if let Ok(service) = service_obj.downcast::<CBService>() {
                    let service_uuid = service.UUID().UUIDString().to_string();
                    println!("Service: {}", service_uuid);
                    
                    if let Some(characteristics) = service.characteristics() {
                        for j in 0..characteristics.count() {
                            let char_obj = characteristics.objectAtIndex(j);
                            if let Ok(characteristic) = char_obj.downcast::<CBCharacteristic>() {
                                let char_uuid = characteristic.UUID().UUIDString().to_string();
                                let props = characteristic.properties();
                                
                                let mut props_str = Vec::new();
                                if props.contains(CBCharacteristicProperties::Read) { props_str.push("READ"); }
                                if props.contains(CBCharacteristicProperties::Write) { props_str.push("WRITE"); }
                                if props.contains(CBCharacteristicProperties::WriteWithoutResponse) { props_str.push("WRITE_NO_RESP"); }
                                if props.contains(CBCharacteristicProperties::Notify) { props_str.push("NOTIFY"); }
                                if props.contains(CBCharacteristicProperties::Indicate) { props_str.push("INDICATE"); }
                                
                                println!("  Characteristic: {} [{}]", char_uuid, props_str.join(", "));
                            }
                        }
                    }
                }
            }
        }
    }
}

fn write_data_to_peripheral(identifier: &str, test_data: &[u8]) {
    let peripherals = match CONNECTED_PERIPHERALS.get() {
        Some(p) => p,
        None => {
            eprintln!("CONNECTED_PERIPHERALS not initialized");
            return;
        }
    };
    
    let handle = {
        let guard = match peripherals.lock() {
            Ok(g) => g,
            Err(e) => {
                eprintln!("Failed to lock peripherals: {:?}", e);
                return;
            }
        };
        
        match guard.get(identifier) {
            Some(h) => PeripheralHandle {
                peripheral: h.peripheral.retain(),
                delegate: h.delegate.retain(),
                transfer_characteristic: h.transfer_characteristic.as_ref().map(|c| c.retain()),
            },
            None => {
                eprintln!("Peripheral {} not found", identifier);
                return;
            }
        }
    };
    
    let transfer_char = match handle.transfer_characteristic {
        Some(c) => c,
        None => {
            println!("No transfer characteristic available");
            return;
        }
    };
    
    let iterations = ITERATION_STATE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut iter_guard = match iterations.lock() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("Failed to lock iteration state: {:?}", e);
            return;
        }
    };
    
    let state = iter_guard.entry(identifier.to_string()).or_insert(IterationState {
        connection_iterations_complete: 0,
        write_iterations_complete: 0,
        default_iterations: 5,
    });
    
    unsafe {
        // Check if we can send more data
        while state.write_iterations_complete < state.default_iterations && 
              handle.peripheral.canSendWriteWithoutResponse() {
            
            let mtu = handle.peripheral.maximumWriteValueLengthForType(CBCharacteristicWriteType::WithoutResponse);
            let bytes_to_copy = std::cmp::min(mtu, test_data.len());
            
            let packet_data = objc2_foundation::NSData::from_vec(test_data[..bytes_to_copy].to_vec());
            
            if let Ok(string_from_data) = std::str::from_utf8(&test_data[..bytes_to_copy]) {
                println!("Writing {} bytes: {}", bytes_to_copy, string_from_data);
            } else {
                println!("Writing {} bytes", bytes_to_copy);
            }
            
            handle.peripheral.writeValue_forCharacteristic_type(
                &packet_data,
                &transfer_char,
                CBCharacteristicWriteType::WithoutResponse
            );
            
            state.write_iterations_complete += 1;
        }
        
        if state.write_iterations_complete == state.default_iterations {
            // Cancel subscription
            handle.peripheral.setNotifyValue_forCharacteristic(false, &transfer_char);
        }
    }
}

pub(crate) fn cleanup_peripheral(identifier: &str) {
    let peripherals = match CONNECTED_PERIPHERALS.get() {
        Some(p) => p,
        None => return,
    };
    
    let handle = {
        let guard = match peripherals.lock() {
            Ok(g) => g,
            Err(e) => {
                eprintln!("Failed to lock peripherals during cleanup: {:?}", e);
                return;
            }
        };
        
        match guard.get(identifier) {
            Some(h) => PeripheralHandle {
                peripheral: h.peripheral.retain(),
                delegate: h.delegate.retain(),
                transfer_characteristic: h.transfer_characteristic.as_ref().map(|c| c.retain()),
            },
            None => return,
        }
    };
    
    unsafe {
        if handle.peripheral.state() != CBPeripheralState::Connected {
            return;
        }
        
        // Unsubscribe from notifying characteristics
        if let Some(services) = handle.peripheral.services() {
            for i in 0..services.count() {
                let service_obj = services.objectAtIndex(i);
                if let Ok(service) = service_obj.downcast::<CBService>() {
                    if let Some(characteristics) = service.characteristics() {
                        for j in 0..characteristics.count() {
                            let char_obj = characteristics.objectAtIndex(j);
                            if let Ok(characteristic) = char_obj.downcast::<CBCharacteristic>() {
                                if characteristic.isNotifying() {
                                    handle.peripheral.setNotifyValue_forCharacteristic(false, &characteristic);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "MyPeripheralConnectionDelegate"]
    pub(crate) struct MyPeripheralConnectionDelegate;

    unsafe impl NSObjectProtocol for MyPeripheralConnectionDelegate {}
    unsafe impl CBPeripheralDelegate for MyPeripheralConnectionDelegate {
        #[unsafe(method(peripheral:didDiscoverServices:))]
        fn peripheral_did_discover_services(&self, peripheral: &CBPeripheral, error: Option<&objc2_foundation::NSError>) {
            let _ = catch_unwind(AssertUnwindSafe(|| {
                unsafe {
                    if let Some(err) = error {
                        println!("Error discovering services: {:?}", err);
                        cleanup_peripheral(&peripheral.identifier().UUIDString().to_string());
                        return;
                    }
                    
                    if let Some(services) = peripheral.services() {
                        let identifier = peripheral.identifier().UUIDString().to_string();
                        
                        // Update device info with discovered services
                        if let Some(devices) = DISCOVERED_DEVICES.get() {
                            if let Ok(mut guard) = devices.lock() {
                                if let Some(device) = guard.get_mut(&identifier) {
                                    device.services.clear();
                                    for i in 0..services.count() {
                                        let service = services.objectAtIndex(i);
                                        if let Ok(cb_service) = service.downcast::<CBService>() {
                                            let uuid = cb_service.UUID().UUIDString().to_string();
                                            device.services.push(uuid);
                                        }
                                    }
                                }
                            }
                        }
                        
                        // Find and discover characteristics ONLY for the target service
                        let mut found_target_service = false;
                        for i in 0..services.count() {
                            let service = services.objectAtIndex(i);
                            if let Ok(cb_service) = service.downcast::<CBService>() {
                                let service_uuid = cb_service.UUID().UUIDString().to_string();
                                
                                // Only discover characteristics for our target service
                                if service_uuid.eq_ignore_ascii_case(TARGET_SERVICE_UUID) {
                                    println!("Found target service {}, discovering characteristics", TARGET_SERVICE_UUID);
                                    peripheral.discoverCharacteristics_forService(None, &cb_service);
                                    found_target_service = true;
                                }
                            }
                        }
                        
                        if !found_target_service {
                            println!("Warning: Target service {} not found in discovered services", TARGET_SERVICE_UUID);
                            cleanup_peripheral(&identifier);
                        }
                    }
                }
            })).map_err(|e| eprintln!("Panic in peripheral_did_discover_services: {:?}", e));
        }

        #[unsafe(method(peripheral:didDiscoverCharacteristicsForService:error:))]
        fn peripheral_did_discover_characteristics_for_service(
            &self, peripheral: &CBPeripheral, service: &CBService, error: Option<&objc2_foundation::NSError>
        ) {
            let _ = catch_unwind(AssertUnwindSafe(|| {
                unsafe {
                    if let Some(err) = error {
                        println!("Error discovering characteristics: {:?}", err);
                        cleanup_peripheral(&peripheral.identifier().UUIDString().to_string());
                        return;
                    }
                    
                    let service_uuid = service.UUID().UUIDString().to_string();
                    let identifier = peripheral.identifier().UUIDString().to_string();
                    
                    // Only process characteristics if this is our target service
                    if !service_uuid.eq_ignore_ascii_case(TARGET_SERVICE_UUID) {
                        return;
                    }
                    
                    println!("Discovered characteristics for target service {}", TARGET_SERVICE_UUID);
                    
                    if let Some(characteristics) = service.characteristics() {
                        // Update device info
                        if let Some(devices) = DISCOVERED_DEVICES.get() {
                            if let Ok(mut guard) = devices.lock() {
                                if let Some(device) = guard.get_mut(&identifier) {
                                    let mut char_uuids = Vec::new();
                                    for i in 0..characteristics.count() {
                                        let char_obj = characteristics.objectAtIndex(i);
                                        if let Ok(characteristic) = char_obj.downcast::<CBCharacteristic>() {
                                            let char_uuid = characteristic.UUID().UUIDString().to_string();
                                            char_uuids.push(char_uuid);
                                        }
                                    }
                                    device.characteristics.insert(service_uuid.clone(), char_uuids);
                                }
                            }
                        }
                        
                        // Subscribe to notify/indicate characteristics and read from readable ones in the target service
                        let mut subscribed_count = 0;
                        let mut read_count = 0;
                        for i in 0..characteristics.count() {
                            let char_obj = characteristics.objectAtIndex(i);
                            if let Ok(characteristic) = char_obj.downcast::<CBCharacteristic>() {
                                let props = characteristic.properties();
                                let char_uuid = characteristic.UUID().UUIDString().to_string();
                                
                                // Subscribe to notify/indicate characteristics
                                if props.contains(CBCharacteristicProperties::Notify) || 
                                   props.contains(CBCharacteristicProperties::Indicate) {
                                    println!("Subscribing to characteristic {} in target service", char_uuid);
                                    
                                    // Save transfer characteristic
                                    if let Some(peripherals) = CONNECTED_PERIPHERALS.get() {
                                        if let Ok(mut guard) = peripherals.lock() {
                                            if let Some(handle) = guard.get_mut(&identifier) {
                                                handle.transfer_characteristic = Some(characteristic.retain());
                                            }
                                        }
                                    }
                                    
                                    // Subscribe to notifications
                                    peripheral.setNotifyValue_forCharacteristic(true, &characteristic);
                                    subscribed_count += 1;
                                }
                                
                                // Also read from readable characteristics
                                if props.contains(CBCharacteristicProperties::Read) {
                                    println!("Reading from characteristic {} in target service", char_uuid);
                                    peripheral.readValueForCharacteristic(&characteristic);
                                    read_count += 1;
                                }
                            }
                        }
                        
                        if subscribed_count > 0 {
                            println!("Subscribed to {} characteristic(s) in target service", subscribed_count);
                        }
                        if read_count > 0 {
                            println!("Reading from {} characteristic(s) in target service", read_count);
                        }
                        if subscribed_count == 0 && read_count == 0 {
                            println!("Warning: No notify/indicate/read characteristics found in target service");
                        }
                        
                        // Print GATT structure after discovery
                        print_gatt_structure(&identifier, peripheral);
                    }
                }
            })).map_err(|e| eprintln!("Panic in peripheral_did_discover_characteristics_for_service: {:?}", e));
        }

        #[unsafe(method(peripheral:didWriteValueForCharacteristic:error:))]
        fn peripheral_did_write_value_for_characteristic(
            &self, _peripheral: &CBPeripheral, characteristic: &CBCharacteristic, error: Option<&objc2_foundation::NSError>
        ) {
            let _ = catch_unwind(AssertUnwindSafe(|| {
                unsafe {
                    if let Some(err) = error {
                        println!("Write failed: {:?}", err);
                    } else {
                        println!("Write success: {}", characteristic.UUID().UUIDString());
                    }
                }
            })).map_err(|e| eprintln!("Panic in peripheral_did_write_value_for_characteristic: {:?}", e));
        }

        #[unsafe(method(peripheral:didUpdateValueForCharacteristic:error:))]
        fn peripheral_did_update_value_for_characteristic(
            &self, peripheral: &CBPeripheral, characteristic: &CBCharacteristic, error: Option<&objc2_foundation::NSError>
        ) {
            let _ = catch_unwind(AssertUnwindSafe(|| {
                unsafe {
                    if let Some(err) = error {
                        println!("Error updating value: {:?}", err);
                        cleanup_peripheral(&peripheral.identifier().UUIDString().to_string());
                        return;
                    }
                    
                    let identifier = peripheral.identifier().UUIDString().to_string();
                    let char_uuid = characteristic.UUID().UUIDString().to_string();
                    
                    if let Some(value) = characteristic.value() {
                        let len = value.len();
                        let bytes_ptr: *const std::ffi::c_void = msg_send![&*value, bytes];
                        let data = std::slice::from_raw_parts(bytes_ptr as *const u8, len);
                        
                        if let Ok(string_from_data) = std::str::from_utf8(data) {
                            println!("Received {} bytes from {}: {}", len, char_uuid, string_from_data);
                            
                            if string_from_data == "EOM" {
                                let data_buffer = DATA_BUFFER.get_or_init(|| Mutex::new(HashMap::new()));
                                if let Ok(mut buffer_guard) = data_buffer.lock() {
                                    if let Some(accumulated_data) = buffer_guard.get(&identifier) {
                                        if let Ok(text) = std::str::from_utf8(accumulated_data) {
                                            println!("Complete message received: {}", text);
                                            
                                            // Store the complete message
                                            let messages = RECEIVED_MESSAGES.get_or_init(|| Mutex::new(HashMap::new()));
                                            if let Ok(mut msg_guard) = messages.lock() {
                                                msg_guard.entry(identifier.clone())
                                                    .or_insert_with(Vec::new)
                                                    .push(text.to_string());
                                            }
                                        }
                                    }
                                    buffer_guard.remove(&identifier);
                                }
                                
                            } else {
                                let data_buffer = DATA_BUFFER.get_or_init(|| Mutex::new(HashMap::new()));
                                if let Ok(mut buffer_guard) = data_buffer.lock() {
                                    buffer_guard.entry(identifier.clone())
                                        .or_insert_with(Vec::new)
                                        .extend_from_slice(data);
                                }
                                
                                // Also store individual messages that aren't part of a multi-packet transfer
                                let messages = RECEIVED_MESSAGES.get_or_init(|| Mutex::new(HashMap::new()));
                                if let Ok(mut msg_guard) = messages.lock() {
                                    msg_guard.entry(identifier.clone())
                                        .or_insert_with(Vec::new)
                                        .push(string_from_data.to_string());
                                }
                            }
                        } else {
                            println!("Read from {}: {:02X?}", char_uuid, data);
                        }
                    }
                }
            })).map_err(|e| eprintln!("Panic in peripheral_did_update_value_for_characteristic: {:?}", e));
        }
        
        #[unsafe(method(peripheral:didUpdateNotificationStateForCharacteristic:error:))]
        fn peripheral_did_update_notification_state_for_characteristic(
            &self, peripheral: &CBPeripheral, characteristic: &CBCharacteristic, error: Option<&objc2_foundation::NSError>
        ) {
            let _ = catch_unwind(AssertUnwindSafe(|| {
                unsafe {
                    if let Some(err) = error {
                        println!("Error changing notification state: {:?}", err);
                        return;
                    }
                    
                    let identifier = peripheral.identifier().UUIDString().to_string();
                    
                    if characteristic.isNotifying() {
                        println!("Notification began on {}", characteristic.UUID().UUIDString());
                    } else {
                        // Notification has stopped, so disconnect
                        println!("Notification stopped on {}. Disconnecting", characteristic.UUID().UUIDString());
                        cleanup_peripheral(&identifier);
                    }
                }
            })).map_err(|e| eprintln!("Panic in peripheral_did_update_notification_state_for_characteristic: {:?}", e));
        }
        
        #[unsafe(method(peripheralIsReadyToSendWriteWithoutResponse:))]
        fn peripheral_is_ready_to_send_write_without_response(&self, peripheral: &CBPeripheral) {
            let _ = catch_unwind(AssertUnwindSafe(|| {
                let identifier = unsafe {
                    peripheral.identifier().UUIDString().to_string()
                };
                println!("Peripheral is ready, send data");

                let test_data = b"Test data from Rust central";
                write_data_to_peripheral(&identifier, test_data);
            })).map_err(|e| eprintln!("Panic in peripheral_is_ready_to_send_write_without_response: {:?}", e));
        }
    }
);

impl MyPeripheralConnectionDelegate {
    pub(crate) fn new() -> Retained<Self> {
        unsafe { msg_send![Self::alloc(), init] }
    }
}

pub(crate) fn cleanup_all_peripherals() {
    if let Some(peripherals) = CONNECTED_PERIPHERALS.get() {
        if let Ok(mut guard) = peripherals.lock() {
            guard.clear();
        }
    }
    
    if let Some(data_buffer) = DATA_BUFFER.get() {
        if let Ok(mut guard) = data_buffer.lock() {
            guard.clear();
        }
    }
    
    if let Some(iterations) = ITERATION_STATE.get() {
        if let Ok(mut guard) = iterations.lock() {
            guard.clear();
        }
    }
    
    if let Some(messages) = RECEIVED_MESSAGES.get() {
        if let Ok(mut guard) = messages.lock() {
            guard.clear();
        }
    }
}