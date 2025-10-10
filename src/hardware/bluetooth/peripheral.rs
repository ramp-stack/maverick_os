// bluetooth/peripheral.rs - macOS/iOS only

#![cfg(any(target_os = "macos", target_os = "ios"))]

use std::sync::{Mutex, OnceLock};
use objc2::rc::Retained;
use objc2::runtime::{NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send};
use objc2_foundation::{NSObject, NSString, NSData, NSArray, NSDictionary};
use objc2_core_bluetooth::*;
use objc2::runtime::AnyObject;
use objc2::AnyThread;

static PERIPHERAL: OnceLock<Mutex<Option<PeripheralWrapper>>> = OnceLock::new();

#[derive(Clone)]
struct PendingAdvertisement {
    service_uuid: String,
    local_name: String,
    custom_data: Vec<u8>,
}

struct PeripheralWrapper {
    delegate: Retained<MyPeripheralDelegate>,
    manager: Retained<CBPeripheralManager>,
    shared_data: Mutex<Vec<u8>>,
    custom_data: Mutex<Vec<u8>>,
    pending_ad: Mutex<Option<PendingAdvertisement>>,
    custom_characteristic: Mutex<Option<Retained<CBMutableCharacteristic>>>,
}

unsafe impl Send for PeripheralWrapper {}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "MyPeripheralDelegate"]
    struct MyPeripheralDelegate;

    unsafe impl NSObjectProtocol for MyPeripheralDelegate {}
    unsafe impl CBPeripheralManagerDelegate for MyPeripheralDelegate {
        #[unsafe(method(peripheralManagerDidUpdateState:))]
        fn peripheral_manager_did_update_state(&self, peripheral: &CBPeripheralManager) {
            unsafe {
                let state = peripheral.state();
                println!("[PERIPHERAL] State: {:?}", state);
                
                if state == CBManagerState::PoweredOn {
                    // Check if we have a pending advertisement to start
                    if let Some(ad) = get_pending_advertisement() {
                        println!("[PERIPHERAL] Starting pending ad: {}", ad.local_name);
                        start_advertising_internal(&ad.service_uuid, &ad.local_name, &ad.custom_data);
                    }
                }
            }
        }

        #[unsafe(method(peripheralManagerDidStartAdvertising:error:))]
        fn peripheral_manager_did_start_advertising(
            &self, 
            _peripheral: &CBPeripheralManager, 
            error: Option<&objc2_foundation::NSError>
        ) {
            if let Some(err) = error {
                println!("[PERIPHERAL] Advertising failed: {:?}", err);
            } else {
                println!("[PERIPHERAL] Advertising started");
            }
        }

        #[unsafe(method(peripheralManager:didAddService:error:))]
        fn peripheral_manager_did_add_service(
            &self, 
            peripheral: &CBPeripheralManager, 
            _service: &CBService, 
            error: Option<&objc2_foundation::NSError>
        ) {
            if let Some(err) = error {
                println!("[PERIPHERAL] Failed to add service: {:?}", err);
                return;
            }
            
            println!("[PERIPHERAL] Service added, starting broadcast");
            
            if let Some(ad) = get_pending_advertisement() {
                unsafe {
                    let uuid = CBUUID::UUIDWithString(&NSString::from_str(&ad.service_uuid));
                    let adv_data = NSDictionary::from_slices(
                        &[&*CBAdvertisementDataServiceUUIDsKey, &*CBAdvertisementDataLocalNameKey],
                        &[
                            &*NSArray::from_slice(&[&*uuid]) as &AnyObject, 
                            &*NSString::from_str(&ad.local_name) as &AnyObject
                        ]
                    );
                    
                    peripheral.startAdvertising(Some(&adv_data));
                }
            }
        }

        #[unsafe(method(peripheralManager:didReceiveReadRequest:))]
        fn peripheral_manager_did_receive_read_request(
            &self, 
            peripheral: &CBPeripheralManager, 
            request: &CBATTRequest
        ) {
            unsafe {
                let char_uuid = request.characteristic().UUID().UUIDString().to_string();
                
                let data = match char_uuid.as_str() {
                    "12345678-1234-1234-1234-123456789ABC" => get_shared_data(),
                    "12345678-1234-1234-1234-123456789ABD" => get_custom_data(),
                    _ => Vec::new()
                };

                println!("[PERIPHERAL] Read request for {}: {} bytes", char_uuid, data.len());
                request.setValue(Some(&NSData::from_vec(data)));
                peripheral.respondToRequest_withResult(request, CBATTError::Success);
            }
        }

        #[unsafe(method(peripheralManager:didReceiveWriteRequests:))]
        fn peripheral_manager_did_receive_write_requests(
            &self, 
            peripheral: &CBPeripheralManager, 
            requests: &NSArray
        ) {
            unsafe {
                println!("[PERIPHERAL] Write request: {} requests", requests.count());
                
                for i in 0..requests.count() {
                    if let Ok(request) = requests.objectAtIndex(i).downcast::<CBATTRequest>() {
                        if let Some(data) = request.value() {
                            let bytes = data.as_bytes_unchecked().to_vec();
                            println!("[PERIPHERAL] Writing {} bytes", bytes.len());
                            set_shared_data(&bytes);
                        }
                    }
                }

                // Respond to first request
                if requests.count() > 0 {
                    if let Ok(request) = requests.objectAtIndex(0).downcast::<CBATTRequest>() {
                        peripheral.respondToRequest_withResult(&request, CBATTError::Success);
                    }
                }
            }
        }

        #[unsafe(method(peripheralManager:central:didSubscribeToCharacteristic:))]
        fn peripheral_manager_central_did_subscribe_to_characteristic(
            &self, 
            _peripheral: &CBPeripheralManager, 
            central: &CBCentral, 
            characteristic: &CBCharacteristic
        ) {
            unsafe {
                println!("[PERIPHERAL] Central {} subscribed to {}", 
                    central.identifier().UUIDString(), 
                    characteristic.UUID().UUIDString()
                );
            }
        }
    }
);

impl MyPeripheralDelegate {
    fn new() -> Retained<Self> {
        unsafe { msg_send![Self::alloc(), init] }
    }
}

// Helper functions to safely access wrapper data
fn get_pending_advertisement() -> Option<PendingAdvertisement> {
    PERIPHERAL.get()?
        .lock().ok()?
        .as_ref()?
        .pending_ad.lock().ok()?
        .clone()
}

fn get_shared_data() -> Vec<u8> {
    PERIPHERAL.get()
        .and_then(|p| p.lock().ok())
        .and_then(|g| g.as_ref().map(|w| w.shared_data.lock().unwrap().clone()))
        .unwrap_or_default()
}

fn get_custom_data() -> Vec<u8> {
    PERIPHERAL.get()
        .and_then(|p| p.lock().ok())
        .and_then(|g| g.as_ref().map(|w| w.custom_data.lock().unwrap().clone()))
        .unwrap_or_default()
}

fn set_shared_data(data: &[u8]) {
    if let Some(peripheral) = PERIPHERAL.get() {
        if let Ok(guard) = peripheral.lock() {
            if let Some(wrapper) = guard.as_ref() {
                *wrapper.shared_data.lock().unwrap() = data.to_vec();
            }
        }
    }
}

fn start_advertising_internal(service_uuid: &str, local_name: &str, custom_data: &[u8]) {
    println!("[PERIPHERAL] Setting up service: {}", local_name);
    
    let Some(peripheral) = PERIPHERAL.get() else { return };
    let Ok(guard) = peripheral.lock() else { return };
    let Some(wrapper) = guard.as_ref() else { return };

    unsafe {
        if wrapper.manager.state() != CBManagerState::PoweredOn {
            println!("[PERIPHERAL] Cannot advertise - not powered on");
            return;
        }

        // Store custom data
        *wrapper.custom_data.lock().unwrap() = custom_data.to_vec();

        // Create service
        let uuid = CBUUID::UUIDWithString(&NSString::from_str(service_uuid));
        let service = CBMutableService::initWithType_primary(
            CBMutableService::alloc(),
            &uuid,
            true
        );

        // Main characteristic (read/write/notify)
        let char_uuid = CBUUID::UUIDWithString(&NSString::from_str("12345678-1234-1234-1234-123456789ABC"));
        let characteristic = CBMutableCharacteristic::initWithType_properties_value_permissions(
            CBMutableCharacteristic::alloc(),
            &char_uuid,
            CBCharacteristicProperties::Read | CBCharacteristicProperties::Write | CBCharacteristicProperties::Notify,
            None,
            CBAttributePermissions::Readable | CBAttributePermissions::Writeable
        );
        
        // Initialize shared data if empty
        if wrapper.shared_data.lock().unwrap().is_empty() {
            *wrapper.shared_data.lock().unwrap() = b"Ready".to_vec();
        }

        // Custom data characteristic (read-only)
        let custom_char_uuid = CBUUID::UUIDWithString(&NSString::from_str("12345678-1234-1234-1234-123456789ABD"));
        let custom_data_value = if !custom_data.is_empty() {
            NSData::from_vec(custom_data.to_vec())
        } else {
            NSData::from_vec(b"No custom data".to_vec())
        };

        let custom_characteristic = CBMutableCharacteristic::initWithType_properties_value_permissions(
            CBMutableCharacteristic::alloc(),
            &custom_char_uuid,
            CBCharacteristicProperties::Read,
            Some(&custom_data_value),
            CBAttributePermissions::Readable
        );

        // Store custom characteristic for later updates
        *wrapper.custom_characteristic.lock().unwrap() = Some(custom_characteristic.clone());

        // Add both characteristics to service
        service.setCharacteristics(Some(&NSArray::from_slice(&[
            &*characteristic as &CBCharacteristic,
            &*custom_characteristic as &CBCharacteristic
        ])));
        
        // Store pending advertisement
        *wrapper.pending_ad.lock().unwrap() = Some(PendingAdvertisement {
            service_uuid: service_uuid.to_string(),
            local_name: local_name.to_string(),
            custom_data: custom_data.to_vec(),
        });
        
        // Add service - advertising will start in didAddService callback
        println!("[PERIPHERAL] Adding service");
        wrapper.manager.addService(&service);
    }
}

fn generate_random_uuid() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    
    format!(
        "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
        (nanos & 0xFFFFFFFF) as u32,
        ((nanos >> 32) & 0xFFFF) as u16,
        ((nanos >> 48) & 0xFFF) as u16,
        ((nanos >> 60) & 0xFFFF) as u16 | 0x8000,
        ((nanos >> 76) & 0xFFFFFFFFFFFF) as u64
    )
}

// Public API

pub fn create_peripheral() {
    println!("[PERIPHERAL] Creating peripheral");
    
    PERIPHERAL.get_or_init(|| {
        let delegate = MyPeripheralDelegate::new();
        let manager = unsafe {
            CBPeripheralManager::initWithDelegate_queue(
                CBPeripheralManager::alloc(),
                Some(ProtocolObject::from_ref(&*delegate)),
                None
            )
        };

        Mutex::new(Some(PeripheralWrapper { 
            delegate, 
            manager, 
            shared_data: Mutex::new(Vec::new()),
            custom_data: Mutex::new(Vec::new()),
            pending_ad: Mutex::new(None),
            custom_characteristic: Mutex::new(None),
        }))
    });
}

pub fn advertise(local_name: &str, data: &str) -> Result<(), String> {
    let service_uuid = generate_random_uuid();
    println!("[PERIPHERAL] Advertise: {} (UUID: {})", local_name, service_uuid);
    
    create_peripheral();
    start_advertising_with_data(&service_uuid, local_name, data.as_bytes())
}

pub fn start_advertising(service_uuid: &str, local_name: &str) -> Result<(), String> {
    start_advertising_with_data(service_uuid, local_name, &[])
}

pub fn start_advertising_with_data<T: AsRef<[u8]>>(
    service_uuid: &str, 
    local_name: &str, 
    custom_data: T
) -> Result<(), String> {
    let custom_data_bytes = custom_data.as_ref();
    println!("[PERIPHERAL] Starting: {}", local_name);
    
    let peripheral = PERIPHERAL.get()
        .ok_or("Peripheral not initialized")?;
    
    let guard = peripheral.lock()
        .map_err(|_| "Failed to lock peripheral")?;
    
    let wrapper = guard.as_ref()
        .ok_or("Peripheral not initialized")?;

    unsafe {
        let state = wrapper.manager.state();
        
        if state == CBManagerState::PoweredOn {
            drop(guard);
            start_advertising_internal(service_uuid, local_name, custom_data_bytes);
        } else {
            // Store pending advertisement to start when powered on
            *wrapper.pending_ad.lock().unwrap() = Some(PendingAdvertisement {
                service_uuid: service_uuid.to_string(),
                local_name: local_name.to_string(),
                custom_data: custom_data_bytes.to_vec(),
            });
            println!("[PERIPHERAL] Waiting for Bluetooth to power on");
        }
    }
    
    Ok(())
}

pub fn stop_advertising() {
    println!("[PERIPHERAL] Stopping advertising");
    
    if let Some(peripheral) = PERIPHERAL.get() {
        if let Ok(guard) = peripheral.lock() {
            if let Some(wrapper) = guard.as_ref() {
                unsafe { 
                    wrapper.manager.stopAdvertising();
                }
            }
        }
    }
}

pub fn set_shareable_string(text: &str) {
    set_shared_data(text.as_bytes());
}

pub fn update_custom_data(custom_data: &[u8]) {
    println!("[PERIPHERAL] Updating custom data: {} bytes", custom_data.len());
    
    if let Some(peripheral) = PERIPHERAL.get() {
        if let Ok(guard) = peripheral.lock() {
            if let Some(wrapper) = guard.as_ref() {
                *wrapper.custom_data.lock().unwrap() = custom_data.to_vec();
                
                // Notify subscribed centrals if characteristic exists
                if let Some(characteristic) = wrapper.custom_characteristic.lock().unwrap().as_ref() {
                    unsafe {
                        let data = NSData::from_vec(custom_data.to_vec());
                        let success = wrapper.manager.updateValue_forCharacteristic_onSubscribedCentrals(
                            &data,
                            characteristic,
                            None
                        );
                        println!("[PERIPHERAL] Notification sent: {}", if success { "success" } else { "failed" });
                    }
                }
            }
        }
    }
}

pub(crate) fn cleanup_peripheral() {
    println!("[PERIPHERAL] Cleanup");
    
    if let Some(peripheral) = PERIPHERAL.get() {
        if let Ok(mut guard) = peripheral.lock() {
            *guard = None;
        }
    }
}