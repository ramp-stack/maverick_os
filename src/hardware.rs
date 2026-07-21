mod android_util;
mod app_support;
mod camera;
mod clipboard;
mod cloud;
mod haptics;
mod logger;
mod notifications;
mod photo_picker;
mod safe_area;
mod share;

pub use camera::Camera;
pub use clipboard::Clipboard;
pub use cloud::CloudStorage;
pub use haptics::Haptics;
pub use logger::Logger;
pub use notifications::Notifications;
pub use photo_picker::PhotoPicker;
pub use safe_area::SafeAreaInsets;
pub use share::Share;

use crate::window::Input;

pub struct Context {
    pub camera: Camera,
    pub clipboard: Clipboard,
    pub share: Share,
    pub haptics: Haptics,
    pub notifications: Notifications,
    pub photo_picker: PhotoPicker,
    // pub(crate) cloud: CloudStorage
}
impl Context {
    pub fn new() -> Self {
        std::env::set_current_dir(
            app_support::ApplicationSupport::get().expect("Could not get app support dir"),
        )
        .unwrap();
        Logger::start(None);
        // let cloud = CloudStorage::new(
        //     #[cfg(target_os = "android")]
        //     &vm
        // );

        Context {
            camera: Camera::new(),
            clipboard: Clipboard::new(),
            share: Share::new(),
            haptics: Haptics::new(),
            notifications: Notifications::new(),
            // cloud,
            photo_picker: PhotoPicker::new(),
        }
    }

    pub(crate) fn tick(&mut self) -> Vec<Input> {
        let mut events = Vec::new();
        if let Some(frame) = self.camera.tick() {
            events.push(Input::CameraFrame(frame));
        }
        if let Some(photo) = self.photo_picker.tick() {
            events.push(Input::Photo(photo));
        }
        events
    }
}
impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}
