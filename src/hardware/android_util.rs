use jni::objects::{ GlobalRef, JValue };
use jni::{ JNIEnv, JavaVM, NativeMethod };
use std::sync::Arc;
use std::error::Error;

pub struct JNIUtil {}

impl JNIUtil {
    /// Explicitly binds native methods on a class, rather than relying on Android's
    /// automatic `Java_pkg_Class_method` symbol-name lookup. That automatic resolution
    /// is scoped to the app's default ClassLoader and does not reliably find native
    /// methods on classes loaded via a dynamically-constructed one (like the
    /// InMemoryDexClassLoader used by `instantiate_class_from_embedded_dex` /
    /// `load_class_from_embedded_dex`), even though the symbol is present in the .so.
    ///
    /// `object` must be a reference to an *instance* of the target class (e.g. the
    /// GlobalRef returned by `instantiate_class_from_embedded_dex`) - its actual
    /// runtime Class is looked up via GetObjectClass, since RegisterNatives needs a
    /// real jclass, not an instance reference reinterpreted as one.
    pub fn register_native_methods(
        env: &mut JNIEnv,
        object: &GlobalRef,
        methods: Vec<NativeMethod>,
    ) -> Result<(), Box<dyn Error>> {
        let jclass = env.get_object_class(object.as_obj())?;
        env.register_native_methods(&jclass, &methods)?;
        Ok(())
    }

    /// Same as `register_native_methods`, but for a GlobalRef that's already a Class object
    /// (e.g. from `load_app_class`), not an instance.
    pub fn register_native_methods_on_class(
        env: &mut JNIEnv,
        class: &GlobalRef,
        methods: Vec<NativeMethod>,
    ) -> Result<(), Box<dyn Error>> {
        let jclass = <&jni::objects::JClass>::from(class.as_obj());
        env.register_native_methods(jclass, &methods)?;
        Ok(())
    }

    /// Loads a class through the app's own ClassLoader (as opposed to the InMemoryDexClassLoader
    /// used elsewhere in this file) - for classes that are actually part of the app's own
    /// build-time dex, like a manifest-declared Activity.
    pub fn load_app_class(
        env: &mut JNIEnv,
        app_context: &GlobalRef,
        class_name: &str,
    ) -> Result<GlobalRef, Box<dyn Error>> {
        let context_class = env.get_object_class(app_context.as_obj())?;
        let get_class_loader = env.get_method_id(
            &context_class,
            "getClassLoader",
            "()Ljava/lang/ClassLoader;",
        )?;
        let loader = unsafe {
            env.call_method_unchecked(
                app_context.as_obj(),
                get_class_loader,
                jni::signature::ReturnType::Object,
                &[],
            )
        }?.l()?;

        let class_name_jstr = env.new_string(class_name)?;
        let loaded_class = env.call_method(
            loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&class_name_jstr)],
        )?.l()?;

        Ok(env.new_global_ref(loaded_class)?)
    }

   /// Loads a class from an embedded .dex file and instantiates it.
    /// Returns a GlobalRef to the instantiated helper object.
    pub unsafe fn instantiate_class_from_embedded_dex(
        java_vm: &Arc<JavaVM>,
        app_context: &GlobalRef,
        dex_bytes: &[u8],
        class_name: &str,
    ) -> Result<GlobalRef, Box<dyn Error>> {
        let mut env = java_vm.attach_current_thread()?;

        // Create ByteBuffer from dex bytes
        let byte_buffer = env.new_direct_byte_buffer(
            dex_bytes.as_ptr() as *mut u8,
            dex_bytes.len(),
        )?;

        // Get the parent ClassLoader from Context
        let context_class = env.get_object_class(app_context.as_obj())?;
        let get_class_loader = env.get_method_id(
            &context_class,
            "getClassLoader",
            "()Ljava/lang/ClassLoader;",
        )?;
        let parent_loader = env.call_method_unchecked(
            app_context.as_obj(),
            get_class_loader,
            jni::signature::ReturnType::Object,
            &[],
        )?.l()?;

        // Create InMemoryDexClassLoader
        let loader_class = env.find_class("dalvik/system/InMemoryDexClassLoader")?;
        let constructor = env.get_method_id(
            &loader_class,
            "<init>",
            "(Ljava/nio/ByteBuffer;Ljava/lang/ClassLoader;)V",
        )?;

        let loader_obj = env.new_object_unchecked(
            loader_class,
            constructor,
            &[
                JValue::Object(&byte_buffer).as_jni(),
                JValue::Object(&parent_loader).as_jni(),
            ],
        )?;

        let global_loader = env.new_global_ref(loader_obj)?;

        // Set context class loader (important for some Android APIs)
        Self::set_context_class_loader(&mut env, &global_loader)?;

        // Load the target class
        let class_name_jstr = env.new_string(class_name)?;
        let loaded_class = env.call_method(
            global_loader.as_obj(),
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&class_name_jstr)],
        )?.l()?;

        let loaded_jclass = jni::objects::JClass::from(loaded_class);

        // Instantiate the class (assumes it has a (Context) constructor)
        let constructor_id = env.get_method_id(
            &loaded_jclass,
            "<init>",
            "(Landroid/content/Context;)V",
        )?;

        let helper_obj = env.new_object_unchecked(
            loaded_jclass,
            constructor_id,
            &[JValue::Object(app_context.as_obj()).as_jni()],
        )?;

        Ok(env.new_global_ref(helper_obj)?)
    }

    fn set_context_class_loader(
        env: &mut JNIEnv,
        loader: &GlobalRef,
    ) -> Result<(), Box<dyn Error>> {
        let thread = env.call_static_method(
            "java/lang/Thread",
            "currentThread",
            "()Ljava/lang/Thread;",
            &[],
        )?.l()?;

        env.call_method(
            thread,
            "setContextClassLoader",
            "(Ljava/lang/ClassLoader;)V",
            &[JValue::Object(loader.as_obj())],
        )?;
        Ok(())
    }

    /// Loads a class from an embedded .dex file without instantiating it.
    /// Useful for classes that only have static methods (like PhotoPickerHelper).
    pub unsafe fn load_class_from_embedded_dex(
        java_vm: &Arc<JavaVM>,
        app_context: &GlobalRef,
        dex_bytes: &[u8],
        class_name: &str,
    ) -> Result<GlobalRef, Box<dyn Error>> {
        let mut env = java_vm.attach_current_thread()?;

        let byte_buffer = env.new_direct_byte_buffer(
            dex_bytes.as_ptr() as *mut u8,
            dex_bytes.len(),
        )?;

        let context_class = env.get_object_class(app_context.as_obj())?;
        let get_class_loader = env.get_method_id(
            &context_class,
            "getClassLoader",
            "()Ljava/lang/ClassLoader;",
        )?;
        let parent_loader = env.call_method_unchecked(
            app_context.as_obj(),
            get_class_loader,
            jni::signature::ReturnType::Object,
            &[],
        )?.l()?;

        let loader_class = env.find_class("dalvik/system/InMemoryDexClassLoader")?;
        let constructor = env.get_method_id(
            &loader_class,
            "<init>",
            "(Ljava/nio/ByteBuffer;Ljava/lang/ClassLoader;)V",
        )?;

        let loader_obj = env.new_object_unchecked(
            loader_class,
            constructor,
            &[
                JValue::Object(&byte_buffer).as_jni(),
                JValue::Object(&parent_loader).as_jni(),
            ],
        )?;

        let global_loader = env.new_global_ref(loader_obj)?;
        Self::set_context_class_loader(&mut env, &global_loader)?;

        // Load the class
        let class_name_jstr = env.new_string(class_name)?;
        let result = env.call_method(
            global_loader.as_obj(),
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&class_name_jstr)],
        );

        // Better error reporting
        if result.is_err() && env.exception_check().unwrap_or(false) {
            env.exception_describe(); // ← This prints the real Java exception to logcat
            let _ = env.exception_clear();
        }

        let loaded_class = result?.l()?;
        Ok(env.new_global_ref(loaded_class)?)
    }
}