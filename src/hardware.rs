mod logger;
mod camera;
mod share;
mod clipboard;
mod cloud;
mod haptics;
mod photo_picker;
mod safe_area;
mod notifications;
mod app_support;

pub use clipboard::Clipboard;
pub use camera::Camera;
pub use share::Share;
pub use cloud::CloudStorage;
pub use photo_picker::PhotoPicker;
pub use safe_area::SafeAreaInsets;
pub use haptics::Haptics;
pub use notifications::Notifications;
pub use logger::Logger;

use crate::window::Input;

pub struct Context {
    pub camera: Camera,
    pub clipboard: Clipboard,
    pub share: Share,
    pub haptics: Haptics,
    pub notifications: Notifications,
    //photo_picker: PhotoPicker

    #[allow(dead_code)]
    pub(crate) cloud: CloudStorage
}
impl Context {
    pub fn new() -> Self {
        std::env::set_current_dir(app_support::ApplicationSupport::get().expect("Could not get app support dir")).unwrap();
        Logger::start(None);
        #[cfg(target_os = "android")]
        let vm = {
            let vm_ptr = ndk_context::android_context().vm().cast();
            unsafe { jni::JavaVM::from_raw(vm_ptr).unwrap() }
        };
        let cloud = CloudStorage::new(
            #[cfg(target_os = "android")]
            &vm
        );

        Context {
            camera: Camera::new(),
            clipboard: Clipboard::new(
                #[cfg(target_os = "android")]
                &vm
            ),
            share: Share::new(),
            haptics: Haptics::new(),
            notifications: Notifications::new(),
            cloud
            //photo_picker: PhotoPicker::new(tx),
        }
    }

    pub(crate) fn tick(&mut self) -> Vec<Input> {
        let mut events = Vec::new();
        if let Some(frame) = self.camera.tick() {
            events.push(Input::CameraFrame(frame));
        }
        events
    }
}
impl Default for Context {fn default() -> Self {Self::new()}}
