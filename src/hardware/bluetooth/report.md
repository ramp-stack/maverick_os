# BOOP Report: Status – Unfinished

## Overview

This report summarizes the current state of the BLE peripheral implementation using Rust (`objc2` + CoreBluetooth) and the equivalent Swift process.  

- **macOS:** Both central and peripheral roles are fully functional.  
- **iOS:** Peripheral shows up as controllable but only advertises basic manufacturer data (time, device info, etc.). Custom advertisement data is not being broadcast.  

---

## Peripheral Implementation in Rust (`objc2` + CoreBluetooth)

### BLE Peripheral Process

1. **Initialize Peripheral**  
   - Create a single instance of the peripheral manager.  
   - Assign a delegate to handle Bluetooth events.  
   - Prepare storage for shared and custom data.  

2. **Handle Bluetooth State**  
   - Monitor the Bluetooth state.  
   - If `poweredOn`:  
     - Start any pending advertisement.  
   - If not `poweredOn`:  
     - Store advertisement data for later broadcast.  

3. **Start Advertising**  
   - Create a service with a unique UUID.  
   - Create characteristics for:  
     - Shared read/write data.  
     - Custom read-only data.  
   - Add the service to the peripheral manager.  
   - Begin broadcasting the advertisement.  

4. **Respond to Central Requests**  
   - **Read Requests:** Return the current value of the requested characteristic.  
   - **Write Requests:** Update shared data from central input.  
   - **Subscribe Requests:** Track which centrals are subscribed for notifications.  

5. **Update Data**  
   - Update shared or custom data dynamically.  
   - Notify subscribed centrals of changes if needed.  

6. **Stop Advertising**  
   - Stop broadcasting when no longer needed.  

7. **Cleanup**  
   - Release peripheral resources.  
   - Reset state for future use.  

---

## Peripheral Implementation in Swift

1. **Initialize Peripheral**  
   - Create a single instance of `CBPeripheralManager`.  
   - Assign a delegate to handle Bluetooth events.  
   - Prepare storage for shared and custom data (e.g., using `Data` or `String`).  

2. **Handle Bluetooth State**  
   - Monitor the Bluetooth state through delegate methods.  
   - If `poweredOn`:  
     - Start any pending advertisement.  
   - If not `poweredOn`:  
     - Store advertisement data for later broadcast.  

3. **Start Advertising**  
   - Create a `CBMutableService` with a unique UUID.  
   - Create `CBMutableCharacteristics` for:  
     - Shared read/write data.  
     - Custom read-only data.  
   - Add the service to the peripheral manager.  
   - Begin broadcasting advertisement data with `startAdvertising`.  

4. **Respond to Central Requests**  
   - **Read Requests:** Return the current value of the requested characteristic.  
   - **Write Requests:** Update shared data from central input.  
   - **Subscribe Requests:** Track which centrals are subscribed for notifications.  

5. **Update Data**  
   - Update shared or custom data dynamically.  
   - Notify subscribed centrals with `updateValue(_:for:onSubscribedCentrals:)`.  

6. **Stop Advertising**  
   - Stop broadcasting using `stopAdvertising`.  

7. **Cleanup**  
   - Remove services if needed.  
   - Release references to delegate and manager.  

---

## Observations

- Both Rust and Swift implementations follow the same fundamental process.  
- macOS works fully for both central and peripheral roles.  
- iOS currently only advertises default manufacturer data and does not broadcast custom data.  

---

## Next Steps

- Implement the peripheral in Swift on iOS and verify proper advertisement of custom data.  
- Compare behavior to the Rust implementation to identify differences.  
- Ensure all CoreBluetooth frameworks and background modes are correctly configured in Xcode.  
