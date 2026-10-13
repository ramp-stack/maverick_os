#[cfg(target_os = "ios")]
mod ios;
#[cfg(target_os = "ios")]
use ios::{Clipboard, Haptics, Share, PhotoPicker};
#[cfg(target_os = "ios")]
pub use ios::SafeArea;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos::{Clipboard, Haptics, Share, PhotoPicker};
#[cfg(target_os = "macos")]
pub use macos::SafeArea;

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod apple;
#[cfg(any(target_os = "macos", target_os = "ios"))]
use apple::{ApplicationSupport, CameraHandle, Camera, Logger};

use predator::{traits, Data, Image};
use crossfire::{spsc, AsyncRx};

impl traits::Camera for Camera {
    fn frame(&mut self) -> Image {Camera::frame(self)}
}

pub struct PickedPhoto(AsyncRx<spsc::One<Result<Image, ()>>>);

impl traits::PickedPhoto for PickedPhoto {
    async fn get(&mut self) -> Result<Image, ()> {self.0.recv().await.unwrap()}
    fn try_get(&mut self) -> Option<Result<Image, ()>> {self.0.try_recv().ok()}
}

impl traits::Clipboard for Clipboard {
    fn get(&mut self) -> Option<Data> {Clipboard::get(self)}
    fn set(&mut self, data: Data) {Clipboard::set(self, data)}
}

pub struct Hardware {
    clipboard: Clipboard,
    camera: CameraHandle,
    haptics: Haptics,
    share: Share,
    photo_picker: PhotoPicker
}
impl Hardware {
    pub fn new() -> Self {
        Logger::start(None);
        std::env::set_current_dir(ApplicationSupport::get()).unwrap();
        Hardware {
            clipboard: Clipboard::new(),
            camera: CameraHandle::new(),
            haptics: Haptics::new(),
            share: Share::new(),
            photo_picker: PhotoPicker::new()
        }
    }
}
impl Default for Hardware {fn default() -> Self {Self::new()}}

impl traits::Hardware for Hardware {
    type Camera = Camera;
    type PickedPhoto = PickedPhoto;

    fn start_camera(&mut self) -> Self::Camera {self.camera.start()}
    fn pick_photo(&mut self) -> Self::PickedPhoto {PickedPhoto(self.photo_picker.pick_photo())}

    fn share(&mut self, data: Data) {self.share.share(data);}
    fn clipboard(&mut self) -> &mut dyn traits::Clipboard {&mut self.clipboard}

    fn trigger_haptic(&mut self) {self.haptics.trigger()}
}
