// bluetooth/central_manager.rs - Auto-connecting Central Manager
#![cfg(any(target_os = "macos", target_os = "ios"))]

use std::sync::{Mutex, OnceLock};
use std::collections::HashMap;
use objc2::rc::Retained;
use objc2::runtime::{NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send};
use objc2_foundation::{NSObject, NSString, NSArray, NSDictionary, NSNumber, NSUUID};
use objc2_core_bluetooth::*;
use objc2::runtime::AnyObject;
use objc2::Message;
use objc2::AnyThread;

use crate::hardware::bluetooth::{DeviceInfo, DISCOVERED_DEVICES};
use super::peripheral_connection::{
    MyPeripheralConnectionDelegate, PeripheralHandle, CONNECTED_PERIPHERALS,
    ITERATION_STATE, DATA_BUFFER, IterationState, cleanup_peripheral
};

// Target service UUID - only connect to devices advertising this service
const TARGET_SERVICE_UUID: &str = "E20A39F4-73F5-4BC4-A12F-17D1AD07A961";

struct CentralWrapper {
    delegate: Retained<MyCentralDelegate>,
    manager: Retained<CBCentralManager>,
}

unsafe impl Send for CentralWrapper {}

static CENTRAL_INSTANCE: OnceLock<Mutex<Option<CentralWrapper>>> = OnceLock::new();

// Track devices we're currently trying to connect to
static CONNECTING_DEVICES: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "MyCentralDelegate"]
    struct MyCentralDelegate;

    unsafe impl NSObjectProtocol for MyCentralDelegate {}
    unsafe impl CBCentralManagerDelegate for MyCentralDelegate {
        #[unsafe(method(centralManagerDidUpdateState:))]
        fn central_manager_did_update_state(&self, central: &CBCentralManager) {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                unsafe {
                    match central.state() {
                        CBManagerState::PoweredOn => {
                            println!("CBManager is powered on - starting auto-scan");
                            start_scanning_for_target_service(central);
                        }
                        CBManagerState::PoweredOff => println!("CBManager is not powered on"),
                        CBManagerState::Resetting => println!("CBManager is resetting"),
                        CBManagerState::Unauthorized => println!("BLE unauthorized"),
                        CBManagerState::Unknown => println!("CBManager state is unknown"),
                        CBManagerState::Unsupported => println!("Bluetooth is not supported on this device"),
                        _ => println!("A previously unknown central manager state occurred"),
                    }
                }
            }));
        }

        #[unsafe(method(centralManager:didDiscoverPeripheral:advertisementData:RSSI:))]
        fn central_manager_did_discover_peripheral_advertisement_data_rssi(
            &self, central: &CBCentralManager, peripheral: &CBPeripheral, 
            advertisement_data: &NSDictionary, rssi: &NSNumber,
        ) {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                unsafe {
                    let rssi_val = rssi.intValue();
                    let identifier = peripheral.identifier().UUIDString().to_string();
                    
                    // Reject if signal strength is too low
                    if rssi_val < -50 {
                        return;
                    }
                    
                    // Check if device advertises the target service UUID
                    let mut has_target_service = false;
                    if let Some(svc_uuids) = advertisement_data.objectForKey(&*CBAdvertisementDataServiceUUIDsKey) {
                        if let Ok(uuid_array) = svc_uuids.downcast::<NSArray>() {
                            for i in 0..uuid_array.count() {
                                if let Ok(uuid) = uuid_array.objectAtIndex(i).downcast::<CBUUID>() {
                                    let uuid_str = uuid.UUIDString().to_string();
                                    if uuid_str.eq_ignore_ascii_case(TARGET_SERVICE_UUID) {
                                        has_target_service = true;
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    
                    if !has_target_service {
                        return;
                    }
                    
                    // Initialize CONNECTED_PERIPHERALS if needed
                    CONNECTED_PERIPHERALS.get_or_init(|| Mutex::new(HashMap::new()));
                    
                    // Check if already connected
                    let already_connected = CONNECTED_PERIPHERALS.get()
                        .and_then(|p| p.lock().ok())
                        .map(|guard| guard.contains_key(&identifier))
                        .unwrap_or(false);
                    
                    if already_connected {
                        return;
                    }
                    
                    // Check if already trying to connect
                    let connecting = CONNECTING_DEVICES.get_or_init(|| Mutex::new(std::collections::HashSet::new()));
                    let already_connecting = connecting.lock()
                        .map(|guard| guard.contains(&identifier))
                        .unwrap_or(false);
                    
                    if already_connecting {
                        return;
                    }

                    let mut device_name = "Unknown".to_string();
                    if let Some(local_name) = advertisement_data.objectForKey(&*CBAdvertisementDataLocalNameKey) {
                        if let Ok(name_str) = local_name.downcast::<NSString>() {
                            device_name = name_str.to_string();
                        }
                    } else if let Some(name) = peripheral.name() {
                        device_name = name.to_string();
                    }

                    println!("Auto-discovered target device {} at {} dBm", device_name, rssi_val);

                    let is_connectable = if let Some(connectable) = advertisement_data.objectForKey(&*CBAdvertisementDataIsConnectable) {
                        if let Ok(conn_bool) = connectable.downcast::<NSNumber>() {
                            conn_bool.boolValue()
                        } else { true }
                    } else { true };

                    let mut service_uuids = Vec::new();
                    if let Some(svc_uuids) = advertisement_data.objectForKey(&*CBAdvertisementDataServiceUUIDsKey) {
                        if let Ok(uuid_array) = svc_uuids.downcast::<NSArray>() {
                            for i in 0..uuid_array.count() {
                                if let Ok(uuid) = uuid_array.objectAtIndex(i).downcast::<CBUUID>() {
                                    service_uuids.push(uuid.UUIDString().to_string());
                                }
                            }
                        }
                    }

                    let mut service_data = HashMap::new();
                    if let Some(svc_data) = advertisement_data.objectForKey(&*CBAdvertisementDataServiceDataKey) {
                        if let Ok(svc_dict) = svc_data.downcast::<NSDictionary>() {
                            let keys = svc_dict.allKeys();
                            for i in 0..keys.count() {
                                let key_obj = keys.objectAtIndex(i);
                                if let Ok(uuid) = key_obj.downcast::<CBUUID>() {
                                    if let Some(data_obj) = svc_dict.objectForKey(&*uuid) {
                                        if let Ok(data) = data_obj.downcast::<objc2_foundation::NSData>() {
                                            let len = data.len();
                                            let bytes_ptr: *const std::ffi::c_void = msg_send![&*data, bytes];
                                            let vec = std::slice::from_raw_parts(bytes_ptr as *const u8, len).to_vec();
                                            let uuid_str = uuid.UUIDString().to_string();
                                            service_data.insert(uuid_str, vec);
                                        }
                                    }
                                }
                            }
                        }
                    }

                    let mut manufacturer_data = None;
                    if let Some(mfg_data) = advertisement_data.objectForKey(&*CBAdvertisementDataManufacturerDataKey) {
                        if let Ok(data) = mfg_data.downcast::<objc2_foundation::NSData>() {
                            let len = data.len();
                            let bytes_ptr: *const std::ffi::c_void = msg_send![&*data, bytes];
                            manufacturer_data = Some(std::slice::from_raw_parts(bytes_ptr as *const u8, len).to_vec());
                        }
                    }

                    let mut tx_power_level = None;
                    if let Some(tx_power) = advertisement_data.objectForKey(&*CBAdvertisementDataTxPowerLevelKey) {
                        if let Ok(power_num) = tx_power.downcast::<NSNumber>() {
                            tx_power_level = Some(power_num.intValue());
                        }
                    }

                    let device_info = DeviceInfo {
                        name: device_name.clone(),
                        rssi: rssi_val,
                        is_connected: false,
                        last_seen: std::time::SystemTime::now(),
                        services: service_uuids,
                        characteristics: HashMap::new(),
                        manufacturer_data,
                        service_data,
                        tx_power_level,
                        is_connectable,
                        advertisement_data: HashMap::new(),
                    };

                    if let Some(discovered) = DISCOVERED_DEVICES.get_or_init(|| Mutex::new(HashMap::new())).lock().ok() {
                        let mut guard = discovered;
                        guard.insert(identifier.clone(), device_info);
                    }
                    
                    if is_connectable {
                        println!("Auto-connecting to {}", identifier);
                        
                        // Mark as connecting
                        if let Ok(mut guard) = connecting.lock() {
                            guard.insert(identifier.clone());
                        }
                        
                        // CRITICAL: Save peripheral reference BEFORE connecting (prevents CoreBluetooth from deallocating it)
                        // But DON'T set the delegate yet - that happens in didConnect
                        let peripherals = CONNECTED_PERIPHERALS.get_or_init(|| Mutex::new(HashMap::new()));
                        if let Ok(mut guard) = peripherals.lock() {
                            // Create placeholder delegate (will be replaced in didConnect)
                            let placeholder_delegate = MyPeripheralConnectionDelegate::new();
                            guard.insert(
                                identifier.clone(),
                                PeripheralHandle {
                                    peripheral: peripheral.retain(),
                                    delegate: placeholder_delegate,
                                    transfer_characteristic: None,
                                }
                            );
                        }
                        
                        // Now connect (peripheral reference is saved, so CoreBluetooth won't deallocate)
                        central.connectPeripheral_options(peripheral, None);
                    }
                }
            }));
        }

        #[unsafe(method(centralManager:didConnectPeripheral:))]
        fn central_manager_did_connect_peripheral(&self, central: &CBCentralManager, peripheral: &CBPeripheral) {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                unsafe {
                    let identifier = peripheral.identifier().UUIDString().to_string();
                    let name = peripheral.name().map(|n| n.to_string()).unwrap_or("Unknown".to_string());
                    
                    println!("✓ Successfully connected to peripheral: {} ({})", name, identifier);
                    
                    // Remove from connecting set
                    if let Some(connecting) = CONNECTING_DEVICES.get() {
                        if let Ok(mut guard) = connecting.lock() {
                            guard.remove(&identifier);
                        }
                    }
                    
                    central.stopScan();
                    println!("Scanning stopped");
                    
                    // NOW set the delegate (after connection is established, like Swift example)
                    let delegate = MyPeripheralConnectionDelegate::new();
                    peripheral.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
                    
                    // Update the stored peripheral handle with the new delegate
                    let peripherals = CONNECTED_PERIPHERALS.get_or_init(|| Mutex::new(HashMap::new()));
                    if let Ok(mut guard) = peripherals.lock() {
                        if let Some(handle) = guard.get_mut(&identifier) {
                            // Update the delegate
                            handle.delegate = delegate;
                        } else {
                            // If not found, insert it (shouldn't happen, but be safe)
                            guard.insert(
                                identifier.clone(),
                                PeripheralHandle {
                                    peripheral: peripheral.retain(),
                                    delegate,
                                    transfer_characteristic: None,
                                }
                            );
                        }
                    }
                    
                    // Initialize iteration state
                    let iterations = ITERATION_STATE.get_or_init(|| Mutex::new(HashMap::new()));
                    if let Ok(mut guard) = iterations.lock() {
                        let state = guard.entry(identifier.clone()).or_insert(IterationState {
                            connection_iterations_complete: 0,
                            write_iterations_complete: 0,
                            default_iterations: 5,
                        });
                        state.connection_iterations_complete += 1;
                        state.write_iterations_complete = 0;
                    }
                    
                    // Initialize data buffer (clear any old data)
                    let data_buffer = DATA_BUFFER.get_or_init(|| Mutex::new(HashMap::new()));
                    if let Ok(mut buffer_guard) = data_buffer.lock() {
                        buffer_guard.insert(identifier.clone(), Vec::new());
                    }
                    
                    // Update device info
                    if let Some(devices) = DISCOVERED_DEVICES.get() {
                        if let Ok(mut guard) = devices.lock() {
                            if let Some(device) = guard.get_mut(&identifier) {
                                device.is_connected = true;
                            }
                        }
                    }
                    
                    // Start service discovery (search only for our target service)
                    println!("Discovering services...");
                    let target_uuid_string = NSString::from_str(TARGET_SERVICE_UUID);
                    let target_cbuuid = CBUUID::UUIDWithString(&target_uuid_string);
                    let service_uuids = NSArray::from_slice(&[&*target_cbuuid]);
                    peripheral.discoverServices(Some(&service_uuids));
                }
            }));
        }

        #[unsafe(method(centralManager:didFailToConnectPeripheral:error:))]
        fn central_manager_did_fail_to_connect_peripheral(
            &self, central: &CBCentralManager, peripheral: &CBPeripheral, error: Option<&objc2_foundation::NSError>
        ) {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                unsafe {
                    let identifier = peripheral.identifier().UUIDString().to_string();
                    let name = peripheral.name().map(|n| n.to_string()).unwrap_or("Unknown".to_string());
                    println!("✗ Failed to auto-connect to {} ({}): {:?}", name, identifier, error);
                    
                    // Remove from connecting set
                    if let Some(connecting) = CONNECTING_DEVICES.get() {
                        if let Ok(mut guard) = connecting.lock() {
                            guard.remove(&identifier);
                        }
                    }
                    
                    cleanup_peripheral(&identifier);
                    
                    if let Some(peripherals) = CONNECTED_PERIPHERALS.get() {
                        if let Ok(mut guard) = peripherals.lock() {
                            guard.remove(&identifier);
                        }
                    }
                    
                    // Resume scanning
                    println!("Resuming scan...");
                    start_scanning_for_target_service(central);
                }
            }));
        }

        #[unsafe(method(centralManager:didDisconnectPeripheral:error:))]
        fn central_manager_did_disconnect_peripheral(
            &self, central: &CBCentralManager, peripheral: &CBPeripheral, error: Option<&objc2_foundation::NSError>
        ) {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                unsafe {
                    let identifier = peripheral.identifier().UUIDString().to_string();
                    let name = peripheral.name().map(|n| n.to_string()).unwrap_or("Unknown".to_string());
                    
                    // Remove from connecting set
                    if let Some(connecting) = CONNECTING_DEVICES.get() {
                        if let Ok(mut guard) = connecting.lock() {
                            guard.remove(&identifier);
                        }
                    }
                    
                    if let Some(err) = error {
                        println!("✗ Peripheral disconnected: {} ({}) - Error: {:?}", name, identifier, err);
                    } else {
                        println!("✓ Peripheral cleanly disconnected: {} ({})", name, identifier);
                    }
                    
                    if let Some(devices) = DISCOVERED_DEVICES.get() {
                        if let Ok(mut guard) = devices.lock() {
                            if let Some(device) = guard.get_mut(&identifier) {
                                device.is_connected = false;
                            }
                        }
                    }
                    
                    if let Some(peripherals) = CONNECTED_PERIPHERALS.get() {
                        if let Ok(mut guard) = peripherals.lock() {
                            guard.remove(&identifier);
                        }
                    }
                    
                    // Check if should auto-reconnect
                    let iterations = ITERATION_STATE.get_or_init(|| Mutex::new(HashMap::new()));
                    let should_reconnect = if let Ok(guard) = iterations.lock() {
                        if let Some(state) = guard.get(&identifier) {
                            state.connection_iterations_complete < state.default_iterations
                        } else {
                            false
                        }
                    } else {
                        false
                    };
                    
                    if should_reconnect {
                        if let Ok(guard) = iterations.lock() {
                            if let Some(state) = guard.get(&identifier) {
                                println!("Auto-reconnecting... ({}/{})", state.connection_iterations_complete, state.default_iterations);
                            }
                        }
                        retrieve_peripheral(central);
                    } else {
                        println!("Connection iterations completed - resuming scan");
                        start_scanning_for_target_service(central);
                    }
                }
            }));
        }
    }
);

impl MyCentralDelegate {
    fn new() -> Retained<Self> {
        unsafe { msg_send![Self::alloc(), init] }
    }
}

unsafe fn start_scanning_for_target_service(central: &CBCentralManager) {
    let target_uuid_string = NSString::from_str(TARGET_SERVICE_UUID);
    let target_cbuuid = CBUUID::UUIDWithString(&target_uuid_string);
    let service_uuids = NSArray::from_slice(&[&*target_cbuuid]);
    
    // Don't allow duplicates - this prevents multiple connection attempts
    let options = NSDictionary::from_slices(
        &[&*CBCentralManagerScanOptionAllowDuplicatesKey],
        &[&*NSNumber::numberWithBool(false) as &AnyObject]
    );
    
    println!("Scanning for service UUID: {}", TARGET_SERVICE_UUID);
    central.scanForPeripheralsWithServices_options(Some(&service_uuids), Some(&options));
}

fn retrieve_peripheral(central: &CBCentralManager) {
    unsafe {
        let target_uuid_string = NSString::from_str(TARGET_SERVICE_UUID);
        let target_cbuuid = CBUUID::UUIDWithString(&target_uuid_string);
        let service_uuids = NSArray::from_slice(&[&*target_cbuuid]);
        
        let connected_peripherals = central.retrieveConnectedPeripheralsWithServices(&service_uuids);
        
        if connected_peripherals.count() > 0 {
            let peripheral_obj = connected_peripherals.objectAtIndex(connected_peripherals.count() - 1);
            if let Ok(peripheral) = peripheral_obj.downcast::<CBPeripheral>() {
                let identifier = peripheral.identifier().UUIDString().to_string();
                
                // Check if already trying to connect
                let connecting = CONNECTING_DEVICES.get_or_init(|| Mutex::new(std::collections::HashSet::new()));
                let already_connecting = connecting.lock()
                    .map(|guard| guard.contains(&identifier))
                    .unwrap_or(false);
                
                if already_connecting {
                    start_scanning_for_target_service(central);
                    return;
                }
                
                println!("Auto-reconnecting to peripheral {}", identifier);
                
                // Mark as connecting
                if let Ok(mut guard) = connecting.lock() {
                    guard.insert(identifier.clone());
                }
                
                // Save peripheral reference BEFORE connecting
                let peripherals = CONNECTED_PERIPHERALS.get_or_init(|| Mutex::new(HashMap::new()));
                if let Ok(mut guard) = peripherals.lock() {
                    let placeholder_delegate = MyPeripheralConnectionDelegate::new();
                    guard.insert(
                        identifier.clone(),
                        PeripheralHandle {
                            peripheral: peripheral.retain(),
                            delegate: placeholder_delegate,
                            transfer_characteristic: None,
                        }
                    );
                }
                
                // Connect (delegate will be set in didConnect callback)
                central.connectPeripheral_options(&peripheral, None);
                return;
            }
        }
        
        start_scanning_for_target_service(central);
    }
}

/// Initialize and start the BLE central manager
/// This will automatically scan, connect, and read data from devices advertising the target service
pub fn start_central() {
    // Initialize all static collections
    CONNECTED_PERIPHERALS.get_or_init(|| Mutex::new(HashMap::new()));
    CONNECTING_DEVICES.get_or_init(|| Mutex::new(std::collections::HashSet::new()));
    ITERATION_STATE.get_or_init(|| Mutex::new(HashMap::new()));
    DATA_BUFFER.get_or_init(|| Mutex::new(HashMap::new()));
    
    CENTRAL_INSTANCE.get_or_init(|| {
        let delegate = MyCentralDelegate::new();
        let manager = unsafe {
            CBCentralManager::initWithDelegate_queue(
                CBCentralManager::alloc(),
                Some(ProtocolObject::from_ref(&*delegate)),
                None
            )
        };
        println!("BLE Central Manager started - will auto-connect when powered on");
        Mutex::new(Some(CentralWrapper { delegate, manager }))
    });
}

/// Stop the central manager and cleanup all connections
pub fn stop_central() {
    // Clear connecting devices set
    if let Some(connecting) = CONNECTING_DEVICES.get() {
        if let Ok(mut guard) = connecting.lock() {
            guard.clear();
        }
    }
    
    if let Some(peripherals_lock) = CONNECTED_PERIPHERALS.get() {
        if let Ok(guard) = peripherals_lock.lock() {
            if let Some(central_lock) = CENTRAL_INSTANCE.get() {
                if let Ok(central_guard) = central_lock.lock() {
                    if let Some(central) = central_guard.as_ref() {
                        for (_, handle) in guard.iter() {
                            unsafe {
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
                                central.manager.cancelPeripheralConnection(&handle.peripheral);
                            }
                        }
                    }
                }
            }
        }
    }
    
    if let Some(central_lock) = CENTRAL_INSTANCE.get() {
        if let Ok(mut guard) = central_lock.lock() {
            if let Some(central) = guard.as_ref() {
                unsafe {
                    central.manager.stopScan();
                    println!("BLE Central Manager stopped");
                }
            }
            *guard = None;
        }
    }
    
    if let Some(peripherals) = CONNECTED_PERIPHERALS.get() {
        if let Ok(mut guard) = peripherals.lock() {
            guard.clear();
        }
    }
}