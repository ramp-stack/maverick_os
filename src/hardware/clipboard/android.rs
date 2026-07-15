use jni::objects::{GlobalRef, JObject};
use jni::{JNIEnv, JavaVM};
use std::sync::Arc;

#[derive(Clone)]
pub struct OsClipboard {
    vm: Arc<JavaVM>,
    context: GlobalRef,
}

impl OsClipboard {
    pub fn new() -> Self {
        // Get JavaVM
        let vm = match unsafe {
            JavaVM::from_raw(ndk_context::android_context().vm().cast())
        } {
            Ok(vm) => Arc::new(vm),
            Err(e) => {
                log::error!("Failed to get JavaVM: {}", e);
                // Critical failure - better to panic than return broken state
                panic!("Critical: JavaVM unavailable for OsClipboard");
            }
        };

        // Get application/activity context as GlobalRef
        let context = {
            let mut env = vm
                .attach_current_thread()
                .expect("Failed to attach thread to get context");

            let context_ptr = ndk_context::android_context().context().cast();
            let context_obj = unsafe { JObject::from_raw(context_ptr) };

            env.new_global_ref(context_obj)
                .expect("Failed to create global ref to Android context")
        };

        Self { vm, context }
    }

    pub fn get_content(&self) -> String {
        self.get_content_impl().unwrap_or_default()
    }

    fn get_content_impl(&self) -> Result<String, Box<dyn std::error::Error>> {
        let mut env = self.vm.attach_current_thread()?;
        let context = self.context.as_obj();

        // Get ClipboardManager
        let clipboard_name = env.new_string("clipboard")?;
        let clipboard_service = env
            .call_method(
                context,
                "getSystemService",
                "(Ljava/lang/String;)Ljava/lang/Object;",
                &[(&clipboard_name).into()],
            )?
            .l()?;

        let clipboard_manager = JObject::from(clipboard_service);

        // Get primary clip
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

        let item_count: i32 = env
            .call_method(&primary_clip, "getItemCount", "()I", &[])?
            .i()?;

        if item_count == 0 {
            return Ok(String::new());
        }

        // Get first item
        let clip_item = env
            .call_method(
                primary_clip,
                "getItemAt",
                "(I)Landroid/content/ClipData$Item;",
                &[0i32.into()],
            )?
            .l()?;

        let text = env
            .call_method(clip_item, "getText", "()Ljava/lang/CharSequence;", &[])?
            .l()?;

        if text.is_null() {
            return Ok(String::new());
        }

        // Convert to Rust String
        let java_string = env
            .call_method(&text, "toString", "()Ljava/lang/String;", &[])?
            .l()?;

        let java_string: jni::objects::JString = env
            .call_method(&text, "toString", "()Ljava/lang/String;", &[])?
            .l()?
            .into();

        let rust_string: String = env.get_string(&java_string)?.into();        
        
        Ok(rust_string)
    }

    pub fn set_content(&self, text: String) {
        // We ignore errors here on purpose (fire-and-forget style)
        let _ = self.set_content_impl(text);
    }

    fn set_content_impl(&self, text: String) -> Result<(), Box<dyn std::error::Error>> {
        let mut env = self.vm.attach_current_thread()?;
        let context = self.context.as_obj();

        // Get ClipboardManager
        let clipboard_name = env.new_string("clipboard")?;
        let clipboard_service = env
            .call_method(
                context,
                "getSystemService",
                "(Ljava/lang/String;)Ljava/lang/Object;",
                &[(&clipboard_name).into()],
            )?
            .l()?;

        let clipboard_manager = JObject::from(clipboard_service);

        // Create ClipData
        let clip_data_class = env.find_class("android/content/ClipData")?;
        let label = env.new_string("label")?;
        let text_string = env.new_string(&text)?;

        let clip_data = env
            .call_static_method(
                clip_data_class,
                "newPlainText",
                "(Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Landroid/content/ClipData;",
                &[(&label).into(), (&text_string).into()],
            )?
            .l()?;

        // Set it
        env.call_method(
            clipboard_manager,
            "setPrimaryClip",
            "(Landroid/content/ClipData;)V",
            &[(&clip_data).into()],
        )?;

        Ok(())
    }
}