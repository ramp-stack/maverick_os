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

/// Cloud storage for key–value data.
#[derive(Debug, Clone)]
pub struct CloudStorage;

#[cfg(target_os = "android")]
#[derive(Debug)]
pub enum CloudStorageError {
    JniError(String),
    ContextNotFound,
    JavaVmNotInitialized,
}

#[cfg(target_os = "android")]
impl std::fmt::Display for CloudStorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CloudStorageError::JniError(msg) => write!(f, "JNI Error: {}", msg),
            CloudStorageError::ContextNotFound => write!(f, "Android context not found"),
            CloudStorageError::JavaVmNotInitialized => write!(f, "JavaVM not initialized"),
        }
    }
}

impl CloudStorage {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn save(key: &str, value: &str) -> Result<(), String> {
        unsafe {
            let _pool = NSAutoreleasePool::new();

            let store: *mut AnyObject = msg_send![class!(NSUbiquitousKeyValueStore), defaultStore];
            let ns_key: Retained<NSString> = NSString::from_str(key);
            let ns_value: Retained<NSString> = NSString::from_str(value);
            let _: () = msg_send![store, setString: &*ns_value, forKey: &*ns_key];
            let success: bool = msg_send![store, synchronize];

            if success {
                Ok(())
            } else {
                Err("Failed to synchronize with iCloud".to_string())
            }
        }
    }

    #[cfg(target_os = "android")]
    pub fn save(key: &str, value: &str) -> Result<(), CloudStorageError> {
        let instance = Self;
        instance.save_with_context(key, value)
    }

    #[cfg(target_os = "android")]
    fn save_with_context(&self, key: &str, value: &str) -> Result<(), CloudStorageError> {
        let vm = JAVA_VM.get().ok_or(CloudStorageError::JavaVmNotInitialized)?;
        let mut env = vm.attach_current_thread()
            .map_err(|e| CloudStorageError::JniError(format!("Failed to attach thread: {}", e)))?;

        let context = self.get_or_create_application_context(&mut env)?;

        let prefs_name = env.new_string("CloudStoragePrefs")
            .map_err(|e| CloudStorageError::JniError(format!("Failed to create prefs name: {}", e)))?;

        let shared_prefs = env.call_method(
            &context,
            "getSharedPreferences",
            "(Ljava/lang/String;I)Landroid/content/SharedPreferences;",
            &[JValue::Object(&prefs_name), JValue::Int(0)]
        ).map_err(|e| CloudStorageError::JniError(format!("Failed to get SharedPreferences: {}", e)))?
            .l().map_err(|e| CloudStorageError::JniError(format!("SharedPreferences is null: {}", e)))?;

        let editor = env.call_method(
            &shared_prefs,
            "edit",
            "()Landroid/content/SharedPreferences$Editor;",
            &[]
        ).map_err(|e| CloudStorageError::JniError(format!("Failed to get editor: {}", e)))?
            .l().map_err(|e| CloudStorageError::JniError(format!("Editor is null: {}", e)))?;

        let j_key = env.new_string(key)
            .map_err(|e| CloudStorageError::JniError(format!("Failed to create key string: {}", e)))?;
        let j_value = env.new_string(value)
            .map_err(|e| CloudStorageError::JniError(format!("Failed to create value string: {}", e)))?;

        let _ = env.call_method(
            &editor,
            "putString",
            "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/SharedPreferences$Editor;",
            &[JValue::Object(&j_key), JValue::Object(&j_value)]
        ).map_err(|e| CloudStorageError::JniError(format!("Failed to put string: {}", e)))?;

        let _ = env.call_method(
            &editor,
            "apply",
            "()V",
            &[]
        ).map_err(|e| CloudStorageError::JniError(format!("Failed to apply changes: {}", e)))?;

        Ok(())
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn get(key: &str) -> Result<Option<String>, String> {
        unsafe {
            let _pool = NSAutoreleasePool::new();

            let store: *mut AnyObject = msg_send![class!(NSUbiquitousKeyValueStore), defaultStore];
            let ns_key: Retained<NSString> = NSString::from_str(key);
            let ns_value: *mut NSString = msg_send![store, stringForKey: &*ns_key];
            if ns_value.is_null() {
                Ok(None)
            } else {
                Ok(Some((*ns_value).to_string()))
            }
        }
    }

    #[cfg(target_os = "android")]
    pub fn get(key: &str) -> Result<Option<String>, CloudStorageError> {
        let instance = Self;
        instance.get_with_context(key)
    }

    #[cfg(target_os = "android")]
    fn get_with_context(&self, key: &str) -> Result<Option<String>, CloudStorageError> {
        let vm = JAVA_VM.get().ok_or(CloudStorageError::JavaVmNotInitialized)?;
        let mut env = vm.attach_current_thread()
            .map_err(|e| CloudStorageError::JniError(format!("Failed to attach thread: {}", e)))?;

        // Get or create the application context
        let context = self.get_or_create_application_context(&mut env)?;

        let prefs_name = env.new_string("CloudStoragePrefs")
            .map_err(|e| CloudStorageError::JniError(format!("Failed to create prefs name: {}", e)))?;

        let shared_prefs = env.call_method(
            &context,
            "getSharedPreferences",
            "(Ljava/lang/String;I)Landroid/content/SharedPreferences;",
            &[JValue::Object(&prefs_name), JValue::Int(0)]
        ).map_err(|e| CloudStorageError::JniError(format!("Failed to get SharedPreferences: {}", e)))?
            .l().map_err(|e| CloudStorageError::JniError(format!("SharedPreferences is null: {}", e)))?;

        let j_key = env.new_string(key)
            .map_err(|e| CloudStorageError::JniError(format!("Failed to create key string: {}", e)))?;

        let result = env.call_method(
            &shared_prefs,
            "getString",
            "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
            &[JValue::Object(&j_key), JValue::Object(&JObject::null())]
        ).map_err(|e| CloudStorageError::JniError(format!("Failed to get string: {}", e)))?;

        match result.l() {
            Ok(obj) if !obj.is_null() => {
                let j_string = JString::from(obj);
                let rust_string: String = env.get_string(&j_string)
                    .map_err(|e| CloudStorageError::JniError(format!("Failed to convert JString: {}", e)))?
                    .into();
                Ok(Some(rust_string))
            }
            _ => Ok(None)
        }
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn remove(key: &str) -> Result<(), String> {
        unsafe {
            let _pool = NSAutoreleasePool::new();

            let store: *mut AnyObject = msg_send![class!(NSUbiquitousKeyValueStore), defaultStore];
            let ns_key: Retained<NSString> = NSString::from_str(key);
            let _: () = msg_send![store, removeObjectForKey: &*ns_key];
            let success: bool = msg_send![store, synchronize];

            if success {
                Ok(())
            } else {
                Err("Failed to synchronize with iCloud".to_string())
            }
        }
    }

    #[cfg(target_os = "android")]
    pub fn remove(key: &str) -> Result<(), CloudStorageError> {
        let instance = Self;
        instance.remove_with_context(key)
    }

    #[cfg(target_os = "android")]
    fn remove_with_context(&self, key: &str) -> Result<(), CloudStorageError> {
        let vm = JAVA_VM.get().ok_or(CloudStorageError::JavaVmNotInitialized)?;
        let mut env = vm.attach_current_thread()
            .map_err(|e| CloudStorageError::JniError(format!("Failed to attach thread: {}", e)))?;

        // Get or create the application context
        let context = self.get_or_create_application_context(&mut env)?;

        let prefs_name = env.new_string("CloudStoragePrefs")
            .map_err(|e| CloudStorageError::JniError(format!("Failed to create prefs name: {}", e)))?;

        let shared_prefs = env.call_method(
            &context,
            "getSharedPreferences",
            "(Ljava/lang/String;I)Landroid/content/SharedPreferences;",
            &[JValue::Object(&prefs_name), JValue::Int(0)]
        ).map_err(|e| CloudStorageError::JniError(format!("Failed to get SharedPreferences: {}", e)))?
            .l().map_err(|e| CloudStorageError::JniError(format!("SharedPreferences is null: {}", e)))?;

        let editor = env.call_method(
            &shared_prefs,
            "edit",
            "()Landroid/content/SharedPreferences$Editor;",
            &[]
        ).map_err(|e| CloudStorageError::JniError(format!("Failed to get editor: {}", e)))?
            .l().map_err(|e| CloudStorageError::JniError(format!("Editor is null: {}", e)))?;

        let j_key = env.new_string(key)
            .map_err(|e| CloudStorageError::JniError(format!("Failed to create key string: {}", e)))?;

        let _ = env.call_method(
            &editor,
            "remove",
            "(Ljava/lang/String;)Landroid/content/SharedPreferences$Editor;",
            &[JValue::Object(&j_key)]
        ).map_err(|e| CloudStorageError::JniError(format!("Failed to remove key: {}", e)))?;

        let _ = env.call_method(
            &editor,
            "apply",
            "()V",
            &[]
        ).map_err(|e| CloudStorageError::JniError(format!("Failed to apply changes: {}", e)))?;

        Ok(())
    }

    /// Clear all stored data
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn clear() -> Result<(), String> {
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
            if success {
                Ok(())
            } else {
                Err("Failed to synchronize with iCloud".to_string())
            }
        }
    }

    #[cfg(target_os = "android")]
    pub fn clear() -> Result<(), CloudStorageError> {
        let instance = Self;
        instance.clear_with_context()
    }

    #[cfg(target_os = "android")]
    fn clear_with_context(&self) -> Result<(), CloudStorageError> {
        let vm = JAVA_VM.get().ok_or(CloudStorageError::JavaVmNotInitialized)?;
        let mut env = vm.attach_current_thread()
            .map_err(|e| CloudStorageError::JniError(format!("Failed to attach thread: {}", e)))?;

        // Get or create the application context
        let context = self.get_or_create_application_context(&mut env)?;

        let prefs_name = env.new_string("CloudStoragePrefs")
            .map_err(|e| CloudStorageError::JniError(format!("Failed to create prefs name: {}", e)))?;

        let shared_prefs = env.call_method(
            &context,
            "getSharedPreferences",
            "(Ljava/lang/String;I)Landroid/content/SharedPreferences;",
            &[JValue::Object(&prefs_name), JValue::Int(0)]
        ).map_err(|e| CloudStorageError::JniError(format!("Failed to get SharedPreferences: {}", e)))?
            .l().map_err(|e| CloudStorageError::JniError(format!("SharedPreferences is null: {}", e)))?;

        let editor = env.call_method(
            &shared_prefs,
            "edit",
            "()Landroid/content/SharedPreferences$Editor;",
            &[]
        ).map_err(|e| CloudStorageError::JniError(format!("Failed to get editor: {}", e)))?
            .l().map_err(|e| CloudStorageError::JniError(format!("Editor is null: {}", e)))?;

        let _ = env.call_method(
            &editor,
            "clear",
            "()Landroid/content/SharedPreferences$Editor;",
            &[]
        ).map_err(|e| CloudStorageError::JniError(format!("Failed to clear: {}", e)))?;

        let _ = env.call_method(
            &editor,
            "apply",
            "()V",
            &[]
        ).map_err(|e| CloudStorageError::JniError(format!("Failed to apply changes: {}", e)))?;

        Ok(())
    }

    /// Get or create Android application context as instance method
    #[cfg(target_os = "android")]
    fn get_or_create_application_context<'a>(&self, env: &mut JNIEnv<'a>) -> Result<JObject<'a>, CloudStorageError> {
        if let Some(context_mutex) = APP_CONTEXT.get() {
            if let Ok(context_guard) = context_mutex.lock() {
                if let Some(context_ref) = context_guard.as_ref() {
                    // Create a new local reference from the global reference
                    return env.new_local_ref(context_ref.as_obj())
                        .map_err(|e| CloudStorageError::JniError(format!("Failed to create local ref from global: {}", e)));
                }
            }
        }

        // If not found, create new context
        let activity_thread_class = env.find_class("android/app/ActivityThread")
            .map_err(|e| CloudStorageError::JniError(format!("Failed to find ActivityThread class: {}", e)))?;

        let activity_thread = env.call_static_method(
            activity_thread_class,
            "currentActivityThread",
            "()Landroid/app/ActivityThread;",
            &[]
        ).map_err(|e| CloudStorageError::JniError(format!("Failed to get current ActivityThread: {}", e)))?
            .l().map_err(|e| CloudStorageError::JniError(format!("ActivityThread is null: {}", e)))?;

        let context = env.call_method(
            &activity_thread,
            "getApplication",
            "()Landroid/app/Application;",
            &[]
        ).map_err(|e| CloudStorageError::JniError(format!("Failed to get application: {}", e)))?
            .l().map_err(|e| CloudStorageError::JniError(format!("Application context is null: {}", e)))?;

        // Create global reference and store it for future use
        let global_context = env.new_global_ref(&context)
            .map_err(|e| CloudStorageError::JniError(format!("Failed to create global ref: {}", e)))?;

        // Store the global reference
        if APP_CONTEXT.get().is_none() {
            let _ = APP_CONTEXT.set(Mutex::new(Some(global_context)));
        } else {
            if let Some(context_mutex) = APP_CONTEXT.get() {
                if let Ok(mut context_guard) = context_mutex.lock() {
                    *context_guard = Some(global_context);
                }
            }
        }

        // Return the local reference we already have
        Ok(context)
    }

    #[cfg(target_os = "android")]
    pub fn init_java_vm(vm: JavaVM) -> Result<(), CloudStorageError> {
        JAVA_VM.set(vm).map_err(|_| CloudStorageError::JniError("JavaVM already initialized".to_string()))?;
        Ok(())
    }

    // Stub implementations for other platforms
    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
    pub fn save(_key: &str, _value: &str) -> Result<(), String> {
        Err("CloudStorage::save not implemented for this platform".to_string())
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
    pub fn get(_key: &str) -> Result<Option<String>, String> {
        Err("CloudStorage::get not implemented for this platform".to_string())
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
    pub fn remove(_key: &str) -> Result<(), String> {
        Err("CloudStorage::remove not implemented for this platform".to_string())
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
    pub fn clear() -> Result<(), String> {
        Err("CloudStorage::clear not implemented for this platform".to_string())
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
    if let Err(e) = CloudStorage::init_java_vm(vm) {
        eprintln!("Failed to initialize JavaVM: {}", e);
        return jni::sys::JNI_VERSION_1_1.into();
    }

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

    match CloudStorage::save(key_str, value_str) {
        Ok(_) => 0,
        Err(_) => -1,
    }
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
        Ok(Some(value)) => {
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
        Ok(None) => 0, // Key not found
        Err(_) => -1,  // Error occurred
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

    match CloudStorage::remove(key_str) {
        Ok(_) => 0,
        Err(_) => -1,
    }
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "C" fn cloud_storage_clear() -> i32 {
    match CloudStorage::clear() {
        Ok(_) => 0,
        Err(_) => -1,
    }
}