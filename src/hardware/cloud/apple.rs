use objc2_foundation::{NSString, NSAutoreleasePool};
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use objc2::rc::Retained;

#[derive(Debug, Clone)]
pub struct OsCloudStorage;

impl OsCloudStorage {
    pub fn new() -> Self {
        Self
    }

    pub fn save(&self, key: &str, value: &str) {
        unsafe {
            let _pool = NSAutoreleasePool::new();

            let store: *mut AnyObject = msg_send![class!(NSUbiquitousKeyValueStore), defaultStore];
            let ns_key: Retained<NSString> = NSString::from_str(key);
            let ns_value: Retained<NSString> = NSString::from_str(value);
            let _: () = msg_send![store, setString: &*ns_value, forKey: &*ns_key];
            let success: bool = msg_send![store, synchronize];

            if !success {
                panic!("Failed to synchronize with iCloud");
            }
        }
    }

    pub fn get(&self, key: &str) -> Option<String> {
        unsafe {
            let _pool = NSAutoreleasePool::new();

            let store: *mut AnyObject = msg_send![class!(NSUbiquitousKeyValueStore), defaultStore];
            let ns_key: Retained<NSString> = NSString::from_str(key);
            let ns_value: *mut NSString = msg_send![store, stringForKey: &*ns_key];
            if ns_value.is_null() {
                None
            } else {
                Some((*ns_value).to_string())
            }
        }
    }

    pub fn remove(&self, key: &str) {
        unsafe {
            let _pool = NSAutoreleasePool::new();

            let store: *mut AnyObject = msg_send![class!(NSUbiquitousKeyValueStore), defaultStore];
            let ns_key: Retained<NSString> = NSString::from_str(key);
            let _: () = msg_send![store, removeObjectForKey: &*ns_key];
            let success: bool = msg_send![store, synchronize];

            if !success {
                panic!("Failed to synchronize with iCloud");
            }
        }
    }

    pub fn clear(&self) {
        unsafe {
            let _pool = NSAutoreleasePool::new();

            let store: *mut AnyObject = msg_send![class!(NSUbiquitousKeyValueStore), defaultStore];
            let dict: *mut AnyObject = msg_send![store, dictionaryRepresentation];
            let keys: *mut AnyObject = msg_send![dict, allKeys];

            let count: usize = msg_send![keys, count];
            for i in 0..count {
                let key: *mut NSString = msg_send![keys, objectAtIndex: i];
                let _: () = msg_send![store, removeObjectForKey: key];
            }

            let success: bool = msg_send![store, synchronize];
            if !success {
                panic!("Failed to synchronize with iCloud");
            }
        }
    }
}

impl Default for OsCloudStorage {
    fn default() -> Self {
        OsCloudStorage
    }
}