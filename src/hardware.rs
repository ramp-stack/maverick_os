pub use ::media::{
    Image, ImageFormat, Pixel, Sample, Sound, Audio, AudioFrame, Video, VideoFrame,
    Media, MediaFrame, Cursor, Encoder, Decoder, Tracks, Format, Frame, Player, PlaybackMode,
};
use winit::event_loop::EventLoopProxy;

use std::cell::OnceCell;
use std::path::PathBuf;
use std::rc::Rc;

use crate::events::Output;
use crate::runtime::Runtime;

#[cfg(target_os = "ios")]
mod ios;
#[cfg(target_os = "ios")]
use ios::{Clipboard, Haptics, Share, DataPicker, Push, AudioSession};
#[cfg(target_os = "ios")]
pub use ios::{SafeArea, LaunchScreen, KeyboardInset, Keyboard, Events};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos::{Clipboard, Haptics, Share, DataPicker, Push, AudioSession};
#[cfg(target_os = "macos")]
pub use macos::{SafeArea, LaunchScreen, KeyboardInset, Keyboard, Events};

#[cfg(any(target_os = "ios", target_os = "macos"))]
mod apple;
#[cfg(any(target_os = "ios", target_os = "macos"))]
use apple::{ApplicationSupport, CaptureHub, Logger, MediaCommands};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux::{
    Clipboard, Haptics, Share, DataPicker, Push, AudioSession,
    ApplicationSupport, CaptureHub, Logger, MediaCommands,
};
#[cfg(target_os = "linux")]
pub use linux::{SafeArea, LaunchScreen, KeyboardInset, Keyboard, Events};

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
use android::{
    Clipboard, Haptics, Share, DataPicker, Push, AudioSession,
    ApplicationSupport, CaptureHub, Logger, MediaCommands,
};
#[cfg(target_os = "android")]
pub use android::{SafeArea, LaunchScreen, KeyboardInset, Keyboard, Events};

mod capture;
pub use capture::{Camera, Microphone, Capture};

mod picker;
pub use picker::{Cancled, PickResult, Pickable, Picked, Visual};

mod cache;
pub use cache::Cache;

mod speaker;
pub use speaker::Speaker;

#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum PushToken {Apns(String), Fcm(String)}

#[derive(Debug, Clone)]
pub enum Data {Text(String), Image(Image), Media(Media), File(PathBuf)}

pub struct Playback;
impl Playback {
    pub fn update(id: u128, playing: bool) {MediaCommands::update(id, playing);}
    pub fn remove(id: u128) {MediaCommands::remove(id);}
}

pub struct PushDestination(Push);
impl PushDestination {
    /// Waits for the initial token, then for a different token on each subsequent call.
    pub async fn next(&mut self) -> PushToken {self.0.next().await}
}

#[derive(Clone)]
pub struct Notify(EventLoopProxy<Output>);
impl Notify {
    pub fn notify(&self, header: &str, body: &str) {
        Push::notify(self.0.clone(), header.to_owned(), body.to_owned());
    }
}

#[derive(Clone)]
pub struct Hardware(Rc<_Hardware>, EventLoopProxy<Output>);
impl Hardware {
    pub(crate) fn new(name: String, proxy: EventLoopProxy<Output>, keyboard: Keyboard) -> Self {
        Logger::start(None);
        let app_support = ApplicationSupport::get(name);
        std::fs::create_dir_all(&app_support).unwrap();
        let device_id = ApplicationSupport::device_id(&app_support);
        let cache = Cache::new(&app_support.join("CACHE")).unwrap();
        Runtime::start(cache.clone());
        Self(Rc::new(_Hardware {
            device_id, cache, clipboard: OnceCell::new(), capture: OnceCell::new(),
            keyboard, haptics: OnceCell::new(), share: OnceCell::new(),
            picker: OnceCell::new(),
        }), proxy)
    }

    /// Identifier persisted with this installation's application data.
    pub fn device_id(&self) -> &str {&self.0.device_id}

    pub fn push_destination(&self) -> PushDestination {PushDestination(Push::new())}

    pub fn notifier(&self) -> Notify {Notify(self.1.clone())}

    /// Returns the shared native keyboard capability without showing it.
    pub fn keyboard(&self) -> Keyboard {self.0.keyboard.clone()}

    pub fn start_camera(&self) -> Result<Camera, String> {
        self.0.capture.get_or_init(|| CaptureHub::new(self)).camera(self)
    }
    pub fn start_microphone(&self) -> Result<Microphone, String> {
        self.0.capture.get_or_init(|| CaptureHub::new(self)).microphone(self)
    }
    pub fn start_capture(&self) -> Result<Capture, String> {
        self.0.capture.get_or_init(|| CaptureHub::new(self)).capture(self)
    }
    pub fn trigger_haptic(&self) {self.0.haptics.get_or_init(Haptics::new).trigger()}

    pub fn pick<T: Pickable>(&self) -> Picked<T> {
        self.0.picker.get_or_init(DataPicker::new).pick(self)
    }

    pub fn share(&self, data: &Data) {self.0.share.get_or_init(Share::new).share(data);}
    pub fn set_clipboard(&self, data: &Data) {self.0.clipboard.get_or_init(Clipboard::new).set(data)}
    pub fn get_clipboard(&self) -> Option<Data> {self.0.clipboard.get_or_init(Clipboard::new).get()}
    pub fn set_cache(&self, key: &str, data: &Data) -> Result<(), String> {self.0.cache.set(key, data)}
    pub fn get_cache(&self, key: &str) -> Option<Data> {self.0.cache.get(key)}
}

struct _Hardware {
    device_id: String,
    clipboard: OnceCell<Clipboard>,
    capture: OnceCell<CaptureHub>,
    keyboard: Keyboard,
    haptics: OnceCell<Haptics>,
    share: OnceCell<Share>,
    picker: OnceCell<DataPicker>,
    cache: Cache,
}
