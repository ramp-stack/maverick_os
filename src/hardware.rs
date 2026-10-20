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
mod video_player;

pub use clipboard::Clipboard;
pub use camera::Camera;
pub use share::Share;
pub use cloud::CloudStorage;
pub use photo_picker::{PhotoPicker, Media, MAX_VIDEO_BYTES};
pub use video_player::VideoPlayer;
pub use safe_area::SafeAreaInsets;
pub use haptics::Haptics;
pub use notifications::Notifications;
pub use logger::Logger;

use crate::window::Input;

#[derive(Clone)]
pub struct Context {
    pub camera: Camera,
    pub clipboard: Clipboard,
    pub share: Share,
    pub haptics: Haptics,
    pub notifications: Notifications,
    pub photo_picker: PhotoPicker,
    pub video_player: VideoPlayer,

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
            cloud,
            photo_picker: PhotoPicker::new(),
            video_player: VideoPlayer::new(),
        }
    }

    pub(crate) fn tick(&mut self) -> Vec<Input> {
        let mut events = Vec::new();
        if let Some(frame) = self.camera.tick() {
            events.push(Input::CameraFrame(frame));
        }
        if let Some(photo) = self.photo_picker.tick() {
            events.push(Input::SelectedPhoto(photo));
        }
        if let Some(media) = self.photo_picker.tick_media() {
            events.push(Input::SelectedMedia(media));
        }
        events
    }
}
impl Default for Context {fn default() -> Self {Self::new()}}
