use jni::{
    JNIEnv,
    objects::{JObject, JString, JValue},
    JavaVM,
};
use std::sync::OnceLock;


static JAVA_VM: OnceLock<JavaVM> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextMenuAction {
    Copy,
    Paste,
    Cut,
    SelectAll,
    Delete,
    Custom(String),
}

impl ContextMenuAction {
    fn label(&self) -> &str {
        match self {
            ContextMenuAction::Copy => "Copy",
            ContextMenuAction::Paste => "Paste",
            ContextMenuAction::Cut => "Cut",
            ContextMenuAction::SelectAll => "Select All",
            ContextMenuAction::Delete => "Delete",
            ContextMenuAction::Custom(label) => label.as_str(),
        }
    }

    fn from_index(index: i32, actions: &[ContextMenuAction]) -> Option<ContextMenuAction> {
        actions.get(index as usize).cloned()
    }
}

pub struct ContextMenu;

impl ContextMenu {
    pub fn init_android(vm: &JavaVM) {
        if let Ok(env) = vm.attach_current_thread() {
            if let Ok(new_vm) = env.get_java_vm() {
                JAVA_VM.set(new_vm).expect("JavaVM already initialized");
            }
        }
    }

    pub fn show(x: f32, y: f32, actions: &[ContextMenuAction]) -> Option<ContextMenuAction> {
        let vm = JAVA_VM.get().or_else(|| {
            eprintln!("ERROR: Android JavaVM not initialized! Call ContextMenu::init_android() first.");
            None
        })?;

        let mut env = vm.attach_current_thread().ok()?;
        Self::show_android_popup(&mut env, x, y, actions)
    }

    fn show_android_popup(
        env: &mut JNIEnv,
        x: f32,
        y: f32,
        actions: &[ContextMenuAction],
    ) -> Option<ContextMenuAction> {
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

        let string_class = env.find_class("java/lang/String").ok()?;
        let labels = env
            .new_object_array(actions.len() as i32, string_class, JObject::null())
            .ok()?;

        for (i, action) in actions.iter().enumerate() {
            let label = env.new_string(action.label()).ok()?;
            env.set_object_array_element(&labels, i as i32, label)
                .ok()?;
        }

        let helper_class = env
            .find_class("org/ramp/orange/ContextMenuHelper")
            .ok()?;

        let result = env
            .call_static_method(
                helper_class,
                "showAndWait",
                "(Landroid/content/Context;FF[Ljava/lang/String;)I",
                &[
                    JValue::Object(&context),
                    JValue::Float(x),
                    JValue::Float(y),
                    JValue::Object(&labels.into()),
                ],
            )
            .ok()?
            .i()
            .ok()?;

        if result < 0 {
            None
        } else {
            ContextMenuAction::from_index(result, actions)
        }
    }
}