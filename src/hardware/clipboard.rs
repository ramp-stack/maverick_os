#[cfg(target_os = "ios")]
use objc2_foundation::NSString;
#[cfg(target_os = "ios")]
use objc2_ui_kit::UIPasteboard;

#[cfg(target_os = "android")]
use jni::objects::{GlobalRef, JObject};
#[cfg(target_os = "android")]
use jni::{JNIEnv, JavaVM};

#[cfg(target_os = "android")]
use std::sync::{Arc, Mutex};


#[cfg(target_os = "android")]
static CLIPBOARD_INSTANCE: Mutex<Option<Clipboard>> = Mutex::new(None);

/// Clipboard access for copying and pasting text.
#[derive(Clone)]
pub struct Clipboard {
    #[cfg(target_os = "android")]
    vm: Arc<JavaVM>,
    #[cfg(target_os = "android")]
    context: GlobalRef,
}

#[cfg(target_os = "android")]
impl Clipboard {
    pub fn new() -> Option<Self> {
        let instance = CLIPBOARD_INSTANCE.lock().unwrap();
        instance.clone()
    }

    pub fn initialize(env: JNIEnv<'_>, context: JObject) -> Result<(), jni::errors::Error> {
        let vm = Arc::new(env.get_java_vm()?);
        let global_context = env.new_global_ref(context)?;
        let clipboard = Self {
            vm,
            context: global_context,
        };
        let mut instance = CLIPBOARD_INSTANCE.lock().unwrap();
        *instance = Some(clipboard);
        Ok(())
    }

    pub fn get_content(&self) -> Result<String, jni::errors::Error> {
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

    pub fn set_content(&self, text: String) -> Result<(), jni::errors::Error> {
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

#[cfg(not(target_os = "android"))]
impl Default for Clipboard {
    fn default() -> Self {
        Self::new()
    }
}

impl Clipboard {
    #[cfg(any(target_os = "ios", target_os = "macos"))]
    pub fn new() -> Self {
        Self {}
    }

    #[cfg(not(any(target_os = "ios", target_os = "macos", target_os = "android")))]
    pub fn new() -> Self {
        Self {}
    }

    pub fn get() -> String {
        #[cfg(target_os = "android")]
        {
            if let Some(clipboard) = Self::new() {
                clipboard.get_content().unwrap_or_default()
            } else {
                String::new()
            }
        }

        #[cfg(target_os = "ios")]
        {
            Self::new().get_content()
        }

        #[cfg(target_os = "macos")]
        {
            Self::new().get_content()
        }

        #[cfg(not(any(target_os = "ios", target_os = "macos", target_os = "android")))]
        {
            String::new()
        }
    }

    pub fn set(text: String) {
        #[cfg(target_os = "android")]
        {
            if let Some(clipboard) = Self::new() {
                let _ = clipboard.set_content(text);
            }
        }

        #[cfg(target_os = "ios")]
        {
            Self::new().set_content(text);
        }

        #[cfg(target_os = "macos")]
        {
            Self::new().set_content(text);
        }

        #[cfg(not(any(target_os = "ios", target_os = "macos", target_os = "android")))]
        {
            // No-op
        }
    }

    // iOS-specific
    #[cfg(target_os = "ios")]
    pub fn get_content(&self) -> String {
        unsafe {
            let pasteboard = UIPasteboard::generalPasteboard();
            pasteboard.string().map(|s| s.to_string()).unwrap_or_default()
        }
    }

    #[cfg(target_os = "ios")]
    pub fn set_content(&self, text: String) {
        unsafe {
            let pasteboard = UIPasteboard::generalPasteboard();
            let ns_string = NSString::from_str(&text);
            pasteboard.setString(Some(&ns_string));
        }
    }

    // macOS-specific
    #[cfg(target_os = "macos")]
    pub fn get_content(&self) -> String {
        cli_clipboard::get_contents().unwrap_or_default()
    }

    #[cfg(target_os = "macos")]
    pub fn set_content(&self, text: String) {
        let _ = cli_clipboard::set_contents(text);
    }

    // No-op for unsupported platforms
    #[cfg(not(any(target_os = "ios", target_os = "macos", target_os = "android")))]
    pub fn get_content(&self) -> String {
        String::new()
    }

    #[cfg(not(any(target_os = "ios", target_os = "macos", target_os = "android")))]
    pub fn set_content(&self, _text: String) {
        // No-op
    }
}
