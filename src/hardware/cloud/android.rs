use jni::objects::{JObject, JString, JValue, GlobalRef};
use jni::{JNIEnv, JavaVM};
use std::sync::{Mutex, OnceLock};
use std::ffi::{CStr, c_char};

static JAVA_VM: OnceLock<JavaVM> = OnceLock::new();
static APP_CONTEXT: OnceLock<Mutex<Option<GlobalRef>>> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct OsCloudStorage;

impl OsCloudStorage {
    pub fn new() -> Self {
        OsCloudStorage
    }

    pub fn save(&self, key: &str, value: &str) {
        Self::save_static(key, value);
    }

    pub fn get(&self, key: &str) -> Option<String> {
        Self::get_static(key)
    }

    pub fn remove(&self, key: &str) {
        Self::remove_static(key);
    }

    pub fn clear(&self) {
        Self::clear_static();
    }

    // Static implementations (keeping the originals for FFI)
    fn save_static(key: &str, value: &str) {
        let vm = JAVA_VM.get().expect("JavaVM not initialized");
        let mut env = vm.attach_current_thread()
            .expect("Failed to attach thread");

        let context = Self::get_or_create_application_context(&mut env);

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

    fn get_static(key: &str) -> Option<String> {
        let vm = JAVA_VM.get().expect("JavaVM not initialized");
        let mut env = vm.attach_current_thread()
            .expect("Failed to attach thread");

        let context = Self::get_or_create_application_context(&mut env);

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

    fn remove_static(key: &str) {
        let vm = JAVA_VM.get().expect("JavaVM not initialized");
        let mut env = vm.attach_current_thread()
            .expect("Failed to attach thread");

        let context = Self::get_or_create_application_context(&mut env);

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

    fn clear_static() {
        let vm = JAVA_VM.get().expect("JavaVM not initialized");
        let mut env = vm.attach_current_thread()
            .expect("Failed to attach thread");

        let context = Self::get_or_create_application_context(&mut env);

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

    fn get_or_create_application_context<'a>(env: &mut JNIEnv<'a>) -> JObject<'a> {
        if let Some(context_mutex) = APP_CONTEXT.get() {
            if let Ok(context_guard) = context_mutex.lock() {
                if let Some(context_ref) = context_guard.as_ref() {
                    return env.new_local_ref(context_ref.as_obj())
                        .expect("Failed to create local ref from global");
                }
            }
        }

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

        let global_context = env.new_global_ref(&context)
            .expect("Failed to create global ref");

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

        context
    }

    pub fn init_java_vm(vm: &JavaVM) {
        if let Ok(env) = vm.attach_current_thread() {
            if let Ok(new_vm) = env.get_java_vm() {
                JAVA_VM.set(new_vm).ok();
            }
        }
    }
}

impl Default for OsCloudStorage {
    fn default() -> Self {
        Self::new()
    }
}

// C FFI exports
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

    OsCloudStorage::save_static(key_str, value_str);
    0
}

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

    match OsCloudStorage::get_static(key_str) {
        Some(value) => {
            let value_bytes = value.as_bytes();
            if value_bytes.len() + 1 > buffer_size {
                return -2;
            }

            unsafe {
                std::ptr::copy_nonoverlapping(value_bytes.as_ptr(), buffer as *mut u8, value_bytes.len());
                *buffer.add(value_bytes.len()) = 0;
            }

            value_bytes.len() as i32
        }
        None => 0,
    }
}

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

    OsCloudStorage::remove_static(key_str);
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn cloud_storage_clear() -> i32 {
    OsCloudStorage::clear_static();
    0
}