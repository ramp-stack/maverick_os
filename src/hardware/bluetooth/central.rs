// bluetooth/central.rs - macOS/iOS only

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

static SHOULD_SCAN: OnceLock<Mutex<bool>> = OnceLock::new();

pub(crate) struct PeripheralHandle {
    pub peripheral: Retained<CBPeripheral>,
    pub delegate: Retained<MyPeripheralConnectionDelegate>,
}

unsafe impl Send for PeripheralHandle {}
unsafe impl Sync for PeripheralHandle {}

pub(crate) static CONNECTED_PERIPHERALS: OnceLock<Mutex<HashMap<String, PeripheralHandle>>> = OnceLock::new();

struct CentralWrapper {
    delegate: Retained<MyCentralDelegate>,
    manager: Retained<CBCentralManager>,
}

unsafe impl Send for CentralWrapper {}

static CENTRAL_INSTANCE: OnceLock<Mutex<Option<CentralWrapper>>> = OnceLock::new();

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

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "MyPeripheralConnectionDelegate"]
    struct MyPeripheralConnectionDelegate;

    unsafe impl NSObjectProtocol for MyPeripheralConnectionDelegate {}
    unsafe impl CBPeripheralDelegate for MyPeripheralConnectionDelegate {
        #[unsafe(method(peripheral:didDiscoverServices:))]
        fn peripheral_did_discover_services(&self, peripheral: &CBPeripheral, error: Option<&objc2_foundation::NSError>) {
            unsafe {
                if let Some(err) = error {
                    println!("Error discovering services: {:?}", err);
                    return;
                }
                
                if let Some(services) = peripheral.services() {
                    let identifier = peripheral.identifier().UUIDString().to_string();
                    
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
                    
                    for i in 0..services.count() {
                        let service = services.objectAtIndex(i);
                        if let Ok(cb_service) = service.downcast::<CBService>() {
                            peripheral.discoverCharacteristics_forService(None, &cb_service);
                        }
                    }
                }
            }
        }

        #[unsafe(method(peripheral:didDiscoverCharacteristicsForService:error:))]
        fn peripheral_did_discover_characteristics_for_service(
            &self, peripheral: &CBPeripheral, service: &CBService, error: Option<&objc2_foundation::NSError>
        ) {
            unsafe {
                if let Some(err) = error {
                    println!("Error discovering characteristics: {:?}", err);
                    return;
                }
                
                let service_uuid = service.UUID().UUIDString().to_string();
                let identifier = peripheral.identifier().UUIDString().to_string();
                
                if let Some(characteristics) = service.characteristics() {
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
                    
                    if let Some(services) = peripheral.services() {
                        let mut all_discovered = true;
                        for i in 0..services.count() {
                            let svc_obj = services.objectAtIndex(i);
                            if let Ok(svc) = svc_obj.downcast::<CBService>() {
                                if svc.characteristics().is_none() {
                                    all_discovered = false;
                                    break;
                                }
                            }
                        }
                        
                        if all_discovered {
                            print_gatt_structure(&identifier, peripheral);
                        }
                    }
                }
            }
        }

        #[unsafe(method(peripheral:didWriteValueForCharacteristic:error:))]
        fn peripheral_did_write_value_for_characteristic(
            &self, _peripheral: &CBPeripheral, characteristic: &CBCharacteristic, error: Option<&objc2_foundation::NSError>
        ) {
            unsafe {
                if let Some(err) = error {
                    println!("Write failed: {:?}", err);
                } else {
                    println!("Write success: {}", characteristic.UUID().UUIDString());
                }
            }
        }

        #[unsafe(method(peripheral:didUpdateValueForCharacteristic:error:))]
        fn peripheral_did_update_value_for_characteristic(
            &self, _peripheral: &CBPeripheral, characteristic: &CBCharacteristic, error: Option<&objc2_foundation::NSError>
        ) {
            unsafe {
                if let Some(err) = error {
                    println!("Read failed: {:?}", err);
                } else if let Some(value) = characteristic.value() {
                    let len = value.len();
                    let bytes_ptr: *const std::ffi::c_void = msg_send![&*value, bytes];
                    let data = std::slice::from_raw_parts(bytes_ptr as *const u8, len);
                    println!("Read from {}: {:02X?}", characteristic.UUID().UUIDString(), data);
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
                        if let Some(should_scan) = SHOULD_SCAN.get() {
                            if let Ok(guard) = should_scan.lock() {
                                if *guard {
                                    let options = NSDictionary::from_slices(
                                        &[&*CBCentralManagerScanOptionAllowDuplicatesKey],
                                        &[&*NSNumber::numberWithBool(false) as &AnyObject]
                                    );
                                    central.scanForPeripheralsWithServices_options(None, Some(&options));
                                }
                            }
                        }
                    }
                    CBManagerState::PoweredOff => println!("BLE powered off"),
                    CBManagerState::Unauthorized => println!("BLE unauthorized"),
                    _ => {}
                }
            }
        }

        #[unsafe(method(centralManager:didDiscoverPeripheral:advertisementData:RSSI:))]
        fn central_manager_did_discover_peripheral_advertisement_data_rssi(
            &self, central: &CBCentralManager, peripheral: &CBPeripheral, 
            advertisement_data: &NSDictionary, rssi: &NSNumber,
        ) {
            unsafe {
                let rssi_val = rssi.intValue();
                let identifier = peripheral.identifier().UUIDString().to_string();
                
                let already_connected = CONNECTED_PERIPHERALS.get()
                    .and_then(|p| p.lock().ok())
                    .map(|guard| guard.contains_key(&identifier))
                    .unwrap_or(false);
                
                if already_connected {
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

                DISCOVERED_DEVICES.get_or_init(|| Mutex::new(HashMap::new()))
                    .lock().unwrap().insert(identifier.clone(), device_info);

                println!("Discovered: {} | {} | {} dBm", device_name, identifier, rssi_val);
                
                if is_connectable {
                    let delegate = MyPeripheralConnectionDelegate::new();
                    peripheral.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
                    
                    if let Some(peripherals) = CONNECTED_PERIPHERALS.get() {
                        if let Ok(mut guard) = peripherals.lock() {
                            guard.insert(
                                identifier.clone(),
                                PeripheralHandle {
                                    peripheral: peripheral.retain(),
                                    delegate,
                                }
                            );
                        }
                    }
                    
                    central.connectPeripheral_options(peripheral, None);
                }
            }
        }

        #[unsafe(method(centralManager:didConnectPeripheral:))]
        fn central_manager_did_connect_peripheral(&self, _central: &CBCentralManager, peripheral: &CBPeripheral) {
            unsafe {
                let identifier = peripheral.identifier().UUIDString().to_string();
                let name = peripheral.name().map(|n| n.to_string()).unwrap_or("Unknown".to_string());
                
                println!("Connected: {} | {}", name, identifier);
                
                if let Some(devices) = DISCOVERED_DEVICES.get() {
                    if let Ok(mut guard) = devices.lock() {
                        if let Some(device) = guard.get_mut(&identifier) {
                            device.is_connected = true;
                        }
                    }
                }
                
                peripheral.discoverServices(None);
            }
        }

        #[unsafe(method(centralManager:didFailToConnectPeripheral:error:))]
        fn central_manager_did_fail_to_connect_peripheral(
            &self, _central: &CBCentralManager, peripheral: &CBPeripheral, error: Option<&objc2_foundation::NSError>
        ) {
            unsafe {
                let identifier = peripheral.identifier().UUIDString().to_string();
                let name = peripheral.name().map(|n| n.to_string()).unwrap_or("Unknown".to_string());
                println!("Connection failed: {} | {:?}", name, error);
                
                if let Some(peripherals) = CONNECTED_PERIPHERALS.get() {
                    if let Ok(mut guard) = peripherals.lock() {
                        guard.remove(&identifier);
                    }
                }
            }
        }

        #[unsafe(method(centralManager:didDisconnectPeripheral:error:))]
        fn central_manager_did_disconnect_peripheral(
            &self, _central: &CBCentralManager, peripheral: &CBPeripheral, error: Option<&objc2_foundation::NSError>
        ) {
            unsafe {
                let identifier = peripheral.identifier().UUIDString().to_string();
                let name = peripheral.name().map(|n| n.to_string()).unwrap_or("Unknown".to_string());
                
                if let Some(err) = error {
                    println!("Disconnected: {} | {:?}", name, err);
                } else {
                    println!("Disconnected: {}", name);
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
            }
        }
    }
);

impl MyCentralDelegate {
    fn new() -> Retained<Self> {
        unsafe { msg_send![Self::alloc(), init] }
    }
}

pub fn create_central() {
    SHOULD_SCAN.get_or_init(|| Mutex::new(false));
    
    CENTRAL_INSTANCE.get_or_init(|| {
        let delegate = MyCentralDelegate::new();
        let manager = unsafe {
            CBCentralManager::initWithDelegate_queue(
                CBCentralManager::alloc(),
                Some(ProtocolObject::from_ref(&*delegate)),
                None
            )
        };
        Mutex::new(Some(CentralWrapper { delegate, manager }))
    });
    
    CONNECTED_PERIPHERALS.get_or_init(|| Mutex::new(HashMap::new()));
}

pub fn start_central_scan() {
    if let Some(should_scan) = SHOULD_SCAN.get() {
        if let Ok(mut guard) = should_scan.lock() {
            *guard = true;
        }
    }
    
    if let Some(central_lock) = CENTRAL_INSTANCE.get() {
        if let Ok(guard) = central_lock.lock() {
            if let Some(ref central) = *guard {
                unsafe {
                    if central.manager.state() == CBManagerState::PoweredOn {
                        let options = NSDictionary::from_slices(
                            &[&*CBCentralManagerScanOptionAllowDuplicatesKey],
                            &[&*NSNumber::numberWithBool(false) as &AnyObject]
                        );
                        central.manager.scanForPeripheralsWithServices_options(None, Some(&options));
                    }
                }
            }
        }
    }
}

pub fn stop_central_scan() {
    if let Some(should_scan) = SHOULD_SCAN.get() {
        if let Ok(mut guard) = should_scan.lock() {
            *guard = false;
        }
    }
    
    if let Some(central_lock) = CENTRAL_INSTANCE.get() {
        if let Ok(guard) = central_lock.lock() {
            if let Some(ref central) = *guard {
                unsafe {
                    central.manager.stopScan();
                }
            }
        }
    }
}

pub fn connect_to_device(identifier: &str) -> Result<(), String> {
    let peripherals = CONNECTED_PERIPHERALS.get_or_init(|| Mutex::new(HashMap::new()));
    
    if let Ok(guard) = peripherals.lock() {
        if guard.contains_key(identifier) {
            return Err("Already connected".to_string());
        }
    }
    
    if let Some(central_lock) = CENTRAL_INSTANCE.get() {
        if let Ok(central_guard) = central_lock.lock() {
            if let Some(central) = central_guard.as_ref() {
                unsafe {
                    let uuid = NSUUID::alloc();
                    let uuid = NSUUID::initWithUUIDString(uuid, &NSString::from_str(identifier));
                    
                    if let Some(uuid) = uuid {
                        let uuid_ref: &NSUUID = uuid.as_ref();
                        let retrieved = central.manager.retrievePeripheralsWithIdentifiers(
                            &NSArray::from_slice(&[uuid_ref])
                        );
                        
                        if retrieved.count() > 0 {
                            let peripheral_obj = retrieved.objectAtIndex(0);
                            if let Ok(peripheral) = peripheral_obj.downcast::<CBPeripheral>() {
                                let delegate = MyPeripheralConnectionDelegate::new();
                                peripheral.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
                                
                                if let Ok(mut guard) = peripherals.lock() {
                                    guard.insert(
                                        identifier.to_string(),
                                        PeripheralHandle {
                                            peripheral: peripheral.retain(),
                                            delegate,
                                        }
                                    );
                                }
                                
                                central.manager.connectPeripheral_options(&peripheral, None);
                                return Ok(());
                            }
                        }
                    }
                    
                    return Err("Device not found".to_string());
                }
            }
        }
    }
    
    Err("Central not initialized".to_string())
}

pub fn disconnect_from_device(identifier: &str) -> Result<(), String> {
    let peripherals = CONNECTED_PERIPHERALS.get()
        .ok_or("No peripherals")?;
    
    let handle = {
        let guard = peripherals.lock().map_err(|_| "Lock failed")?;
        guard.get(identifier).map(|h| PeripheralHandle {
            peripheral: h.peripheral.retain(),
            delegate: h.delegate.retain(),
        })
    };
    
    if let Some(handle) = handle {
        if let Some(central_lock) = CENTRAL_INSTANCE.get() {
            if let Ok(central_guard) = central_lock.lock() {
                if let Some(central) = central_guard.as_ref() {
                    unsafe {
                        central.manager.cancelPeripheralConnection(&handle.peripheral);
                        return Ok(());
                    }
                }
            }
        }
    }
    
    Err("Not connected".to_string())
}

pub fn send_data_to_device(
    identifier: &str,
    service_uuid: &str,
    characteristic_uuid: &str,
    text: &str
) -> Result<(), String> {
    let peripherals = CONNECTED_PERIPHERALS.get()
        .ok_or("No peripherals")?;
    
    let handle = {
        let guard = peripherals.lock().map_err(|_| "Lock failed")?;
        guard.get(identifier).map(|h| PeripheralHandle {
            peripheral: h.peripheral.retain(),
            delegate: h.delegate.retain(),
        })
        .ok_or("Not connected")?
    };
    
    unsafe {
        if let Some(services) = handle.peripheral.services() {
            for i in 0..services.count() {
                let service_obj = services.objectAtIndex(i);
                if let Ok(service) = service_obj.downcast::<CBService>() {
                    if service.UUID().UUIDString().to_string() == service_uuid {
                        if let Some(characteristics) = service.characteristics() {
                            for j in 0..characteristics.count() {
                                let char_obj = characteristics.objectAtIndex(j);
                                if let Ok(characteristic) = char_obj.downcast::<CBCharacteristic>() {
                                    if characteristic.UUID().UUIDString().to_string() == characteristic_uuid {
                                        let props = characteristic.properties();
                                        let can_write = props.contains(CBCharacteristicProperties::Write) ||
                                                       props.contains(CBCharacteristicProperties::WriteWithoutResponse);
                                        
                                        if !can_write {
                                            return Err("Characteristic not writable".to_string());
                                        }
                                        
                                        let data_bytes = text.as_bytes();
                                        let ns_data = objc2_foundation::NSData::from_vec(data_bytes.to_vec());
                                        
                                        let write_type = if props.contains(CBCharacteristicProperties::WriteWithoutResponse) {
                                            CBCharacteristicWriteType::WithoutResponse
                                        } else {
                                            CBCharacteristicWriteType::WithResponse
                                        };
                                        
                                        handle.peripheral.writeValue_forCharacteristic_type(
                                            &ns_data,
                                            &characteristic,
                                            write_type
                                        );
                                        
                                        println!("Sent '{}' to {}", text, characteristic_uuid);
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

pub fn send_hello_world(identifier: &str, service_uuid: &str, characteristic_uuid: &str) -> Result<(), String> {
    send_data_to_device(identifier, service_uuid, characteristic_uuid, "Hello World")
}

pub(crate) fn cleanup_central() {
    if let Some(should_scan) = SHOULD_SCAN.get() {
        if let Ok(mut guard) = should_scan.lock() {
            *guard = false;
        }
    }
    
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
    
    if let Some(peripherals) = CONNECTED_PERIPHERALS.get() {
        if let Ok(mut guard) = peripherals.lock() {
            guard.clear();
        }
    }
}