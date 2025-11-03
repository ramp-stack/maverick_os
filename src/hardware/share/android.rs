 use jni::objects::{GlobalRef, JObject, JValue};
use jni::sys::jobject;
use jni::{JNIEnv, JavaVM};
use ndk_context;
use std::error::Error;
use std::sync::{Once, OnceLock};
use image::RgbaImage;

static JAVA_VM: OnceLock<JavaVM> = OnceLock::new();
static APP_CONTEXT: OnceLock<GlobalRef> = OnceLock::new();
static INIT_ONCE: Once = Once::new();

#[derive(Clone)]
pub struct OsShare;

impl OsShare {
    pub fn new() -> Self {
        Self
    }

    pub fn share(&self, text: &str) {
        if JAVA_VM.get().is_none() {
            if let Err(e) = initialize() {
                eprintln!("Failed to initialize Share: {}", e);
                return;
            }
        }

        if let Some(vm) = JAVA_VM.get() {
            if let Ok(mut env) = vm.attach_current_thread() {
                if let Err(e) = self.share_with_jni(&mut env, text) {
                    eprintln!("Failed to share on Android: {}", e);
                }
            } else {
                eprintln!("Failed to attach to current thread");
            }
        } else {
            eprintln!("JavaVM not initialized. Make sure to call Share::initialize() first.");
        }
    }

    fn share_with_jni(&self, env: &mut JNIEnv, text: &str) -> Result<(), Box<dyn Error>> {
        let chooser_intent = self.create_share_intent(env, text)?;
        self.start_share_activity(env, chooser_intent)?;
        Ok(())
    }

    fn create_share_intent<'a>(&self, env: &mut JNIEnv<'a>, text: &str) -> Result<JObject<'a>, Box<dyn Error>> {
        let intent_class = env.find_class("android/content/Intent")?;
        let intent = env.new_object(intent_class, "()V", &[])?;

        let action_send = env.new_string("android.intent.action.SEND")?;
        env.call_method(
            &intent,
            "setAction",
            "(Ljava/lang/String;)Landroid/content/Intent;",
            &[JValue::Object(&action_send)],
        )?;

        let mime_type = env.new_string("text/plain")?;
        env.call_method(
            &intent,
            "setType",
            "(Ljava/lang/String;)Landroid/content/Intent;",
            &[JValue::Object(&mime_type)],
        )?;

        let extra_text = env.new_string("android.intent.extra.TEXT")?;
        let share_text = env.new_string(text)?;
        env.call_method(
            &intent,
            "putExtra",
            "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/Intent;",
            &[JValue::Object(&extra_text), JValue::Object(&share_text)],
        )?;

        let flags = env.get_static_field("android/content/Intent", "FLAG_ACTIVITY_NEW_TASK", "I")?;
        let flag_value = flags.i()?;
        env.call_method(
            &intent,
            "addFlags",
            "(I)Landroid/content/Intent;",
            &[JValue::Int(flag_value)],
        )?;

        let chooser_title = env.new_string("Share via")?;
        let intent_class_static = env.find_class("android/content/Intent")?;
        let chooser = env.call_static_method(
            intent_class_static,
            "createChooser",
            "(Landroid/content/Intent;Ljava/lang/CharSequence;)Landroid/content/Intent;",
            &[JValue::Object(&intent), JValue::Object(&chooser_title)],
        )?;

        let chooser_obj = chooser.l()?;
        env.call_method(
            &chooser_obj,
            "addFlags",
            "(I)Landroid/content/Intent;",
            &[JValue::Int(flag_value)],
        )?;

        Ok(chooser_obj)
    }

    fn start_share_activity<'a>(&self, env: &mut JNIEnv<'a>, chooser_intent: JObject<'a>) -> Result<(), Box<dyn Error>> {
        if let Some(global_context) = APP_CONTEXT.get() {
            let context = env.new_local_ref(global_context)?;

            env.call_method(
                &context,
                "startActivity",
                "(Landroid/content/Intent;)V",
                &[JValue::Object(&chooser_intent)],
            )?;
            Ok(())
        } else {
            Err("App context not initialized. Call Share::initialize() first.".into())
        }
    }

    pub fn share_image(&self, _rgba_image: RgbaImage) {
        // Android share image implementation placeholder
    }
}

pub fn initialize() -> Result<(), Box<dyn Error>> {
    let jvm = unsafe { JavaVM::from_raw(ndk_context::android_context().vm().cast())? };

    let global_context = {
        let mut env = jvm.attach_current_thread()?;

        let ctx_ptr = ndk_context::android_context().context();
        if ctx_ptr.is_null() {
            return Err("Failed to get Android context".into());
        }

        let context_obj = unsafe { JObject::from_raw(ctx_ptr as jobject) };
        env.new_global_ref(context_obj)?
    };

    JAVA_VM.set(jvm).map_err(|_| "JavaVM already initialized")?;
    APP_CONTEXT.set(global_context).map_err(|_| "App context already initialized")?;

    Ok(())
}