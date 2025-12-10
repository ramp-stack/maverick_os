use std::path::PathBuf;
use std::sync::OnceLock;
use jni::{
    JNIEnv,
    objects::{JObject, JString},
    JavaVM,
};

static JAVA_VM: OnceLock<JavaVM> = OnceLock::new();

pub struct OsApplicationSupport;

impl OsApplicationSupport {
    pub fn init_android(vm: &JavaVM) {
        if let Ok(env) = vm.attach_current_thread() {
            if let Ok(new_vm) = env.get_java_vm() {
                JAVA_VM.set(new_vm).expect("JavaVM already initialized");
            }
        }
    }

    pub fn get() -> Option<PathBuf> {
        Self::get_app_name("org.ramp.orange")
    }

    pub fn get_app_name(app_name: &str) -> Option<PathBuf> {
        let vm = JAVA_VM.get().or_else(|| {
            eprintln!("ERROR: Android JavaVM not initialized! Call OsApplicationSupport::init_android() first or ensure JNI_OnLoad is properly set up.");
            None
        })?;
        
        let mut env = vm.attach_current_thread().ok()?;
        Self::get_android_files_dir(&mut env, app_name)
    }

    fn get_android_files_dir(env: &mut JNIEnv, subdir: &str) -> Option<PathBuf> {

        let activity_thread_class = env.find_class("android/app/ActivityThread").ok()?;
        
        let current_activity_thread = env
            .call_static_method(
                activity_thread_class,
                "currentActivityThread",
                "()Landroid/app/ActivityThread;",
                &[],
            )
            .ok()?
            .l()
            .ok()?;
        
        // Get the application context
        let context = env
            .call_method(
                current_activity_thread,
                "getApplication",
                "()Landroid/app/Application;",
                &[],
            )
            .ok()?
            .l()
            .ok()?;
        
        let files_dir = env
            .call_method(&context, "getFilesDir", "()Ljava/io/File;", &[])
            .ok()?
            .l()
            .ok()?;
        
        let path_jstring = env
            .call_method(&files_dir, "getAbsolutePath", "()Ljava/lang/String;", &[])
            .ok()?
            .l()
            .ok()?;
        
        let path_string: String = env
            .get_string(&JString::from(path_jstring))
            .ok()?
            .into();
        
        let mut path = PathBuf::from(path_string);
        
        if !subdir.is_empty() {
            path.push(subdir);
            std::fs::create_dir_all(&path).ok()?;
        }
        
        Some(path)
    }
}