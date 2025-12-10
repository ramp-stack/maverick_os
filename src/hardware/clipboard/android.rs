use jni::objects::{GlobalRef, JObject};
use jni::{JNIEnv, JavaVM};
use std::sync::Arc;

#[derive(Clone)]
pub struct OsClipboard {
    vm: Arc<JavaVM>,
    context: GlobalRef,
}

impl OsClipboard {
    pub fn new(vm: &JavaVM) -> Self {
        let vm = Arc::new(unsafe { JavaVM::from_raw(vm.get_java_vm_pointer()).unwrap() });
        
        let context = {
            let mut env = vm.attach_current_thread().expect("Failed to attach thread");
            let context_obj = ndk_context::android_context().context().cast();
            let context_obj = unsafe { JObject::from_raw(context_obj) };
            env.new_global_ref(context_obj).expect("Failed to create global ref")
        };
        
        Self { vm, context }
    }

    pub fn get_content(&self) -> String {
        self.get_content_impl().unwrap_or_default()
    }

    fn get_content_impl(&self) -> Result<String, Box<dyn std::error::Error>> {
        let mut env = self.vm.attach_current_thread()?;
        let context = self.context.as_obj();

        let clipboard_string = env.new_string("clipboard")?;
        let clipboard_service = env
            .call_method(
                context,
                "getSystemService",
                "(Ljava/lang/String;)Ljava/lang/Object;",
                &[(&clipboard_string).into()],
            )?
            .l()?;

        let clipboard_manager = JObject::from(clipboard_service);

        let primary_clip = env
            .call_method(
                clipboard_manager,
                "getPrimaryClip",
                "()Landroid/content/ClipData;",
                &[],
            )?
            .l()?;

        if primary_clip.is_null() {
            return Ok(String::new());
        }

        let item_count = env
            .call_method(&primary_clip, "getItemCount", "()I", &[])?
            .i()?;
        if item_count == 0 {
            return Ok(String::new());
        }

        let clip_item = env
            .call_method(
                primary_clip,
                "getItemAt",
                "(I)Landroid/content/ClipData$Item;",
                &[0i32.into()],
            )?
            .l()?;

        let text = env
            .call_method(
                clip_item,
                "getText",
                "()Ljava/lang/CharSequence;",
                &[],
            )?
            .l()?;
        if text.is_null() {
            return Ok(String::new());
        }

        let java_string = env.call_method(text, "toString", "()Ljava/lang/String;", &[])?.l()?;
        let rust_string = env.get_string(&java_string.into())?.into();
        Ok(rust_string)
    }

    pub fn set_content(&self, text: String) {
        let _ = self.set_content_impl(text);
    }

    fn set_content_impl(&self, text: String) -> Result<(), Box<dyn std::error::Error>> {
        let mut env = self.vm.attach_current_thread()?;
        let context = self.context.as_obj();

        let clipboard_string = env.new_string("clipboard")?;
        let clipboard_service = env
            .call_method(
                context,
                "getSystemService",
                "(Ljava/lang/String;)Ljava/lang/Object;",
                &[(&clipboard_string).into()],
            )?
            .l()?;

        let clipboard_manager = JObject::from(clipboard_service);

        let clip_data_class = env.find_class("android/content/ClipData")?;
        let label = env.new_string("label")?;
        let text_string = env.new_string(&text)?;
        let clip_data = env
            .call_static_method(
                clip_data_class,
                "newPlainText",
                "(Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Landroid/content/ClipData;",
                &[(&JObject::from(label)).into(), (&JObject::from(text_string)).into()],
            )?
            .l()?;

        env.call_method(
            clipboard_manager,
            "setPrimaryClip",
            "(Landroid/content/ClipData;)V",
            &[(&clip_data).into()],
        )?;

        Ok(())
    }
}