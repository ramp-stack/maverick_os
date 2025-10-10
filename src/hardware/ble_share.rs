use std::sync::{Mutex, OnceLock, Arc};
use std::collections::HashMap;
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send};
use objc2_foundation::{NSString, NSData};
use objc2_core_bluetooth::*;
use objc2::AnyThread;
use objc2::Message;

#[derive(Clone, Debug)]
pub struct DeviceInfo {
    pub name: String,
    pub rssi: i32,
    pub is_connected: bool,
    pub last_seen: std::time::SystemTime,
    pub services: Vec<String>,
    pub characteristics: HashMap<String, Vec<String>>,
    pub manufacturer_data: Option<Vec<u8>>,
    pub service_data: HashMap<String, Vec<u8>>,
    pub tx_power_level: Option<i32>,
    pub is_connectable: bool,
    pub advertisement_data: HashMap<String, String>,
}

static DISCOVERED_DEVICES: OnceLock<Mutex<HashMap<String, DeviceInfo>>> = OnceLock::new();

// Thread-safe wrapper for CBPeripheral
struct PeripheralHandle {
    peripheral: Retained<CBPeripheral>,
}

// SAFETY: We ensure all access happens on the main thread via the Mutex
unsafe impl Send for PeripheralHandle {}
unsafe impl Sync for PeripheralHandle {}

static CONNECTED_PERIPHERALS: OnceLock<Mutex<HashMap<String, PeripheralHandle>>> = OnceLock::new();

// Thread-safe wrappers for Objective-C objects
struct CentralWrapper {
    delegate: Retained<MyCentralDelegate>,
    manager: Retained<CBCentralManager>,
}

struct PeripheralWrapper {
    delegate: Retained<MyPeripheralDelegate>,
    manager: Retained<CBPeripheralManager>,
}

// SAFETY: These are only accessed from the main thread via the hardware context
unsafe impl Send for CentralWrapper {}
unsafe impl Send for PeripheralWrapper {}

// CRITICAL: Store the instances globally so they're never dropped
static CENTRAL_INSTANCE: OnceLock<Mutex<Option<CentralWrapper>>> = OnceLock::new();
static PERIPHERAL_INSTANCE: OnceLock<Mutex<Option<PeripheralWrapper>>> = OnceLock::new();

// Peripheral Delegate
define_class!(
    #[unsafe(super(NSObject))]
    #[name = "MyPeripheralDelegate"]
    struct MyPeripheralDelegate;

    unsafe impl NSObjectProtocol for MyPeripheralDelegate {}
    unsafe impl CBPeripheralManagerDelegate for MyPeripheralDelegate {
        #[unsafe(method(peripheralManagerDidUpdateState:))]
        fn peripheral_manager_did_update_state(&self, peripheral: &CBPeripheralManager) {
            unsafe {
                if peripheral.state() == CBManagerState::PoweredOn {
                    println!("Peripheral powered on");
                } else {
                    println!("Peripheral state: {:?}", peripheral.state());
                }
            }
        }
    }
);

impl MyPeripheralDelegate {
    fn new() -> Retained<Self> {
        unsafe { msg_send![Self::alloc(), init] }
    }
}

pub fn get_all_devices() -> Vec<DeviceInfo> {
    DISCOVERED_DEVICES.get_or_init(|| Mutex::new(HashMap::new()))
        .lock().map(|g| g.values().cloned().collect()).unwrap_or_default()
}

pub fn get_devices_in_range() -> Vec<DeviceInfo> {
    const DEFAULT_SHARING_THRESHOLD: i32 = -70;
    DISCOVERED_DEVICES.get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map(|g| {
            g.values()
                .filter(|device| device.rssi > DEFAULT_SHARING_THRESHOLD)
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

pub fn cleanup_bluetooth() {
    // Disconnect all peripherals
    if let Some(peripherals_lock) = CONNECTED_PERIPHERALS.get() {
        if let Ok(guard) = peripherals_lock.lock() {
            if let Some(central_lock) = CENTRAL_INSTANCE.get() {
                if let Ok(central_guard) = central_lock.lock() {
                    if let Some(central) = central_guard.as_ref() {
                        for handle in guard.values() {
                            unsafe {
                                central.manager.cancelPeripheralConnection(&handle.peripheral);
                            }
                        }
                    }
                }
            }
        }
    }
    
    // Stop scanning
    if let Some(central_lock) = CENTRAL_INSTANCE.get() {
        if let Ok(mut guard) = central_lock.lock() {
            if let Some(central) = guard.as_ref() {
                unsafe {
                    central.manager.stopScan();
                }
            }
            *guard = None;
        }
    }
    
    // Clear peripheral
    if let Some(peripheral_lock) = PERIPHERAL_INSTANCE.get() {
        if let Ok(mut guard) = peripheral_lock.lock() {
            *guard = None;
        }
    }
    
    // Clear devices and connections
    if let Some(devices) = DISCOVERED_DEVICES.get() {
        if let Ok(mut guard) = devices.lock() {
            guard.clear();
        }
    }
    
    if let Some(peripherals) = CONNECTED_PERIPHERALS.get() {
        if let Ok(mut guard) = peripherals.lock() {
            guard.clear();
        }
    }
    
    println!("Bluetooth cleanup complete");
}

// CBPeripheral Delegate for handling connections and characteristics
use objc2::runtime::AnyObject;
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSError};

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "MyPeripheralConnectionDelegate"]
    struct MyPeripheralConnectionDelegate;

    unsafe impl NSObjectProtocol for MyPeripheralConnectionDelegate {}
    unsafe impl CBPeripheralDelegate for MyPeripheralConnectionDelegate {
        #[unsafe(method(peripheral:didDiscoverServices:))]
        fn peripheral_did_discover_services(&self, peripheral: &CBPeripheral, error: Option<&NSError>) {
            unsafe {
                if let Some(err) = error {
                    println!("Error discovering services: {:?}", err);
                    return;
                }
                
                if let Some(services) = peripheral.services() {
                    println!("Discovered {} services", services.count());
                    for i in 0..services.count() {
                        let service = services.objectAtIndex(i);
                        if let Ok(cb_service) = service.downcast::<CBService>() {
                            let uuid = cb_service.UUID().UUIDString().to_string();
                            println!("Service: {}", uuid);
                            peripheral.discoverCharacteristics_forService(None, &cb_service);
                        }
                    }
                }
            }
        }

        #[unsafe(method(peripheral:didDiscoverCharacteristicsForService:error:))]
        fn peripheral_did_discover_characteristics_for_service(
            &self, peripheral: &CBPeripheral, service: &CBService, error: Option<&NSError>
        ) {
            unsafe {
                if let Some(err) = error {
                    println!("Error discovering characteristics: {:?}", err);
                    return;
                }
                
                let service_uuid = service.UUID().UUIDString().to_string();
                if let Some(characteristics) = service.characteristics() {
                    println!("Service {} has {} characteristics", service_uuid, characteristics.count());
                    
                    let identifier = peripheral.identifier().UUIDString().to_string();
                    if let Some(devices) = DISCOVERED_DEVICES.get() {
                        if let Ok(mut guard) = devices.lock() {
                            if let Some(device) = guard.get_mut(&identifier) {
                                let mut char_uuids = Vec::new();
                                for i in 0..characteristics.count() {
                                    let char_obj = characteristics.objectAtIndex(i);
                                    if let Ok(characteristic) = char_obj.downcast::<CBCharacteristic>() {
                                        let char_uuid = characteristic.UUID().UUIDString().to_string();
                                        char_uuids.push(char_uuid.clone());
                                        println!("  Characteristic: {} (Properties: {:?})", 
                                                 char_uuid, characteristic.properties());
                                    }
                                }
                                device.characteristics.insert(service_uuid.clone(), char_uuids);
                            }
                        }
                    }
                }
            }
        }

        #[unsafe(method(peripheral:didWriteValueForCharacteristic:error:))]
        fn peripheral_did_write_value_for_characteristic(
            &self, _peripheral: &CBPeripheral, characteristic: &CBCharacteristic, error: Option<&NSError>
        ) {
            unsafe {
                if let Some(err) = error {
                    println!("Error writing to characteristic: {:?}", err);
                } else {
                    println!("Successfully wrote to characteristic: {}", 
                             characteristic.UUID().UUIDString());
                }
            }
        }
    }
);

impl MyPeripheralConnectionDelegate {
    fn new() -> Retained<Self> {
        unsafe { msg_send![Self::alloc(), init] }
    }
}

// Central Manager with Connection Support
define_class!(
    #[unsafe(super(NSObject))]
    #[name = "MyCentralDelegate"]
    struct MyCentralDelegate;

    unsafe impl NSObjectProtocol for MyCentralDelegate {}
    unsafe impl CBCentralManagerDelegate for MyCentralDelegate {
        #[unsafe(method(centralManagerDidUpdateState:))]
        fn central_manager_did_update_state(&self, central: &CBCentralManager) {
            unsafe {
                match central.state() {
                    CBManagerState::PoweredOn => {
                        println!("Central powered on, starting scan");
                        let options = NSDictionary::from_slices(
                            &[&*CBCentralManagerScanOptionAllowDuplicatesKey],
                            &[&*NSNumber::numberWithBool(true) as &AnyObject]
                        );
                        central.scanForPeripheralsWithServices_options(None, Some(&options));
                    }
                    CBManagerState::PoweredOff => println!("Bluetooth powered off"),
                    CBManagerState::Unauthorized => println!("Bluetooth unauthorized"),
                    state => println!("Central state: {:?}", state),
                }
            }
        }

        #[unsafe(method(centralManager:didDiscoverPeripheral:advertisementData:RSSI:))]
        fn central_manager_did_discover_peripheral_advertisement_data_rssi(
            &self, _central: &CBCentralManager, peripheral: &CBPeripheral, 
            advertisement_data: &NSDictionary, rssi: &NSNumber,
        ) {
            unsafe {
                let rssi_val = rssi.intValue();
                let identifier = peripheral.identifier().UUIDString().to_string();

                let mut device_name = "Unknown".to_string();
                if let Some(name) = peripheral.name() {
                    device_name = name.to_string();
                } else if let Some(local_name) = advertisement_data.objectForKey(&*CBAdvertisementDataLocalNameKey) {
                    if let Ok(name_str) = local_name.downcast::<NSString>() {
                        device_name = name_str.to_string();
                    }
                }

                let mut tx_power_level = None;
                let mut is_connectable = false;
                let mut advertisement_info = HashMap::new();

                if let Some(tx_power) = advertisement_data.objectForKey(&*CBAdvertisementDataTxPowerLevelKey) {
                    if let Ok(power_num) = tx_power.downcast::<NSNumber>() {
                        tx_power_level = Some(power_num.intValue());
                        advertisement_info.insert("TxPowerLevel".to_string(), power_num.intValue().to_string());
                    }
                }

                if let Some(connectable) = advertisement_data.objectForKey(&*CBAdvertisementDataIsConnectable) {
                    if let Ok(conn_bool) = connectable.downcast::<NSNumber>() {
                        is_connectable = conn_bool.boolValue();
                        advertisement_info.insert("Connectable".to_string(), conn_bool.boolValue().to_string());
                    }
                }

                let mut service_uuids = Vec::new();
                if let Some(services) = advertisement_data.objectForKey(&*CBAdvertisementDataServiceUUIDsKey) {
                    if let Ok(service_array) = services.downcast::<NSArray>() {
                        for i in 0..service_array.count() {
                            let service_obj = service_array.objectAtIndex(i);
                            if let Ok(service_uuid) = service_obj.downcast::<CBUUID>() {
                                service_uuids.push(service_uuid.UUIDString().to_string());
                            }
                        }
                        advertisement_info.insert("AdvertisedServices".to_string(), service_uuids.join(", "));
                    }
                }

                let device_info = DeviceInfo {
                    name: device_name.clone(),
                    rssi: rssi_val,
                    is_connected: false,
                    last_seen: std::time::SystemTime::now(),
                    services: service_uuids,
                    characteristics: HashMap::new(),
                    manufacturer_data: None,
                    service_data: HashMap::new(),
                    tx_power_level,
                    is_connectable,
                    advertisement_data: advertisement_info,
                };

                DISCOVERED_DEVICES.get_or_init(|| Mutex::new(HashMap::new()))
                    .lock().unwrap().insert(identifier.clone(), device_info);
            }
        }

        #[unsafe(method(centralManager:didConnectPeripheral:))]
        fn central_manager_did_connect_peripheral(&self, _central: &CBCentralManager, peripheral: &CBPeripheral) {
            unsafe {
                let identifier = peripheral.identifier().UUIDString().to_string();
                let name = peripheral.name().map(|n| n.to_string()).unwrap_or("Unknown".to_string());
                
                println!("Connected to: {}", name);
                
                // Update device info
                if let Some(devices) = DISCOVERED_DEVICES.get() {
                    if let Ok(mut guard) = devices.lock() {
                        if let Some(device) = guard.get_mut(&identifier) {
                            device.is_connected = true;
                        }
                    }
                }
                
                // Discover services
                peripheral.discoverServices(None);
            }
        }

        #[unsafe(method(centralManager:didFailToConnectPeripheral:error:))]
        fn central_manager_did_fail_to_connect_peripheral(
            &self, _central: &CBCentralManager, peripheral: &CBPeripheral, error: Option<&NSError>
        ) {
            unsafe {
                let name = peripheral.name().map(|n| n.to_string()).unwrap_or("Unknown".to_string());
                println!("Failed to connect to {}: {:?}", name, error);
            }
        }

        #[unsafe(method(centralManager:didDisconnectPeripheral:error:))]
        fn central_manager_did_disconnect_peripheral(
            &self, _central: &CBCentralManager, peripheral: &CBPeripheral, error: Option<&NSError>
        ) {
            unsafe {
                let identifier = peripheral.identifier().UUIDString().to_string();
                let name = peripheral.name().map(|n| n.to_string()).unwrap_or("Unknown".to_string());
                
                if let Some(err) = error {
                    println!("Disconnected from {} with error: {:?}", name, err);
                } else {
                    println!("Disconnected from {}", name);
                }
                
                // Update device info
                if let Some(devices) = DISCOVERED_DEVICES.get() {
                    if let Ok(mut guard) = devices.lock() {
                        if let Some(device) = guard.get_mut(&identifier) {
                            device.is_connected = false;
                        }
                    }
                }
                
                // Remove from connected peripherals
                if let Some(peripherals) = CONNECTED_PERIPHERALS.get() {
                    if let Ok(mut guard) = peripherals.lock() {
                        guard.remove(&identifier);
                    }
                }
            }
        }
    }
);

impl MyCentralDelegate {
    fn new() -> Retained<Self> {
        unsafe { msg_send![Self::alloc(), init] }
    }
}

// Public API functions
pub fn create_central() {
    CENTRAL_INSTANCE.get_or_init(|| {
        let delegate = MyCentralDelegate::new();
        let manager = unsafe {
            CBCentralManager::initWithDelegate_queue(
                CBCentralManager::alloc(),
                Some(ProtocolObject::from_ref(&*delegate)),
                None
            )
        };
        println!("BluetoothCentral initialized");
        
        Mutex::new(Some(CentralWrapper { delegate, manager }))
    });
    
    CONNECTED_PERIPHERALS.get_or_init(|| Mutex::new(HashMap::new()));
}

pub fn create_peripheral() {
    PERIPHERAL_INSTANCE.get_or_init(|| {
        let delegate = MyPeripheralDelegate::new();
        let manager = unsafe {
            CBPeripheralManager::initWithDelegate_queue(
                CBPeripheralManager::alloc(),
                Some(ProtocolObject::from_ref(&*delegate)),
                None
            )
        };
        println!("BluetoothPeripheral initialized");
        
        Mutex::new(Some(PeripheralWrapper { delegate, manager }))
    });
}

pub fn start_central_scan() {
    if let Some(central_lock) = CENTRAL_INSTANCE.get() {
        if let Ok(guard) = central_lock.lock() {
            if let Some(ref central) = *guard {
                unsafe {
                    let options = NSDictionary::from_slices(
                        &[&*CBCentralManagerScanOptionAllowDuplicatesKey],
                        &[&*NSNumber::numberWithBool(true) as &AnyObject]
                    );
                    central.manager.scanForPeripheralsWithServices_options(None, Some(&options));
                    println!("Started scanning");
                }
            }
        }
    }
}

pub fn stop_central_scan() {
    if let Some(central_lock) = CENTRAL_INSTANCE.get() {
        if let Ok(guard) = central_lock.lock() {
            if let Some(ref central) = *guard {
                unsafe {
                    central.manager.stopScan();
                    println!("Stopped scanning");
                }
            }
        }
    }
}

// Connect to a device by its identifier (UUID string)
pub fn connect_to_device(identifier: &str) -> Result<(), String> {
    let peripherals = CONNECTED_PERIPHERALS.get_or_init(|| Mutex::new(HashMap::new()));
    
    // Check if already connected
    if let Ok(guard) = peripherals.lock() {
        if guard.contains_key(identifier) {
            return Err("Already connected to this device".to_string());
        }
    }
    
    // Find the peripheral from discovered devices
    if let Some(central_lock) = CENTRAL_INSTANCE.get() {
        if let Ok(central_guard) = central_lock.lock() {
            if let Some(central) = central_guard.as_ref() {
                unsafe {
                    // Retrieve peripherals with this identifier
                    let uuid = objc2_foundation::NSUUID::alloc();
                    let uuid = objc2_foundation::NSUUID::initWithUUIDString(uuid, &NSString::from_str(identifier));
                    
                    if let Some(uuid) = uuid {
                        // Create NSArray from a slice
                        let uuid_ref: &objc2_foundation::NSUUID = uuid.as_ref();
                        let retrieved = central.manager.retrievePeripheralsWithIdentifiers(
                            &NSArray::from_slice(&[uuid_ref])
                        );
                        
                        if retrieved.count() > 0 {
                            let peripheral_obj = retrieved.objectAtIndex(0);
                            if let Ok(peripheral) = peripheral_obj.downcast::<CBPeripheral>() {
                                // Set delegate
                                let delegate = MyPeripheralConnectionDelegate::new();
                                peripheral.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
                                
                                // Store the peripheral in thread-safe wrapper
                                if let Ok(mut guard) = peripherals.lock() {
                                    guard.insert(
                                        identifier.to_string(), 
                                        PeripheralHandle { peripheral: peripheral.retain() }
                                    );
                                }
                                
                                // Connect
                                central.manager.connectPeripheral_options(&peripheral, None);
                                println!("Connecting to device: {}", identifier);
                                return Ok(());
                            }
                        }
                    }
                    
                    return Err("Device not found".to_string());
                }
            }
        }
    }
    
    Err("Central manager not initialized".to_string())
}

// Disconnect from a device
pub fn disconnect_from_device(identifier: &str) -> Result<(), String> {
    let peripherals = CONNECTED_PERIPHERALS.get()
        .ok_or("No peripherals initialized")?;
    
    let handle = {
        let guard = peripherals.lock().map_err(|_| "Failed to lock peripherals")?;
        guard.get(identifier).map(|h| PeripheralHandle { peripheral: h.peripheral.retain() })
    };
    
    if let Some(handle) = handle {
        if let Some(central_lock) = CENTRAL_INSTANCE.get() {
            if let Ok(central_guard) = central_lock.lock() {
                if let Some(central) = central_guard.as_ref() {
                    unsafe {
                        central.manager.cancelPeripheralConnection(&handle.peripheral);
                        println!("Disconnecting from device: {}", identifier);
                        return Ok(());
                    }
                }
            }
        }
    }
    
    Err("Device not connected".to_string())
}

// Send text data to a characteristic
pub fn send_data_to_device(
    identifier: &str, 
    service_uuid: &str, 
    characteristic_uuid: &str, 
    text: &str
) -> Result<(), String> {
    let peripherals = CONNECTED_PERIPHERALS.get()
        .ok_or("No peripherals initialized")?;
    
    let handle = {
        let guard = peripherals.lock().map_err(|_| "Failed to lock peripherals")?;
        guard.get(identifier).map(|h| PeripheralHandle { peripheral: h.peripheral.retain() })
            .ok_or("Device not connected")?
    };
    
    unsafe {
        // Find the service
        if let Some(services) = handle.peripheral.services() {
            for i in 0..services.count() {
                let service_obj = services.objectAtIndex(i);
                if let Ok(service) = service_obj.downcast::<CBService>() {
                    if service.UUID().UUIDString().to_string() == service_uuid {
                        // Find the characteristic
                        if let Some(characteristics) = service.characteristics() {
                            for j in 0..characteristics.count() {
                                let char_obj = characteristics.objectAtIndex(j);
                                if let Ok(characteristic) = char_obj.downcast::<CBCharacteristic>() {
                                    if characteristic.UUID().UUIDString().to_string() == characteristic_uuid {
                                        // Check if characteristic supports writing
                                        let props = characteristic.properties();
                                        let can_write = props.contains(CBCharacteristicProperties::Write) ||
                                                       props.contains(CBCharacteristicProperties::WriteWithoutResponse);
                                        
                                        if !can_write {
                                            return Err("Characteristic does not support writing".to_string());
                                        }
                                        
                                        // Convert text to NSData
                                        let data_bytes = text.as_bytes();
                                        let ns_data = NSData::from_vec(data_bytes.to_vec());
                                        
                                        // Determine write type
                                        let write_type = if props.contains(CBCharacteristicProperties::WriteWithoutResponse) {
                                            CBCharacteristicWriteType::WithoutResponse
                                        } else {
                                            CBCharacteristicWriteType::WithResponse
                                        };
                                        
                                        // Write the data
                                        handle.peripheral.writeValue_forCharacteristic_type(
                                            &ns_data,
                                            &characteristic,
                                            write_type
                                        );
                                        
                                        println!("Sent '{}' to characteristic {}", text, characteristic_uuid);
                                        return Ok(());
                                    }
                                }
                            }
                        }
                        return Err("Characteristic not found".to_string());
                    }
                }
            }
        }
    }
    
    Err("Service not found".to_string())
}

// Convenience function to send "Hello World"
pub fn send_hello_world(identifier: &str, service_uuid: &str, characteristic_uuid: &str) -> Result<(), String> {
    send_data_to_device(identifier, service_uuid, characteristic_uuid, "Hello World")
}

































































// use std::sync::{Mutex, OnceLock};
// use std::collections::HashMap;
// use objc2::rc::Retained;
// use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
// use objc2::{define_class, msg_send};
// use objc2_foundation::NSString;
// use objc2_core_bluetooth::*;
// use objc2::AnyThread;

// #[derive(Clone, Debug)]
// pub struct DeviceInfo {
//     pub name: String,
//     pub rssi: i32,
//     pub is_connected: bool,
//     pub last_seen: std::time::SystemTime,
//     pub services: Vec<String>,
//     pub characteristics: HashMap<String, Vec<String>>,
//     pub manufacturer_data: Option<Vec<u8>>,
//     pub service_data: HashMap<String, Vec<u8>>,
//     pub tx_power_level: Option<i32>,
//     pub is_connectable: bool,
//     pub advertisement_data: HashMap<String, String>,
// }

// static DISCOVERED_DEVICES: OnceLock<Mutex<HashMap<String, DeviceInfo>>> = OnceLock::new();

// // Thread-safe wrappers for Objective-C objects
// struct CentralWrapper {
//     delegate: Retained<MyCentralDelegate>,
//     manager: Retained<CBCentralManager>,
// }

// struct PeripheralWrapper {
//     delegate: Retained<MyPeripheralDelegate>,
//     manager: Retained<CBPeripheralManager>,
// }

// // SAFETY: These are only accessed from the main thread via the hardware context
// unsafe impl Send for CentralWrapper {}
// unsafe impl Send for PeripheralWrapper {}

// // CRITICAL: Store the instances globally so they're never dropped
// static CENTRAL_INSTANCE: OnceLock<Mutex<Option<CentralWrapper>>> = OnceLock::new();
// static PERIPHERAL_INSTANCE: OnceLock<Mutex<Option<PeripheralWrapper>>> = OnceLock::new();

// // Peripheral Delegate
// define_class!(
//     #[unsafe(super(NSObject))]
//     #[name = "MyPeripheralDelegate"]
//     struct MyPeripheralDelegate;

//     unsafe impl NSObjectProtocol for MyPeripheralDelegate {}
//     unsafe impl CBPeripheralManagerDelegate for MyPeripheralDelegate {
//         #[unsafe(method(peripheralManagerDidUpdateState:))]
//         fn peripheral_manager_did_update_state(&self, peripheral: &CBPeripheralManager) {
//             unsafe {
//                 if peripheral.state() == CBManagerState::PoweredOn {
//                     println!("Peripheral powered on");
//                 } else {
//                     println!("Peripheral state: {:?}", peripheral.state());
//                 }
//             }
//         }
//     }
// );

// impl MyPeripheralDelegate {
//     fn new() -> Retained<Self> {
//         unsafe { msg_send![Self::alloc(), init] }
//     }
// }

// pub fn get_all_devices() -> Vec<DeviceInfo> {
//     DISCOVERED_DEVICES.get_or_init(|| Mutex::new(HashMap::new()))
//         .lock().map(|g| g.values().cloned().collect()).unwrap_or_default()
// }

// pub fn get_devices_in_range() -> Vec<DeviceInfo> {
//     const DEFAULT_SHARING_THRESHOLD: i32 = -70;
//     DISCOVERED_DEVICES.get_or_init(|| Mutex::new(HashMap::new()))
//         .lock()
//         .map(|g| {
//             g.values()
//                 .filter(|device| device.rssi > DEFAULT_SHARING_THRESHOLD)
//                 .cloned()
//                 .collect()
//         })
//         .unwrap_or_default()
// }

// pub fn cleanup_bluetooth() {
//     // Stop scanning first
//     if let Some(central_lock) = CENTRAL_INSTANCE.get() {
//         if let Ok(mut guard) = central_lock.lock() {
//             if let Some(central) = guard.as_ref() {
//                 unsafe {
//                     central.manager.stopScan();
//                 }
//             }
//             *guard = None;
//         }
//     }
    
//     // Clear peripheral
//     if let Some(peripheral_lock) = PERIPHERAL_INSTANCE.get() {
//         if let Ok(mut guard) = peripheral_lock.lock() {
//             *guard = None;
//         }
//     }
    
//     // Clear devices
//     if let Some(devices) = DISCOVERED_DEVICES.get() {
//         if let Ok(mut guard) = devices.lock() {
//             guard.clear();
//             println!("Cleared discovered devices");
//         }
//     }
// }

// // Central Manager - Scan Only (No Connection)
// use objc2::runtime::AnyObject;
// use objc2_foundation::{NSArray, NSDictionary, NSNumber};

// define_class!(
//     #[unsafe(super(NSObject))]
//     #[name = "MyCentralDelegate"]
//     struct MyCentralDelegate;

//     unsafe impl NSObjectProtocol for MyCentralDelegate {}
//     unsafe impl CBCentralManagerDelegate for MyCentralDelegate {
//         #[unsafe(method(centralManagerDidUpdateState:))]
//         fn central_manager_did_update_state(&self, central: &CBCentralManager) {
//             unsafe {
//                 match central.state() {
//                     CBManagerState::PoweredOn => {
//                         println!("Central powered on, starting scan");
//                         let options = NSDictionary::from_slices(
//                             &[&*CBCentralManagerScanOptionAllowDuplicatesKey],
//                             &[&*NSNumber::numberWithBool(true) as &AnyObject]
//                         );
//                         central.scanForPeripheralsWithServices_options(None, Some(&options));
//                     }
//                     CBManagerState::PoweredOff => println!("Bluetooth powered off"),
//                     CBManagerState::Unauthorized => println!("Bluetooth unauthorized"),
//                     state => println!("Central state: {:?}", state),
//                 }
//             }
//         }

//         #[unsafe(method(centralManager:didDiscoverPeripheral:advertisementData:RSSI:))]
//         fn central_manager_did_discover_peripheral_advertisement_data_rssi(
//             &self, _central: &CBCentralManager, peripheral: &CBPeripheral, 
//             advertisement_data: &NSDictionary, rssi: &NSNumber,
//         ) {
//             unsafe {
//                 let rssi_val = rssi.intValue();
//                 let identifier = peripheral.identifier().UUIDString().to_string();

//                 let mut device_name = "Unknown".to_string();
//                 if let Some(name) = peripheral.name() {
//                     device_name = name.to_string();
//                 } else if let Some(local_name) = advertisement_data.objectForKey(&*CBAdvertisementDataLocalNameKey) {
//                     if let Ok(name_str) = local_name.downcast::<NSString>() {
//                         device_name = name_str.to_string();
//                     }
//                 }

//                 let mut tx_power_level = None;
//                 let mut is_connectable = false;
//                 let mut advertisement_info = HashMap::new();

//                 if let Some(tx_power) = advertisement_data.objectForKey(&*CBAdvertisementDataTxPowerLevelKey) {
//                     if let Ok(power_num) = tx_power.downcast::<NSNumber>() {
//                         tx_power_level = Some(power_num.intValue());
//                         advertisement_info.insert("TxPowerLevel".to_string(), power_num.intValue().to_string());
//                     }
//                 }

//                 if let Some(connectable) = advertisement_data.objectForKey(&*CBAdvertisementDataIsConnectable) {
//                     if let Ok(conn_bool) = connectable.downcast::<NSNumber>() {
//                         is_connectable = conn_bool.boolValue();
//                         advertisement_info.insert("Connectable".to_string(), conn_bool.boolValue().to_string());
//                     }
//                 }

//                 let mut service_uuids = Vec::new();
//                 if let Some(services) = advertisement_data.objectForKey(&*CBAdvertisementDataServiceUUIDsKey) {
//                     if let Ok(service_array) = services.downcast::<NSArray>() {
//                         for i in 0..service_array.count() {
//                             let service_obj = service_array.objectAtIndex(i);
//                             if let Ok(service_uuid) = service_obj.downcast::<CBUUID>() {
//                                 service_uuids.push(service_uuid.UUIDString().to_string());
//                             }
//                         }
//                         advertisement_info.insert("AdvertisedServices".to_string(), service_uuids.join(", "));
//                     }
//                 }

//                 let device_info = DeviceInfo {
//                     name: device_name.clone(),
//                     rssi: rssi_val,
//                     is_connected: false,
//                     last_seen: std::time::SystemTime::now(),
//                     services: service_uuids,
//                     characteristics: HashMap::new(),
//                     manufacturer_data: None,
//                     service_data: HashMap::new(),
//                     tx_power_level,
//                     is_connectable,
//                     advertisement_data: advertisement_info,
//                 };

//                 DISCOVERED_DEVICES.get_or_init(|| Mutex::new(HashMap::new()))
//                     .lock().unwrap().insert(identifier.clone(), device_info);

//                 println!("Discovered: '{}' RSSI:{} Connectable:{}", device_name, rssi_val, is_connectable);
//             }
//         }
//     }
// );

// impl MyCentralDelegate {
//     fn new() -> Retained<Self> {
//         unsafe { msg_send![Self::alloc(), init] }
//     }
// }

// // Public API functions that store instances globally
// pub fn create_central() {
//     CENTRAL_INSTANCE.get_or_init(|| {
//         let delegate = MyCentralDelegate::new();
//         let manager = unsafe {
//             CBCentralManager::initWithDelegate_queue(
//                 CBCentralManager::alloc(),
//                 Some(ProtocolObject::from_ref(&*delegate)),
//                 None
//             )
//         };
//         println!("BluetoothCentral initialized");
        
//         Mutex::new(Some(CentralWrapper { delegate, manager }))
//     });
// }

// pub fn create_peripheral() {
//     PERIPHERAL_INSTANCE.get_or_init(|| {
//         let delegate = MyPeripheralDelegate::new();
//         let manager = unsafe {
//             CBPeripheralManager::initWithDelegate_queue(
//                 CBPeripheralManager::alloc(),
//                 Some(ProtocolObject::from_ref(&*delegate)),
//                 None
//             )
//         };
//         println!("BluetoothPeripheral initialized");
        
//         Mutex::new(Some(PeripheralWrapper { delegate, manager }))
//     });
// }

// pub fn start_central_scan() {
//     if let Some(central_lock) = CENTRAL_INSTANCE.get() {
//         if let Ok(guard) = central_lock.lock() {
//             if let Some(ref central) = *guard {
//                 unsafe {
//                     let options = NSDictionary::from_slices(
//                         &[&*CBCentralManagerScanOptionAllowDuplicatesKey],
//                         &[&*NSNumber::numberWithBool(true) as &AnyObject]
//                     );
//                     central.manager.scanForPeripheralsWithServices_options(None, Some(&options));
//                     println!("Started scanning");
//                 }
//             }
//         }
//     }
// }

// pub fn stop_central_scan() {
//     if let Some(central_lock) = CENTRAL_INSTANCE.get() {
//         if let Ok(guard) = central_lock.lock() {
//             if let Some(ref central) = *guard {
//                 unsafe {
//                     central.manager.stopScan();
//                     println!("Stopped scanning");
//                 }
//             }
//         }
//     }
// }