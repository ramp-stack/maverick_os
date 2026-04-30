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
    pub camera: Arc<Mutex<Option<Camera>>>,
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
            camera: Arc::new(Mutex::new(None)),
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
        if let Some(camera) = self.camera.lock().unwrap().as_ref() {
            if let Ok(frame) = camera.frame() {
                self.pending.lock().unwrap().push(Input::CameraFrame(frame));
            }
        }
        std::mem::take(&mut self.pending.lock().unwrap())
    }

    pub fn start_camera(&self) {
        let mut camera_guard = self.camera.lock().unwrap();
        if camera_guard.is_none() {
            drop(camera_guard);
            if let Ok(camera) = Camera::new() {
                *self.camera.lock().unwrap() = Some(camera);
            }
        }
    }

    pub fn stop_camera(&self) {
        *self.camera.lock().unwrap() = None;
    }

    pub fn photo_picker(&self) {
        let pending = self.pending.clone();
        PhotoPicker::open(move |bytes, orientation| {
            if let Ok(img) = image::load_from_memory(&bytes) {
                pending.lock().unwrap().push(Input::PickedPhoto(orientation.apply_to(img).to_rgba8()));
            }
        });
    }

    pub fn safe_area_insets(&self) -> (f32, f32, f32, f32) {
        SafeAreaInsets::get()
    }
    
    pub fn clipboard(&self) -> &Clipboard {
        &self.clipboard
    }

    pub fn share(&self, data: &str) {
        self.share.share(data);
    }

    pub fn haptic(&self) {
        self.haptics.vibrate();
    }

    pub fn notifications(&self) -> &Notifications {
        &self.notifications
    }

    #[allow(dead_code)]
    pub(crate) fn cloud(&self) -> &CloudStorage {
        &self.cloud
    }
}