mod logger;
mod cache;
mod camera;
mod share;
mod clipboard;
mod cloud;
mod haptics;
mod photo_picker;
mod safe_area;
mod notifications;

use std::sync::mpsc::Sender;

pub use cache::{Cache, ActiveCache};
pub use clipboard::Clipboard;
pub use camera::{Camera, CameraError, CameraSettings};
pub use share::Share;
pub use cloud::CloudStorage;
pub use photo_picker::{PhotoPicker, ImageOrientation};
pub use safe_area::SafeAreaInsets;
pub use haptics::Haptics;
pub use notifications::Notifications;
pub use logger::Logger;

#[derive(Clone)]
pub struct Context {
    pub cache: Cache,
    pub clipboard: Clipboard,
    pub cloud: CloudStorage,
    pub share: Share,
    pub haptics: Haptics,
    pub notifications: Notifications,
}

impl Context {
    pub(crate) fn new() -> Self {
        Logger::start(None);
        
        #[cfg(target_os = "android")]
        let vm = {
            let vm_ptr = ndk_context::android_context().vm().cast();
            unsafe { jni::JavaVM::from_raw(vm_ptr).unwrap() }
        };
        
        Self {
            cache: Cache::new(
                #[cfg(target_os = "android")]
                &vm
            ),
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
        }
    }
    
    pub fn camera(&self) -> Result<Camera, CameraError> {
        Camera::new()
    }
    
    pub fn photo_picker(&self, sender: Sender<(Vec<u8>, ImageOrientation)>) {
        PhotoPicker::open(sender)
    }
    
    pub fn safe_area_insets(&self) -> (f32, f32, f32, f32) {
        SafeAreaInsets::get()
    }
    
    pub fn clipboard(&self) -> &Clipboard {
        &self.clipboard
    }
    
    pub fn cloud(&self) -> &CloudStorage {
        &self.cloud
    }
    
    pub fn share(&self) -> &Share {
        &self.share
    }
    
    pub fn haptic(&self) -> &Haptics {
        &self.haptics
    }
    
    pub fn notifications(&self) -> &Notifications {
        &self.notifications
    }
}