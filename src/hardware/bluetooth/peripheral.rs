
#![cfg(any(target_os = "macos", target_os = "ios"))]

use std::sync::{Mutex, OnceLock};
use objc2::rc::Retained;
use objc2::runtime::{NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send};
use objc2_foundation::{NSObject, NSString, NSData, NSArray, NSDictionary};
use objc2_core_bluetooth::*;
use objc2::runtime::AnyObject;
use objc2::Message;
use objc2::AnyThread;

static PERIPHERAL: OnceLock<Mutex<Option<PeripheralWrapper>>> = OnceLock::new();

// Transfer Service UUIDs - matching Swift TransferService
const SERVICE_UUID: &str = "E20A39F4-73F5-4BC4-A12F-17D1AD07A961";
const CHARACTERISTIC_UUID: &str = "08590F7E-DB05-467E-8757-72F6FAEB13D4";

struct PeripheralWrapper {
    delegate: Retained<MyPeripheralDelegate>,
    manager: Retained<CBPeripheralManager>,
    transfer_characteristic: Mutex<Option<Retained<CBMutableCharacteristic>>>,
    connected_central: Mutex<Option<Retained<CBCentral>>>,
    data_to_send: Mutex<Vec<u8>>,
    send_data_index: Mutex<usize>,
    sending_eom: Mutex<bool>,
    advertising: Mutex<bool>,
}

unsafe impl Send for PeripheralWrapper {}
unsafe impl Sync for PeripheralWrapper {}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "MyPeripheralDelegate"]
    struct MyPeripheralDelegate;

    unsafe impl NSObjectProtocol for MyPeripheralDelegate {}
    unsafe impl CBPeripheralManagerDelegate for MyPeripheralDelegate {
        // Required protocol method - waiting for CBPeripheralManager to be ready
        #[unsafe(method(peripheralManagerDidUpdateState:))]
        fn peripheral_manager_did_update_state(&self, peripheral: &CBPeripheralManager) {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                unsafe {
                    let state = peripheral.state();
                    println!("[PERIPHERAL] CBManager state: {:?}", state);
                    
                    match state {
                        CBManagerState::PoweredOn => {
                            println!("[PERIPHERAL] CBManager is powered on");
                            setup_peripheral();
                        }
                        CBManagerState::PoweredOff => {
                            println!("[PERIPHERAL] CBManager is not powered on");
                        }
                        CBManagerState::Resetting => {
                            println!("[PERIPHERAL] CBManager is resetting");
                        }
                        CBManagerState::Unauthorized => {
                            println!("[PERIPHERAL] Bluetooth is not authorized");
                        }
                        CBManagerState::Unknown => {
                            println!("[PERIPHERAL] CBManager state is unknown");
                        }
                        CBManagerState::Unsupported => {
                            println!("[PERIPHERAL] Bluetooth is not supported on this device");
                        }
                        _ => {
                            println!("[PERIPHERAL] Unknown peripheral manager state");
                        }
                    }
                }
            }));
        }

        // Catch when someone subscribes to our characteristic, then start sending them data
        #[unsafe(method(peripheralManager:central:didSubscribeToCharacteristic:))]
        fn peripheral_manager_central_did_subscribe_to_characteristic(
            &self, 
            _peripheral: &CBPeripheralManager, 
            central: &CBCentral, 
            _characteristic: &CBCharacteristic
        ) {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                println!("[PERIPHERAL] Central subscribed to characteristic");
                
                // Save central
                set_connected_central(Some(central.retain()));
                
                // Reset the index
                reset_send_index();
                
                // Start sending
                send_data();
            }));
        }

        // Recognize when the central unsubscribes
        #[unsafe(method(peripheralManager:central:didUnsubscribeFromCharacteristic:))]
        fn peripheral_manager_central_did_unsubscribe_from_characteristic(
            &self, 
            _peripheral: &CBPeripheralManager, 
            _central: &CBCentral, 
            _characteristic: &CBCharacteristic
        ) {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                println!("[PERIPHERAL] Central unsubscribed from characteristic");
                set_connected_central(None);
            }));
        }

        // This callback comes in when the PeripheralManager is ready to send the next chunk of data
        // This ensures packets arrive in the order they are sent
        #[unsafe(method(peripheralManagerIsReadyToUpdateSubscribers:))]
        fn peripheral_manager_is_ready_to_update_subscribers(&self, _peripheral: &CBPeripheralManager) {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                // Start sending again
                send_data();
            }));
        }

        // This callback comes in when the PeripheralManager received write to characteristics
        #[unsafe(method(peripheralManager:didReceiveWriteRequests:))]
        fn peripheral_manager_did_receive_write_requests(
            &self, 
            peripheral: &CBPeripheralManager, 
            requests: &NSArray
        ) {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                unsafe {
                    println!("[PERIPHERAL] didReceiveWriteRequests called with {} requests", requests.count());
                    
                    // Process all write requests
                    for i in 0..requests.count() {
                        if let Some(request) = requests.objectAtIndex(i).downcast::<CBATTRequest>().ok() {
                            if let Some(request_value) = request.value() {
                                let bytes = request_value.as_bytes_unchecked();
                                if let Ok(string_from_data) = std::str::from_utf8(bytes) {
                                    println!("[PERIPHERAL] Received write request of {} bytes: {}", 
                                        bytes.len(), string_from_data);
                                    
                                    // Update the data to send with received data AND reset index
                                    set_text_data_and_reset(bytes);
                                }
                            }
                        }
                    }
                    
                    // Respond to the first write request (if any exist)
                    if requests.count() > 0 {
                        if let Some(request) = requests.objectAtIndex(0).downcast::<CBATTRequest>().ok() {
                            peripheral.respondToRequest_withResult(&request, CBATTError::Success);
                        } else {
                            eprintln!("[PERIPHERAL] Failed to downcast request for response");
                        }
                    }
                }
            }));
        }

        // Handle service add completion
        #[unsafe(method(peripheralManager:didAddService:error:))]
        fn peripheral_manager_did_add_service(
            &self,
            _peripheral: &CBPeripheralManager,
            service: &CBService,
            error: *mut NSObject
        ) {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                unsafe {
                    if error.is_null() {
                        println!("[PERIPHERAL] Service added successfully");
                    } else {
                        eprintln!("[PERIPHERAL] Error adding service: {:?}", error);
                    }
                }
            }));
        }
    }
);

impl MyPeripheralDelegate {
    fn new() -> Retained<Self> {
        unsafe { msg_send![Self::alloc(), init] }
    }
}

// MARK: - Helper Methods

// Sends the next amount of data to the connected central
fn send_data() {
    let Some(peripheral) = PERIPHERAL.get() else { return };
    let Ok(guard) = peripheral.lock() else { return };
    let Some(wrapper) = guard.as_ref() else { return };

    // Get the transfer characteristic first
    let transfer_characteristic = {
        let Ok(transfer_char_lock) = wrapper.transfer_characteristic.lock() else { return };
        match transfer_char_lock.as_ref() {
            Some(tc) => tc.clone(),
            None => return,
        }
    };

    unsafe {
        // First up, check if we're meant to be sending an EOM
        let sending_eom_flag = {
            let Ok(sending_eom) = wrapper.sending_eom.lock() else { return };
            *sending_eom
        };
        
        if sending_eom_flag {
            // send it
            let eom_data = NSData::from_vec(b"EOM".to_vec());
            let did_send = wrapper.manager.updateValue_forCharacteristic_onSubscribedCentrals(
                &eom_data,
                &transfer_characteristic,
                None
            );
            
            // Did it send?
            if did_send {
                // It did, so mark it as sent
                if let Ok(mut sending_eom) = wrapper.sending_eom.lock() {
                    *sending_eom = false;
                }
                println!("[PERIPHERAL] Sent: EOM");
            }
            // It didn't send, so we'll exit and wait for peripheralManagerIsReady to call sendData again
            return;
        }

        // We're not sending an EOM, so we're sending data
        // Prepare chunk while holding locks, then release before sending
        loop {
            let (chunk_data, chunk_len, is_last_chunk) = {
                let Ok(data_to_send) = wrapper.data_to_send.lock() else { return };
                let Ok(mut send_data_index) = wrapper.send_data_index.lock() else { return };

                // Is there any left to send?
                if *send_data_index >= data_to_send.len() {
                    // No data left. Do nothing
                    return;
                }

                // Work out how big it should be
                let mut amount_to_send = data_to_send.len() - *send_data_index;
                
                if let Ok(central_lock) = wrapper.connected_central.lock() {
                    if let Some(central) = central_lock.as_ref() {
                        let mtu = central.maximumUpdateValueLength();
                        amount_to_send = amount_to_send.min(mtu);
                    }
                }

                // Copy out the data we want
                let chunk = &data_to_send[*send_data_index..(*send_data_index + amount_to_send)];
                let chunk_vec = chunk.to_vec();
                let chunk_len = chunk_vec.len();
                
                // Update index while we have the lock
                *send_data_index += amount_to_send;
                let is_last = *send_data_index >= data_to_send.len();
                
                (NSData::from_vec(chunk_vec), chunk_len, is_last)
            }; // Locks released here

            // Send it (no locks held)
            let did_send = wrapper.manager.updateValue_forCharacteristic_onSubscribedCentrals(
                &chunk_data,
                &transfer_characteristic,
                None
            );

            // If it didn't work, we need to roll back the index and wait for the callback
            if !did_send {
                // Roll back the send_data_index
                if let Ok(mut send_data_index) = wrapper.send_data_index.lock() {
                    *send_data_index = send_data_index.saturating_sub(chunk_len);
                }
                return;
            }

            let string_from_data = String::from_utf8_lossy(chunk_data.as_bytes_unchecked());
            println!("[PERIPHERAL] Sent {} bytes: {}", chunk_len, string_from_data);

            // Was it the last one?
            if is_last_chunk {
                // It was - send an EOM
                
                // Set this so if the send fails, we'll send it next time
                if let Ok(mut sending_eom) = wrapper.sending_eom.lock() {
                    *sending_eom = true;
                }

                // Send it
                let eom_data = NSData::from_vec(b"EOM".to_vec());
                let eom_sent = wrapper.manager.updateValue_forCharacteristic_onSubscribedCentrals(
                    &eom_data,
                    &transfer_characteristic,
                    None
                );

                if eom_sent {
                    // It sent; we're all done
                    if let Ok(mut sending_eom) = wrapper.sending_eom.lock() {
                        *sending_eom = false;
                    }
                    println!("[PERIPHERAL] Sent: EOM");
                }
                return;
            }
        }
    }
}

fn setup_peripheral() {
    let Some(peripheral) = PERIPHERAL.get() else { 
        eprintln!("[PERIPHERAL] Failed to get PERIPHERAL");
        return;
    };
    let Ok(guard) = peripheral.lock() else { 
        eprintln!("[PERIPHERAL] Failed to lock PERIPHERAL");
        return;
    };
    let Some(wrapper) = guard.as_ref() else { 
        eprintln!("[PERIPHERAL] PERIPHERAL wrapper is None");
        return;
    };

    unsafe {
        // Build our service
        
        // Start with the CBMutableCharacteristic
        // FIXED: Use Write (with response) instead of WriteWithoutResponse
        // Note: Notify/Indicate characteristics cannot have an initial value
        let char_uuid = CBUUID::UUIDWithString(&NSString::from_str(CHARACTERISTIC_UUID));
        let transfer_characteristic = CBMutableCharacteristic::initWithType_properties_value_permissions(
            CBMutableCharacteristic::alloc(),
            &char_uuid,
            CBCharacteristicProperties::Read | CBCharacteristicProperties::Notify | CBCharacteristicProperties::Write,
            None,
            CBAttributePermissions::Readable | CBAttributePermissions::Writeable
        );

        // Create a service from the characteristic
        let service_uuid = CBUUID::UUIDWithString(&NSString::from_str(SERVICE_UUID));
        let transfer_service = CBMutableService::initWithType_primary(
            CBMutableService::alloc(),
            &service_uuid,
            true
        );

        // Add the characteristic to the service
        transfer_service.setCharacteristics(Some(&NSArray::from_slice(&[
            &*transfer_characteristic as &CBCharacteristic
        ])));

        // And add it to the peripheral manager
        wrapper.manager.addService(&transfer_service);

        // Save the characteristic for later
        if let Ok(mut char_lock) = wrapper.transfer_characteristic.lock() {
            *char_lock = Some(transfer_characteristic);
            println!("[PERIPHERAL] Service and characteristic added");
        } else {
            eprintln!("[PERIPHERAL] Failed to lock transfer_characteristic");
        }
    }
}

// MARK: - Helper functions for safe data access

fn set_connected_central(central: Option<Retained<CBCentral>>) {
    if let Some(peripheral) = PERIPHERAL.get() {
        if let Ok(guard) = peripheral.lock() {
            if let Some(wrapper) = guard.as_ref() {
                if let Ok(mut central_lock) = wrapper.connected_central.lock() {
                    *central_lock = central;
                }
            }
        }
    }
}

fn reset_send_index() {
    if let Some(peripheral) = PERIPHERAL.get() {
        if let Ok(guard) = peripheral.lock() {
            if let Some(wrapper) = guard.as_ref() {
                if let Ok(mut index_lock) = wrapper.send_data_index.lock() {
                    *index_lock = 0;
                }
                if let Ok(mut eom_lock) = wrapper.sending_eom.lock() {
                    *eom_lock = false;
                }
            }
        }
    }
}

fn set_text_data(data: &[u8]) {
    if let Some(peripheral) = PERIPHERAL.get() {
        if let Ok(guard) = peripheral.lock() {
            if let Some(wrapper) = guard.as_ref() {
                if let Ok(mut data_lock) = wrapper.data_to_send.lock() {
                    *data_lock = data.to_vec();
                }
            }
        }
    }
}

// FIXED: New function that sets data AND resets index
fn set_text_data_and_reset(data: &[u8]) {
    if let Some(peripheral) = PERIPHERAL.get() {
        if let Ok(guard) = peripheral.lock() {
            if let Some(wrapper) = guard.as_ref() {
                // Set the data
                if let Ok(mut data_lock) = wrapper.data_to_send.lock() {
                    *data_lock = data.to_vec();
                }
                // Reset the index
                if let Ok(mut index_lock) = wrapper.send_data_index.lock() {
                    *index_lock = 0;
                }
                // Reset EOM flag
                if let Ok(mut eom_lock) = wrapper.sending_eom.lock() {
                    *eom_lock = false;
                }
                println!("[PERIPHERAL] Data set and index reset: {} bytes", data.len());
            }
        }
    }
}

// MARK: - Public API

pub fn create_peripheral() {
    println!("[PERIPHERAL] Creating peripheral manager");
    
    // Initialize the peripheral wrapper
    let _ = PERIPHERAL.get_or_init(|| {
        unsafe {
            // Create delegate first
            let delegate = MyPeripheralDelegate::new();
            
            // Create peripheral manager with nil delegate initially
            let manager = CBPeripheralManager::initWithDelegate_queue_options(
                CBPeripheralManager::alloc(),
                None, // Start with no delegate to avoid early callbacks
                None,
                None // Remove the options that might cause issues
            );
            
            // Now set the delegate after manager is created
            manager.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            
            println!("[PERIPHERAL] Peripheral manager created successfully");

            Mutex::new(Some(PeripheralWrapper {
                delegate,
                manager,
                transfer_characteristic: Mutex::new(None),
                connected_central: Mutex::new(None),
                data_to_send: Mutex::new(b"Hello from Rust!".to_vec()),
                send_data_index: Mutex::new(0),
                sending_eom: Mutex::new(false),
                advertising: Mutex::new(false),
            }))
        }
    });
    
    println!("[PERIPHERAL] Peripheral initialization complete");
}

// Start advertising - advertises service UUID only
pub fn start_advertising() -> Result<(), String> {
    println!("[PERIPHERAL] Starting advertising");
    
    let peripheral = PERIPHERAL.get()
        .ok_or("Peripheral not initialized")?;
    
    let guard = peripheral.lock()
        .map_err(|_| "Failed to lock peripheral")?;
    
    let wrapper = guard.as_ref()
        .ok_or("Peripheral not initialized")?;

    unsafe {
        let state = wrapper.manager.state();
        
        if state != CBManagerState::PoweredOn {
            return Err(format!("Bluetooth is not powered on (state: {:?})", state));
        }

        let service_uuid = CBUUID::UUIDWithString(&NSString::from_str(SERVICE_UUID));
        let adv_data = NSDictionary::from_slices(
            &[&*CBAdvertisementDataServiceUUIDsKey],
            &[&*NSArray::from_slice(&[&*service_uuid]) as &AnyObject]
        );

        wrapper.manager.startAdvertising(Some(&adv_data));
        
        if let Ok(mut advertising) = wrapper.advertising.lock() {
            *advertising = true;
        }
        
        println!("[PERIPHERAL] Advertising started with service UUID");
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
                if let Ok(mut advertising) = wrapper.advertising.lock() {
                    *advertising = false;
                }
                println!("[PERIPHERAL] Advertising stopped");
            }
        }
    }
}

// Set the text data to be sent when a central subscribes
pub fn set_text_to_send(text: &str) {
    println!("[PERIPHERAL] Setting text data: {} bytes", text.len());
    set_text_data_and_reset(text.as_bytes());
}

pub fn is_advertising() -> bool {
    PERIPHERAL.get()
        .and_then(|p| p.lock().ok())
        .and_then(|g| g.as_ref().and_then(|w| w.advertising.lock().ok().map(|a| *a)))
        .unwrap_or(false)
}

pub fn cleanup_peripheral() {
    println!("[PERIPHERAL] Cleanup");
    
    if let Some(peripheral) = PERIPHERAL.get() {
        if let Ok(mut guard) = peripheral.lock() {
            if let Some(wrapper) = guard.as_ref() {
                unsafe {
                    wrapper.manager.stopAdvertising();
                }
            }
            *guard = None;
        }
    }
}
