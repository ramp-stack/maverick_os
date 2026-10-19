#[cfg(target_os = "ios")]
mod ios;
#[cfg(target_os = "ios")]
use ios::{Clipboard, Haptics, Share, DataPicker};
#[cfg(target_os = "ios")]
pub use ios::SafeArea;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos::{Clipboard, Haptics, Share, DataPicker};
#[cfg(target_os = "macos")]
pub use macos::SafeArea;

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod apple;
#[cfg(any(target_os = "macos", target_os = "ios"))]
use apple::{ApplicationSupport, CameraHandle, Logger};
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub use apple::{Camera, Pickable, Picked};

mod cache;
pub use cache::Cache;

mod video;
pub use video::Video;

mod player;
pub use player::Player;

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

pub type Image = Arc<image::RgbaImage>;
pub type PickResult = Result<Vec<Data>, Cancled>;

#[derive(Debug, Clone)]
pub enum Data {Text(String), Image(Image), Video(Video), File(PathBuf)}

#[derive(Clone, Debug)]
pub enum Media {Image(Image), Video(Video)}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cancled;

impl std::fmt::Display for Cancled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Selection cancelled")
    }
}

impl std::error::Error for Cancled {}

#[derive(Clone)]
pub struct Hardware(Rc<_Hardware>);
impl Hardware {
    pub fn new(name: String) -> Self {
        Logger::start(None);
        let app_support = ApplicationSupport::get(name);
        let _ = std::fs::create_dir_all(&app_support);
        std::env::set_current_dir(app_support).unwrap();
        Self(Rc::new(_Hardware {
            clipboard: Clipboard::new(),
            camera: CameraHandle::new(),
            haptics: Haptics::new(),
            share: Share::new(),
            picker: Rc::new(DataPicker::new()),
            cache: Cache::new()
        }))
    }

    pub fn start_camera(&self) -> Camera {self.0.camera.start()}
    pub fn trigger_haptic(&self) {self.0.haptics.trigger()}

    pub fn pick<T: Pickable>(&self) -> Picked<T> {self.0.picker.pick()}

    pub fn share(&self, data: &Data) {self.0.share.share(data);}
    pub fn set_clipboard(&self, data: &Data) {self.0.clipboard.set(data)}
    pub fn get_clipboard(&self) -> Option<Data> {self.0.clipboard.get()}
    pub fn set_cache(&self, key: &str, data: &Data) {self.0.cache.set(key, data)}
    pub fn get_cache(&self, key: &str) -> Option<Data> {self.0.cache.get(key)}
}

struct _Hardware {
    clipboard: Clipboard,
    camera: CameraHandle,
    haptics: Haptics,
    share: Share,
    picker: Rc<DataPicker>,
    cache: Cache
}
