mod logger;
mod camera;
mod share;
mod clipboard;
mod cloud;
mod haptics;
pub mod photo_picker;
mod safe_area;
mod notifications;
mod app_support;

use std::env;
use std::sync::{Arc, Mutex};

pub use clipboard::Clipboard;
pub use camera::{Camera, CameraError, CameraSettings};
pub use share::Share;
pub use cloud::CloudStorage;
pub use photo_picker::{PhotoPicker, ImageOrientation};
pub use safe_area::SafeAreaInsets;
pub use haptics::Haptics;
pub use notifications::Notifications;
pub use logger::Logger;
use crate::window::Input;
use image::RgbaImage;

#[derive(Clone)]
pub struct Context {
    pub clipboard: Clipboard,
    pub cloud: CloudStorage,
    pub share: Share,
    pub haptics: Haptics,
    pub notifications: Notifications,
    pending: Arc<Mutex<Vec<Input>>>,
}

impl Context {
    pub(crate) fn new() -> Self {
        let _ = env::set_current_dir(app_support::ApplicationSupport::get().expect("Could not get app support dir"));
        Logger::start(None);

        #[cfg(target_os = "android")]
        let vm = {
            let vm_ptr = ndk_context::android_context().vm().cast();
            unsafe { jni::JavaVM::from_raw(vm_ptr).unwrap() }
        };

        Self {
            clipboard: Clipboard::new(
                #[cfg(target_os = "android")]
                &vm
            ),
            cloud: CloudStorage::new(
                #[cfg(target_os = "android")]
                &vm
            ),
            share: Share::new(),
            haptics: Haptics::new(),
            notifications: Notifications::new(),
            pending: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub(crate) fn tick(&mut self) -> Vec<Input> {
        std::mem::take(&mut self.pending.lock().unwrap())
    }

    pub fn camera(&self) -> Option<Camera> {
        Camera::new().ok()
    }

    pub fn camera_existing(&self) -> Option<Camera> {
        Camera::existing()
    }

    pub fn photo_picker(&self) {
        let pending = self.pending.clone();
        PhotoPicker::open(move |bytes, orientation| {
            let event = if let Ok(img) = image::load_from_memory(&bytes) {
                Input::PickedPhoto(orientation.apply_to(img).to_rgba8(), true)
            } else {
                Input::PickedPhoto(RgbaImage::new(0, 0), false)
            };
            pending.lock().unwrap().push(event);
        });
    }

    pub fn safe_area_insets(&self) -> (f32, f32, f32, f32) {
        SafeAreaInsets::get()
    }

    pub fn clipboard(&self) -> &Clipboard {
        &self.clipboard
    }

    #[allow(dead_code)]
    pub(crate) fn cloud(&self) -> &CloudStorage {
        &self.cloud
    }

    pub fn share(&self, data: &str) {
        self.share.share(data);
    }

    pub fn haptic(&self) { self.haptics.vibrate() }

    pub fn notifications(&self) -> &Notifications {
        &self.notifications
    }
}