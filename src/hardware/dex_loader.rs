use std::fs;
use std::path::{Path, PathBuf};
use jni::{
    objects::{GlobalRef, JByteBuffer, JClass, JObject, JString, JValue},
    JNIEnv, JavaVM,
};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct LoadDex {
    java_vm: Arc<JavaVM>,
    app_context: GlobalRef,
    dex_directory: PathBuf,
    loaded_class_loaders: Vec<GlobalRef>,
}

impl LoadDex {
    pub fn new(java_vm: Arc<JavaVM>, app_context: GlobalRef) -> Self {
        Self {
            java_vm,
            app_context,
            dex_directory: PathBuf::from("./dex_classes/"),
            loaded_class_loaders: Vec::new(),
        }
    }

    pub fn set_dex_directory<P: AsRef<Path>>(&mut self, path: P) {
        self.dex_directory = path.as_ref().to_path_buf();
    }

    pub fn find_dex_files(&self) -> Vec<PathBuf> {
        let mut dex_files = Vec::new();
        
        for entry in fs::read_dir(&self.dex_directory).expect("Failed to read DEX directory") {
            let entry = entry.expect("Failed to read directory entry");
            let path = entry.path();
            
            if path.is_file() {
                if let Some(extension) = path.extension() {
                    if extension == "dex" {
                        dex_files.push(path);
                    }
                }
            }
        }

        dex_files
    }

    pub fn load_dex_file<P: AsRef<Path>>(&mut self, dex_path: P) -> GlobalRef {
        let dex_path = dex_path.as_ref();
        let dex_bytes = fs::read(dex_path).expect("Failed to read DEX file");
        self.load_dex_from_bytes(&dex_bytes)
    }

    pub fn load_dex_from_bytes(&mut self, dex_bytes: &[u8]) -> GlobalRef {
        let mut env = self.java_vm.attach_current_thread().expect("Failed to attach thread");

        let byte_buffer = env.new_direct_byte_buffer(dex_bytes.as_ptr() as *mut u8, dex_bytes.len())
            .expect("Failed to create byte buffer");

        let context_class = env.get_object_class(&self.app_context)
            .expect("Failed to get context class");
        let get_class_loader_method = env.get_method_id(context_class, "getClassLoader", "()Ljava/lang/ClassLoader;")
            .expect("Failed to get getClassLoader method");
        let parent_class_loader = env.call_method_unchecked(
            &self.app_context,
            get_class_loader_method,
            jni::signature::ReturnType::Object,
            &[],
        ).expect("Failed to call getClassLoader").l().expect("Failed to get class loader object");
        println!("Retrieved parent class loader");

        let dex_class_loader_class = env.find_class("dalvik/system/InMemoryDexClassLoader")
            .expect("Failed to find InMemoryDexClassLoader class");
        let constructor_id = env.get_method_id(
            &dex_class_loader_class,
            "<init>",
            "(Ljava/nio/ByteBuffer;Ljava/lang/ClassLoader;)V",
        ).expect("Failed to get constructor method ID");

        let dex_class_loader_obj = env.new_object_unchecked(
            dex_class_loader_class,
            constructor_id,
            &[
                JValue::Object(&byte_buffer).as_jni(),
                JValue::Object(&parent_class_loader).as_jni(),
            ],
        ).expect("Failed to create InMemoryDexClassLoader");
        println!("Created InMemoryDexClassLoader");

        let global_loader = env.new_global_ref(dex_class_loader_obj)
            .expect("Failed to create global reference");
        self.loaded_class_loaders.push(global_loader.clone());

        let current_thread = env.call_static_method("java/lang/Thread", "currentThread", "()Ljava/lang/Thread;", &[])
            .expect("Failed to get current thread").l().expect("Failed to get thread object");
        env.call_method(
            current_thread,
            "setContextClassLoader",
            "(Ljava/lang/ClassLoader;)V",
            &[JValue::Object(global_loader.as_obj())],
        ).expect("Failed to set context class loader");
        println!("Set context class loader");

        global_loader
    }

    pub fn load_all_dex_files(&mut self) -> Vec<GlobalRef> {
        let dex_files = self.find_dex_files();
        let mut loaded_loaders = Vec::new();

        for dex_file in dex_files {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.load_dex_file(&dex_file)
            })) {
                Ok(loader) => {
                    loaded_loaders.push(loader);
                }
                Err(_) => {
                    println!("Failed to load dex");
                }
            }
        }

        loaded_loaders
    }

    pub fn load_class(&self, class_name: &str) -> JClass {
        let mut env = self.java_vm.attach_current_thread()
            .expect("Failed to attach thread");
        
        for (i, loader) in self.loaded_class_loaders.iter().enumerate() {
            println!("Trying to load class '{}' from loader {}", class_name, i);
            
            let class_name_jstr = env.new_string(class_name)
                .expect("Failed to create Java string");
            match env.call_method(
                loader.as_obj(),
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[JValue::Object(&class_name_jstr)],
            ) {
                Ok(class_obj) => {
                    println!("Successfully loaded class '{}' from loader {}", class_name, i);
                    return JClass::from(class_obj.l().expect("Failed to get class object"));
                }
                Err(_) => {
                    continue;
                }
            }
        }

        panic!("Class '{}' not found in any loaded DEX file", class_name);
    }

    pub fn create_instance(&self, class_name: &str, constructor_sig: &str, args: &[JValue]) -> GlobalRef {
        let mut env = self.java_vm.attach_current_thread()
            .expect("Failed to attach thread");
        
        let class = self.load_class(class_name);
        let constructor_id = env.get_method_id(&class, "<init>", constructor_sig)
            .expect("Failed to get constructor method ID");
        
        let instance = env.new_object_unchecked(
            class,
            constructor_id,
            &args.iter().map(|v| v.as_jni()).collect::<Vec<_>>(),
        ).expect("Failed to create instance");
        
        let global_instance = env.new_global_ref(instance)
            .expect("Failed to create global reference");
        
        global_instance
    }

    pub fn loaded_count(&self) -> usize {
        self.loaded_class_loaders.len()
    }

    pub fn get_dex_directory(&self) -> &Path {
        &self.dex_directory
    }

    pub fn clear_loaded(&mut self) {
        self.loaded_class_loaders.clear();
    }
}