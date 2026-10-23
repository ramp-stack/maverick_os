use ashpd::desktop::file_chooser::{FileFilter, SelectedFiles};
use ashpd::desktop::notification::{Notification, NotificationProxy};
use ashpd::desktop::open_uri::OpenFileRequest;
use crossfire::{spsc, AsyncTx};
use tokio::sync::Notify;
use winit::event::WindowEvent;
use winit::event_loop::EventLoopProxy;

use std::borrow::Cow;
use std::cell::RefCell;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{Data, Hardware, Image, Media, Pickable, Picked, PickResult, PushDestination};
use super::picker::{PickKind, PickRequest};
use crate::events::Output;
use crate::inputs::Input;
use crate::runtime::RUNTIME;

#[derive(Clone)]
pub struct Keyboard;

#[derive(Default)]
pub struct Keyboards;
impl Keyboards {
    pub fn open(&self, _proxy: EventLoopProxy<Output>) -> Keyboard {Keyboard}
}

pub struct SafeArea;
impl SafeArea {
    pub fn touch_controls() -> bool {false}
    pub fn physical(window: &winit::window::Window) -> crate::PhysicalWindow {
        let size = window.inner_size();
        crate::PhysicalWindow {
            width: size.width, height: size.height, scale_factor: window.scale_factor(),
        }
    }
    pub fn get(_window: &winit::window::Window) -> (f32, f32, f32, f32) {(0.0, 0.0, 0.0, 0.0)}
}

pub struct LaunchScreen;
impl LaunchScreen {
    pub fn new(_window: &crate::Window) -> Self {Self}
    pub fn dismiss(&mut self) {}
}

pub struct KeyboardInset;
impl KeyboardInset {
    pub fn new(_window: &winit::window::Window, _proxy: EventLoopProxy<Output>) -> Self {Self}
    pub fn get(&self) -> (f32, bool) {(0.0, false)}
}

pub struct Events;
impl Events {
    pub fn new(_window: &crate::Window, _proxy: EventLoopProxy<Output>) -> Self {Self}
    pub fn process(&mut self, event: WindowEvent, _scale: f64) -> Option<WindowEvent> {Some(event)}
    pub fn suspend(&mut self) {}
    pub async fn next(&mut self) -> Input {std::future::pending().await}
}

pub struct Logger;
impl Logger {
    pub fn start(level: Option<log::Level>) {
        env_logger::builder().filter_level(level.unwrap_or(log::Level::Info).to_level_filter()).init();
    }
}

pub struct ApplicationSupport;
impl ApplicationSupport {
    pub fn get(name: String) -> PathBuf {
        assert!(matches!(Path::new(&name).components().collect::<Vec<_>>().as_slice(),
            [std::path::Component::Normal(_)]));
        let directory = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from)
            .filter(|path| path.is_absolute()).unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").expect("HOME is required"))
                    .join(".local/share")
            });
        directory.join(name)
    }

    pub fn device_id(directory: &Path) -> String {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = OpenOptions::new().read(true).write(true).create(true).truncate(false)
            .mode(0o600).open(directory.join("DEVICE_ID")).unwrap();
        file.lock().unwrap();
        let mut id = String::new();
        file.read_to_string(&mut id).unwrap();
        if id.is_empty() {
            id = hex::encode(rand::random::<[u8; 16]>());
            file.write_all(id.as_bytes()).unwrap();
            file.sync_all().unwrap();
        }
        assert!(id.len() == 32 && id.bytes().all(|byte| byte.is_ascii_hexdigit()));
        id
    }
}

pub struct Clipboard(RefCell<Option<arboard::Clipboard>>);
impl Clipboard {
    pub fn new() -> Self {Self(RefCell::new(None))}

    pub fn set(&self, data: &Data) {
        let mut clipboard = self.0.borrow_mut();
        if clipboard.is_none() {
            match arboard::Clipboard::new() {
                Ok(value) => *clipboard = Some(value),
                Err(error) => {log::warn!("Clipboard unavailable: {error}"); return}
            }
        }
        let clipboard = clipboard.as_mut().unwrap();
        let result = match data {
            Data::Text(text) => clipboard.set_text(text),
            Data::Image(image) => clipboard.set_image(arboard::ImageData {
                width: image.width() as usize, height: image.height() as usize,
                bytes: Cow::Borrowed(image.as_raw()),
            }),
            Data::File(path) => clipboard.set().file_list(&[path]),
            Data::Media(_) => {
                log::warn!("Linux audio/video clipboard support is not implemented");
                return
            }
        };
        if let Err(error) = result {log::warn!("Clipboard write failed: {error}");}
    }

    pub fn get(&self) -> Option<Data> {
        let mut clipboard = self.0.borrow_mut();
        if clipboard.is_none() {
            match arboard::Clipboard::new() {
                Ok(value) => *clipboard = Some(value),
                Err(error) => {log::warn!("Clipboard unavailable: {error}"); return None}
            }
        }
        let clipboard = clipboard.as_mut().unwrap();
        if let Ok(files) = clipboard.get().file_list() {
            // Data represents one file; do not silently discard the rest of a file list.
            if files.len() == 1 {return Some(Data::File(files.into_iter().next().unwrap()))}
        }
        if let Ok(image) = clipboard.get_image() {
            let image = image::RgbaImage::from_raw(
                image.width.try_into().unwrap(), image.height.try_into().unwrap(),
                image.bytes.into_owned(),
            ).expect("Clipboard image dimensions must match its pixels");
            return Some(Data::Image(image.into()))
        }
        clipboard.get_text().ok().map(Data::Text)
    }
}

pub struct Haptics;
impl Haptics {
    pub fn new() -> Self {Self}
    pub fn trigger(&self) {}
}

pub struct Push;
impl Push {
    pub async fn current(_previous: Option<&PushDestination>) -> PushDestination {
        // There is no Linux push provider yet; an unavailable destination remains pending.
        std::future::pending().await
    }

    pub fn notify(_proxy: EventLoopProxy<Output>, header: String, body: String) {
        RUNTIME.get().unwrap().block_on(async move {
            tokio::spawn(async move {
                let result = async {
                    let portal = NotificationProxy::new().await?;
                    portal.add_notification(
                        &format!("{:032x}", rand::random::<u128>()),
                        Notification::new(&header).body(body.as_str()),
                    ).await
                }.await;
                if let Err(error) = result {log::warn!("Notification unavailable: {error}");}
            });
        });
    }
}

pub struct Share {
    directory: tempfile::TempDir,
}
impl Share {
    pub fn new() -> Self {Self {directory: tempfile::tempdir().unwrap()}}

    pub fn share(&self, data: &Data) {
        let path = self.directory.path().join(format!("{:032x}", rand::random::<u128>()));
        let file = match data {
            Data::File(path) => std::fs::File::open(path),
            Data::Text(text) => {
                let path = path.with_extension("txt");
                std::fs::write(&path, text).and_then(|_| std::fs::File::open(path))
            }
            Data::Image(image) => {
                let path = path.with_extension("png");
                image.save_with_format(&path, image::ImageFormat::Png)
                    .map_err(std::io::Error::other).and_then(|_| std::fs::File::open(path))
            }
            Data::Media(_) => {
                log::warn!("Linux audio/video sharing is not implemented");
                return
            }
        };
        let file = match file {
            Ok(file) => file,
            Err(error) => {log::warn!("Could not prepare shared file: {error}"); return}
        };
        RUNTIME.get().unwrap().block_on(async move {
            tokio::spawn(async move {
                // Linux offers an application chooser rather than Apple's share sheet.
                let result = OpenFileRequest::default().ask(true).send_file(&file).await
                    .and_then(|request| request.response());
                if let Err(error) = result {log::warn!("Could not open shared file: {error}");}
            });
        });
    }
}

pub struct DataPicker {
    tx: AsyncTx<spsc::List<PickRequest>>,
    changed: Arc<Notify>,
    worker: tokio::task::JoinHandle<()>,
}
impl DataPicker {
    pub fn new() -> Self {
        let (tx, rx): (AsyncTx<spsc::List<PickRequest>>, crossfire::AsyncRx<_>) =
            spsc::build(spsc::List::new());
        let changed = Arc::new(Notify::new());
        let cancel = changed.clone();
        let worker = RUNTIME.get().unwrap().block_on(async move {
            tokio::spawn(async move {
                while let Ok(request) = rx.recv().await {
                    Self::select(request, &cancel).await;
                }
            })
        });
        Self {tx, changed, worker}
    }

    pub fn pick<P: Pickable>(&self, hardware: &Hardware) -> Picked<P> {
        let (tx, rx): (AsyncTx<spsc::One<PickResult>>, _) = spsc::build(spsc::One::new());
        self.tx.try_send(P::request(tx)).unwrap();
        Picked::new(rx, hardware)
    }

    pub fn cancel(&self) {self.changed.notify_one();}

    async fn select(mut request: PickRequest, changed: &Notify) {
        if request.disconnected() {return}
        let mut title = "Choose files".to_owned();
        loop {
            if request.disconnected() {return}
            // Each active request owns a connection; disconnecting cancels only its portal dialog.
            let connection = match ashpd::zbus::Connection::session().await {
                Ok(connection) => connection,
                Err(error) => {log::warn!("File chooser unavailable: {error}"); request.cancel(); return}
            };
            let mut open = SelectedFiles::open_file().title(&*title).multiple(request.multiple())
                .modal(false).connection(Some(connection.clone()));
            let filter = match request.kind() {
                PickKind::Image => Some(FileFilter::new("Images").mimetype("image/*")),
                PickKind::Video => Some(FileFilter::new("Videos").mimetype("video/*")),
                PickKind::Audio => Some(FileFilter::new("Audio").mimetype("audio/*")),
                PickKind::Visual => Some(FileFilter::new("Images and videos")
                    .mimetype("image/*").mimetype("video/*")),
                PickKind::Media => Some(FileFilter::new("Audio and video")
                    .mimetype("audio/*").mimetype("video/*")),
                PickKind::File | PickKind::Data => None,
            };
            if let Some(filter) = filter {open = open.filters([filter]);}
            let result = {
                let selection = open.send();
                tokio::pin!(selection);
                loop {
                    tokio::select! {
                        biased;
                        _ = changed.notified() => {
                            if request.disconnected() {break None}
                        }
                        response = &mut selection => break Some(response.and_then(|r| r.response())),
                    }
                }
            };
            let _ = connection.close().await;
            if request.disconnected() {return}
            let files = match result {
                Some(Ok(files)) if !files.uris().is_empty() => files,
                Some(Err(error)) => {
                    log::debug!("File chooser ended: {error}");
                    request.cancel();
                    return
                }
                _ => {request.cancel(); return}
            };
            let items = files.uris().iter().map(|uri| {
                let path = url::Url::parse(&uri.to_string()).map_err(|error| error.to_string())?
                    .to_file_path().map_err(|_| "The selection is not a local file".to_owned())?;
                Self::load(request.kind(), &path)
                    .map_err(|error| format!("{}: {error}", path.display()))
            }).collect();
            match request.accept(items) {
                None => {request.finish(); return}
                Some(error) => {
                    log::warn!("{error}");
                    title = format!("{error}\nChoose replacement files");
                }
            }
        }
    }

    fn load(kind: PickKind, path: &Path) -> Result<Data, String> {
        if matches!(kind, PickKind::File) {return Ok(Data::File(path.to_owned()))}
        let reader = image::io::Reader::open(path).map_err(|error| error.to_string())?
            .with_guessed_format().map_err(|error| error.to_string())?;
        if reader.format().is_some() || matches!(kind, PickKind::Image) {
            return Image::read(reader.into_inner()).map(Data::Image)
        }
        if kind.media() && !matches!(kind, PickKind::Data) {
            return std::fs::File::open(path).map_err(|error| error.to_string())
                .and_then(Media::read).map(Data::Media)
        }
        Ok(Data::File(path.to_owned()))
    }
}
impl Drop for DataPicker {
    fn drop(&mut self) {
        self.worker.abort();
        RUNTIME.get().unwrap().block_on(async {
            if let Err(error) = (&mut self.worker).await {
                assert!(error.is_cancelled(), "Picker failed: {error}");
            }
        });
    }
}

// Media adapters remain deferred while the shared media API is being refined.
pub struct AudioSession;
impl AudioSession {
    pub fn new() -> Result<Self, String> {Ok(Self)}
}

pub struct MediaCommands;
impl MediaCommands {
    pub fn update() {}
}

#[derive(Clone)]
pub struct Camera(std::convert::Infallible);
impl Camera {
    pub fn error(&self) -> Option<String> {match self.0 {}}
}
impl AsRef<Media> for Camera {
    fn as_ref(&self) -> &Media {match self.0 {}}
}

#[derive(Clone)]
pub struct Microphone(std::convert::Infallible);
impl Microphone {
    pub fn error(&self) -> Option<String> {match self.0 {}}
}
impl AsRef<Media> for Microphone {
    fn as_ref(&self) -> &Media {match self.0 {}}
}

#[derive(Clone)]
pub struct Capture(std::convert::Infallible);
impl Capture {
    pub fn error(&self) -> Option<String> {match self.0 {}}
}
impl AsRef<Media> for Capture {
    fn as_ref(&self) -> &Media {match self.0 {}}
}

pub struct CaptureHub;
impl CaptureHub {
    pub fn new(_hardware: &Hardware) -> Self {Self}
    pub fn camera(&self) -> Result<Camera, String> {
        Err("Linux video capture is not implemented".into())
    }
    pub fn microphone(&self) -> Result<Microphone, String> {
        Err("Linux audio capture is not implemented".into())
    }
    pub fn capture(&self) -> Result<Capture, String> {
        Err("Linux video capture is not implemented".into())
    }
}
