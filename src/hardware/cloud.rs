#[cfg(any(target_os = "macos", target_os = "ios"))]
use objc2_foundation::{NSString, NSAutoreleasePool};
#[cfg(any(target_os = "macos", target_os = "ios"))]
use objc2::runtime::AnyObject;
#[cfg(any(target_os = "macos", target_os = "ios"))]
use objc2::{class, msg_send};
#[cfg(any(target_os = "macos", target_os = "ios"))]
use objc2::rc::Retained;

#[cfg(target_os = "android")]
use jni::objects::{JObject, JString, JValue, GlobalRef};
#[cfg(target_os = "android")]
use jni::{JNIEnv, JavaVM};
#[cfg(target_os = "android")]
use std::sync::{Mutex, OnceLock};
#[cfg(target_os = "android")]
use std::ffi::{CStr, c_char};

#[cfg(target_os = "android")]
static JAVA_VM: OnceLock<JavaVM> = OnceLock::new();
#[cfg(target_os = "android")]
static APP_CONTEXT: OnceLock<Mutex<Option<GlobalRef>>> = OnceLock::new();

// Cross platform cloud key value storage.

//System:
//<iOS/macOS>: Uses NSUbiquitousKeyValueStore which is iCloud.

//<Android>: Uses SharedPreferences.

//<Linux, macOS, Windows>: no operation methods

// Method Save(key, value) stores a string value aka TEXT under a key.

// Method Get(key) gets the value that is owned by that key.

// Method Remove(key) deletes the said key thats passed in.

// Method Clear() deletes all keys.

#[derive(Debug, Clone)]
pub struct CloudStorage;

impl CloudStorage {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn save(key: &str, value: &str) {
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

    #[cfg(target_os = "android")]
    pub fn save(key: &str, value: &str) {
        let instance = Self;
        instance.save_with_context(key, value);
    }

    #[cfg(target_os = "android")]
    fn save_with_context(&self, key: &str, value: &str) {
        let vm = JAVA_VM.get().expect("JavaVM not initialized");
        let mut env = vm.attach_current_thread()
            .expect("Failed to attach thread");

        let context = self.get_or_create_application_context(&mut env);

        let prefs_name = env.new_string("CloudStoragePrefs")
            .expect("Failed to create prefs name");

        let shared_prefs = env.call_method(
            &context,
            "getSharedPreferences",
            "(Ljava/lang/String;I)Landroid/content/SharedPreferences;",
            &[JValue::Object(&prefs_name), JValue::Int(0)]
        ).expect("Failed to get SharedPreferences")
            .l().expect("SharedPreferences is null");

        let editor = env.call_method(
            &shared_prefs,
            "edit",
            "()Landroid/content/SharedPreferences$Editor;",
            &[]
        ).expect("Failed to get editor")
            .l().expect("Editor is null");

        let j_key = env.new_string(key)
            .expect("Failed to create key string");
        let j_value = env.new_string(value)
            .expect("Failed to create value string");

        let _ = env.call_method(
            &editor,
            "putString",
            "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/SharedPreferences$Editor;",
            &[JValue::Object(&j_key), JValue::Object(&j_value)]
        ).expect("Failed to put string");

        let _ = env.call_method(
            &editor,
            "apply",
            "()V",
            &[]
        ).expect("Failed to apply changes");
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn get(key: &str) -> Option<String> {
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

    #[cfg(target_os = "android")]
    pub fn get(key: &str) -> Option<String> {
        let instance = Self;
        instance.get_with_context(key)
    }

    #[cfg(target_os = "android")]
    fn get_with_context(&self, key: &str) -> Option<String> {
        let vm = JAVA_VM.get().expect("JavaVM not initialized");
        let mut env = vm.attach_current_thread()
            .expect("Failed to attach thread");

        let context = self.get_or_create_application_context(&mut env);

        let prefs_name = env.new_string("CloudStoragePrefs")
            .expect("Failed to create prefs name");

        let shared_prefs = env.call_method(
            &context,
            "getSharedPreferences",
            "(Ljava/lang/String;I)Landroid/content/SharedPreferences;",
            &[JValue::Object(&prefs_name), JValue::Int(0)]
        ).expect("Failed to get SharedPreferences")
            .l().expect("SharedPreferences is null");

        let j_key = env.new_string(key)
            .expect("Failed to create key string");

        let result = env.call_method(
            &shared_prefs,
            "getString",
            "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
            &[JValue::Object(&j_key), JValue::Object(&JObject::null())]
        ).expect("Failed to get string");

        match result.l() {
            Ok(obj) if !obj.is_null() => {
                let j_string = JString::from(obj);
                let rust_string: String = env.get_string(&j_string)
                    .expect("Failed to convert JString")
                    .into();
                Some(rust_string)
            }
            _ => None
        }
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn remove(key: &str) {
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

    #[cfg(target_os = "android")]
    pub fn remove(key: &str) {
        let instance = Self;
        instance.remove_with_context(key);
    }

    #[cfg(target_os = "android")]
    fn remove_with_context(&self, key: &str) {
        let vm = JAVA_VM.get().expect("JavaVM not initialized");
        let mut env = vm.attach_current_thread()
            .expect("Failed to attach thread");

        let context = self.get_or_create_application_context(&mut env);

        let prefs_name = env.new_string("CloudStoragePrefs")
            .expect("Failed to create prefs name");

        let shared_prefs = env.call_method(
            &context,
            "getSharedPreferences",
            "(Ljava/lang/String;I)Landroid/content/SharedPreferences;",
            &[JValue::Object(&prefs_name), JValue::Int(0)]
        ).expect("Failed to get SharedPreferences")
            .l().expect("SharedPreferences is null");

        let editor = env.call_method(
            &shared_prefs,
            "edit",
            "()Landroid/content/SharedPreferences$Editor;",
            &[]
        ).expect("Failed to get editor")
            .l().expect("Editor is null");

        let j_key = env.new_string(key)
            .expect("Failed to create key string");

        let _ = env.call_method(
            &editor,
            "remove",
            "(Ljava/lang/String;)Landroid/content/SharedPreferences$Editor;",
            &[JValue::Object(&j_key)]
        ).expect("Failed to remove key");

        let _ = env.call_method(
            &editor,
            "apply",
            "()V",
            &[]
        ).expect("Failed to apply changes");
    }

    /// Clear all stored data
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn clear() {
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

    #[cfg(target_os = "android")]
    pub fn clear() {
        let instance = Self;
        instance.clear_with_context();
    }

    #[cfg(target_os = "android")]
    fn clear_with_context(&self) {
        let vm = JAVA_VM.get().expect("JavaVM not initialized");
        let mut env = vm.attach_current_thread()
            .expect("Failed to attach thread");

        let context = self.get_or_create_application_context(&mut env);

        let prefs_name = env.new_string("CloudStoragePrefs")
            .expect("Failed to create prefs name");

        let shared_prefs = env.call_method(
            &context,
            "getSharedPreferences",
            "(Ljava/lang/String;I)Landroid/content/SharedPreferences;",
            &[JValue::Object(&prefs_name), JValue::Int(0)]
        ).expect("Failed to get SharedPreferences")
            .l().expect("SharedPreferences is null");

        let editor = env.call_method(
            &shared_prefs,
            "edit",
            "()Landroid/content/SharedPreferences$Editor;",
            &[]
        ).expect("Failed to get editor")
            .l().expect("Editor is null");

        let _ = env.call_method(
            &editor,
            "clear",
            "()Landroid/content/SharedPreferences$Editor;",
            &[]
        ).expect("Failed to clear");

        let _ = env.call_method(
            &editor,
            "apply",
            "()V",
            &[]
        ).expect("Failed to apply changes");
    }

    /// Get or create Android application context as instance method
    #[cfg(target_os = "android")]
    fn get_or_create_application_context<'a>(&self, env: &mut JNIEnv<'a>) -> JObject<'a> {
        if let Some(context_mutex) = APP_CONTEXT.get() {
            if let Ok(context_guard) = context_mutex.lock() {
                if let Some(context_ref) = context_guard.as_ref() {
                    return env.new_local_ref(context_ref.as_obj())
                        .expect("Failed to create local ref from global");
                }
            }
        }

        // If not found, create new context
        let activity_thread_class = env.find_class("android/app/ActivityThread")
            .expect("Failed to find ActivityThread class");

        let activity_thread = env.call_static_method(
            activity_thread_class,
            "currentActivityThread",
            "()Landroid/app/ActivityThread;",
            &[]
        ).expect("Failed to get current ActivityThread")
            .l().expect("ActivityThread is null");

        let context = env.call_method(
            &activity_thread,
            "getApplication",
            "()Landroid/app/Application;",
            &[]
        ).expect("Failed to get application")
            .l().expect("Application context is null");

        // Create global reference and store it for future use
        let global_context = env.new_global_ref(&context)
            .expect("Failed to create global ref");

        // Store the global reference
        if APP_CONTEXT.get().is_none() {
            APP_CONTEXT.set(Mutex::new(Some(global_context)))
                .expect("Failed to set APP_CONTEXT");
        } else {
            if let Some(context_mutex) = APP_CONTEXT.get() {
                if let Ok(mut context_guard) = context_mutex.lock() {
                    *context_guard = Some(global_context);
                }
            }
        }

        // Return the local reference we already have
        context
    }

    #[cfg(target_os = "android")]
    pub fn init_java_vm(vm: JavaVM) {
        JAVA_VM.set(vm).expect("JavaVM already initialized");
    }

    // Stub implementations for other platforms
    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
    pub fn save(_key: &str, _value: &str) {
        panic!("CloudStorage::save not implemented for this platform");
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
    pub fn get(_key: &str) -> Option<String> {
        panic!("CloudStorage::get not implemented for this platform");
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
    pub fn remove(_key: &str) {
        panic!("CloudStorage::remove not implemented for this platform");
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
    pub fn clear() {
        panic!("CloudStorage::clear not implemented for this platform");
    }
}

impl Default for CloudStorage {
    fn default() -> Self {
        CloudStorage
    }
}

// JNI initialization
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn JNI_OnLoad(vm: JavaVM, _: *mut std::ffi::c_void) -> jni::sys::jint {
    CloudStorage::init_java_vm(vm);
    jni::sys::JNI_VERSION_1_6.into()
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "C" fn cloud_storage_save(key: *const i8, value: *const i8) -> i32 {
    if key.is_null() || value.is_null() {
        return -1;
    }

    let key_str = unsafe {
        match CStr::from_ptr(key as *const c_char).to_str() {
            Ok(s) => s,
            Err(_) => return -1,
        }
    };

    let value_str = unsafe {
        match CStr::from_ptr(value as *const c_char).to_str() {
            Ok(s) => s,
            Err(_) => return -1,
        }
    };

    CloudStorage::save(key_str, value_str);
    0
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "C" fn cloud_storage_get(key: *const i8, buffer: *mut i8, buffer_size: usize) -> i32 {
    if key.is_null() || buffer.is_null() {
        return -1;
    }

    let key_str = unsafe {
        match CStr::from_ptr(key as *const c_char).to_str() {
            Ok(s) => s,
            Err(_) => return -1,
        }
    };

    match CloudStorage::get(key_str) {
        Some(value) => {
            let value_bytes = value.as_bytes();
            if value_bytes.len() + 1 > buffer_size {
                return -2; // Buffer too small
            }

            unsafe {
                std::ptr::copy_nonoverlapping(value_bytes.as_ptr(), buffer as *mut u8, value_bytes.len());
                *buffer.add(value_bytes.len()) = 0; // Null terminator
            }

            value_bytes.len() as i32
        }
        None => 0, // Key not found
    }
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "C" fn cloud_storage_remove(key: *const i8) -> i32 {
    if key.is_null() {
        return -1;
    }

    let key_str = unsafe {
        match CStr::from_ptr(key as *const c_char).to_str() {
            Ok(s) => s,
            Err(_) => return -1,
        }
    };

    CloudStorage::remove(key_str);
    0
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "C" fn cloud_storage_clear() -> i32 {
    CloudStorage::clear();
    0
}