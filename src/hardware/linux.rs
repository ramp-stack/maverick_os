use ashpd::desktop::file_chooser::{FileFilter, SelectedFiles};
use ashpd::desktop::notification::{Notification, NotificationProxy};
use ashpd::desktop::open_uri::OpenFileRequest;
use crossfire::{spsc, AsyncTx};
use gstreamer::{self as gst, prelude::*};
use gstreamer_app::{AppSink, AppSinkCallbacks};
use gstreamer_video::{VideoFrameExt, VideoFrameRef, VideoInfo};
use souvlaki::{MediaControls, MediaControlEvent, MediaPlayback, PlatformConfig};
use tokio::sync::broadcast;
use tokio::sync::Notify;
use winit::event::WindowEvent;
use winit::event_loop::EventLoopProxy;

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use super::{
    Camera, Microphone, Capture, Data, Format, Hardware, Image, Media, Pickable, Picked, PickResult,
    PushToken, Sample, Sound,
};
use super::capture::{Feed, Frame};
use super::picker::{PickKind, PickRequest};
use crate::events::Output;
use crate::inputs::Input;
use crate::runtime::RUNTIME;

static MEDIA_COMMANDS: LazyLock<Mutex<(Option<MediaControls>, HashMap<u128, bool>)>> =
    LazyLock::new(|| Mutex::new((None, HashMap::new())));

#[derive(Clone)]
pub struct Keyboard;
impl Keyboard {
    pub(crate) fn new(_proxy: EventLoopProxy<Output>, _inset: Arc<KeyboardInset>) -> Self {Self}
    pub fn show(&self, _padding: f32) {}
    pub fn hide(&self) {}
    pub fn visible(&self) -> bool {false}
}

pub struct SafeArea;
impl SafeArea {
    pub fn is_mobile() -> bool {false}
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
    pub fn new(_window: &crate::Window, proxy: EventLoopProxy<Output>) -> Self {
        MediaCommands::start(proxy);
        Self
    }
    pub fn process(&mut self, event: WindowEvent, _scale: f64) -> Option<WindowEvent> {Some(event)}
    pub fn suspend(&mut self) {}
    pub async fn next(&mut self) -> Input {std::future::pending().await}
}

impl Drop for Events {
    fn drop(&mut self) {
        let mut state = MEDIA_COMMANDS.lock().unwrap();
        state.0.take();
        state.1.clear();
    }
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

pub struct Clipboard {
    clipboard: RefCell<Option<arboard::Clipboard>>,
    media: RefCell<Option<tempfile::NamedTempFile>>,
}
impl Clipboard {
    pub fn new() -> Self {
        Self {clipboard: RefCell::new(None), media: RefCell::new(None)}
    }

    pub fn set(&self, data: &Data) {
        let mut clipboard = self.clipboard.borrow_mut();
        if clipboard.is_none() {
            match arboard::Clipboard::new() {
                Ok(value) => *clipboard = Some(value),
                Err(error) => {log::warn!("Clipboard unavailable: {error}"); return}
            }
        }
        let clipboard = clipboard.as_mut().unwrap();
        let result = match data {
            Data::Text(text) => clipboard.set_text(text),
            Data::Image(image) => {
                let (width, height) = image.dimensions;
                clipboard.set_image(arboard::ImageData {
                    width: width as usize, height: height as usize,
                    bytes: Cow::Borrowed(image.as_ref()),
                })
            }
            Data::File(path) => clipboard.set().file_list(&[path]),
            Data::Media(media) => {
                let format = if media.video.dimensions().is_some() {
                    Format::Mkv_Ffv1_PcmF32le
                } else {Format::Wav_PcmF32le};
                let file = match tempfile::Builder::new().suffix(&format!(".{}", format.extension())).tempfile() {
                    Ok(file) => file,
                    Err(error) => {log::warn!("Clipboard export failed: {error}"); return}
                };
                let result = file.reopen().map_err(|error| error.to_string())
                    .and_then(|file| media.write(file, format));
                if let Err(error) = result {log::warn!("Clipboard export failed: {error}"); return}
                let result = clipboard.set().file_list(&[file.path()]);
                if result.is_ok() {*self.media.borrow_mut() = Some(file);}
                result
            }
        };
        if let Err(error) = result {log::warn!("Clipboard write failed: {error}");}
        else if !matches!(data, Data::Media(_)) {self.media.borrow_mut().take();}
    }

    pub fn get(&self) -> Option<Data> {
        let mut clipboard = self.clipboard.borrow_mut();
        if clipboard.is_none() {
            match arboard::Clipboard::new() {
                Ok(value) => *clipboard = Some(value),
                Err(error) => {log::warn!("Clipboard unavailable: {error}"); return None}
            }
        }
        let clipboard = clipboard.as_mut().unwrap();
        if let Ok(files) = clipboard.get().file_list() {
            // Data represents one file; do not silently discard the rest of a file list.
            if files.len() == 1 {
                return DataPicker::load(PickKind::Data, &files.into_iter().next().unwrap()).ok()
            }
        }
        if let Ok(image) = clipboard.get_image() {
            let image = Image::new(
                (image.width.try_into().unwrap(), image.height.try_into().unwrap()),
                bytemuck::cast_slice(&image.bytes).to_vec(),
            );
            return Some(Data::Image(image))
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
    pub fn new() -> Self {Self}
    pub async fn next(&mut self) -> PushToken {
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
                std::fs::File::create(&path).and_then(|file| {
                    image.write(file, super::ImageFormat::Png).map_err(std::io::Error::other)?;
                    std::fs::File::open(path)
                })
            }
            Data::Media(media) => {
                let format = if media.video.dimensions().is_some() {
                    Format::Mkv_Ffv1_PcmF32le
                } else {Format::Wav_PcmF32le};
                let path = path.with_extension(format.extension());
                std::fs::File::create(&path).and_then(|file| {
                    media.write(file, format).map_err(std::io::Error::other)?;
                    std::fs::File::open(path)
                })
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
        if kind.media() {
            let result = std::fs::File::open(path).map_err(|error| error.to_string())
                .and_then(Media::read).map(Data::Media);
            if result.is_ok() || !matches!(kind, PickKind::Data) {return result}
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

pub struct AudioSession;
impl AudioSession {
    pub fn new() -> Result<Self, String> {Ok(Self)}
}

pub struct MediaCommands;
impl MediaCommands {
    pub fn update(id: u128, playing: bool) {
        let mut state = MEDIA_COMMANDS.lock().unwrap();
        if state.1.insert(id, playing) != Some(playing) {Self::publish(&mut state);}
    }
    pub fn remove(id: u128) {
        let mut state = MEDIA_COMMANDS.lock().unwrap();
        if state.1.remove(&id).is_some() {Self::publish(&mut state);}
    }
    fn start(proxy: EventLoopProxy<Output>) {
        let name = format!("maverick.instance{}", std::process::id());
        let result = (|| {
            let mut controls = MediaControls::new(PlatformConfig {
                dbus_name: &name, display_name: "Maverick", hwnd: None,
            })?;
            controls.attach(move |event| {
                let event = match event {
                    MediaControlEvent::Play => crate::inputs::Media::Play,
                    MediaControlEvent::Pause | MediaControlEvent::Stop => crate::inputs::Media::Pause,
                    MediaControlEvent::Toggle => crate::inputs::Media::Toggle,
                    _ => return,
                };
                let _ = proxy.send_event(Output::Input(Input::Media(event)));
            })?;
            Ok::<_, souvlaki::Error>(controls)
        })();
        match result {
            Ok(controls) => {
                let mut state = MEDIA_COMMANDS.lock().unwrap();
                state.0 = Some(controls);
                Self::publish(&mut state);
            }
            Err(error) => log::warn!("Desktop media controls unavailable: {error}"),
        }
    }
    fn publish(state: &mut (Option<MediaControls>, HashMap<u128, bool>)) {
        let playback = if state.1.is_empty() {MediaPlayback::Stopped}
            else if state.1.values().any(|playing| *playing) {MediaPlayback::Playing {progress: None}}
            else {MediaPlayback::Paused {progress: None}};
        if let Some(controls) = &mut state.0
            && let Err(error) = controls.set_playback(playback)
        {
            log::warn!("Could not update desktop playback: {error}");
        }
    }
}

pub struct CaptureHub {
    feeds: Arc<Mutex<HashMap<u128, Feed>>>,
    camera: RefCell<Option<CaptureInput>>,
    microphone: RefCell<Option<CaptureInput>>,
}
impl CaptureHub {
    pub fn new(_hardware: &Hardware) -> Self {
        Self {
            feeds: Arc::new(Mutex::new(HashMap::new())),
            camera: RefCell::new(None), microphone: RefCell::new(None),
        }
    }
    pub fn camera(&self, hardware: &Hardware) -> Result<Camera, String> {
        let (id, frames) = self.subscribe(true, false)?;
        Ok(Camera::new(hardware, id, frames))
    }
    pub fn microphone(&self, hardware: &Hardware) -> Result<Microphone, String> {
        let (id, frames) = self.subscribe(false, true)?;
        Ok(Microphone::new(hardware, id, frames))
    }
    pub fn capture(&self, hardware: &Hardware) -> Result<Capture, String> {
        let (id, frames) = self.subscribe(true, true)?;
        Ok(Capture::new(hardware, id, frames))
    }
    pub fn error(&self, id: u128) -> Option<String> {
        self.feeds.lock().unwrap().get(&id).and_then(|feed| feed.error())
    }
    pub fn remove(&self, id: u128) {
        let (camera, microphone) = {
            let mut feeds = self.feeds.lock().unwrap();
            feeds.remove(&id).unwrap();
            let active = feeds.values().filter(|feed| feed.started());
            (active.clone().any(|feed| feed.camera()), active.clone().any(|feed| feed.microphone()))
        };
        if !camera {self.camera.borrow_mut().take();}
        if !microphone {self.microphone.borrow_mut().take();}
    }
    fn subscribe(&self, camera: bool, microphone: bool) -> Result<(u128, broadcast::Receiver<(Duration, Arc<Frame>)>), String> {
        gst::init().map_err(|error| error.to_string())?;
        let (video_active, audio_active) = {
            let feeds = self.feeds.lock().unwrap();
            let active = feeds.values().filter(|feed| feed.started());
            (active.clone().any(|feed| feed.camera()), active.clone().any(|feed| feed.microphone()))
        };
        if !video_active {self.camera.borrow_mut().take();}
        if !audio_active {self.microphone.borrow_mut().take();}
        let id = rand::random();
        let origin = gst::SystemClock::obtain().time().nseconds() as f64 / 1e9;
        let (mut feed, frames) = Feed::new(camera, microphone);
        feed.start(origin);
        self.feeds.lock().unwrap().insert(id, feed);
        let result = (|| {
            if camera && self.camera.borrow().is_none() {
                *self.camera.borrow_mut() = Some(CaptureInput::new(true, self.feeds.clone())?);
            }
            if microphone && self.microphone.borrow().is_none() {
                *self.microphone.borrow_mut() = Some(CaptureInput::new(false, self.feeds.clone())?);
            }
            Ok(())
        })();
        if let Err(error) = result {self.remove(id); return Err(error)}
        Ok((id, frames))
    }
}
impl Drop for CaptureHub {
    fn drop(&mut self) {
        self.camera.get_mut().take();
        self.microphone.get_mut().take();
        self.feeds.lock().unwrap().clear();
    }
}

struct CaptureInput {
    pipeline: gst::Pipeline,
}
impl CaptureInput {
    fn new(camera: bool, feeds: Arc<Mutex<HashMap<u128, Feed>>>) -> Result<Self, String> {
        let source = if camera {
            "autovideosrc name=device ! decodebin ! videoconvert ! video/x-raw,format=RGBA"
        } else {"autoaudiosrc name=device ! audioconvert ! audio/x-raw,format=F32LE,layout=interleaved"};
        let description = format!("{source} ! appsink name=frames sync=false max-buffers=2 drop=true");
        let pipeline = gst::parse::launch(&description).map_err(|error| error.to_string())?
            .downcast::<gst::Pipeline>().unwrap();
        pipeline.use_clock(Some(&gst::SystemClock::obtain()));
        let sink = pipeline.by_name("frames").unwrap().downcast::<AppSink>().unwrap();
        let weak = pipeline.downgrade();
        let output = feeds.clone();
        sink.set_callbacks(AppSinkCallbacks::builder().new_sample(move |sink| {
            let sample = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
            let Some(pipeline) = weak.upgrade() else {return Err(gst::FlowError::Flushing)};
            match Self::frame(&sample, &pipeline, camera) {
                Ok((timestamp, frame)) => {
                    let frame = Arc::new(frame);
                    for feed in output.lock().unwrap().values() {feed.send(timestamp, &frame);}
                    Ok(gst::FlowSuccess::Ok)
                }
                Err(error) => {
                    Self::fail(&output, camera, error);
                    Err(gst::FlowError::Error)
                }
            }
        }).build());
        pipeline.bus().unwrap().set_sync_handler(move |_, message| {
            match message.view() {
                gst::MessageView::Error(error) => Self::fail(&feeds, camera, error.error().to_string()),
                // Autodetect substitutes a test source when no real device can open.
                gst::MessageView::Warning(error)
                    if error.src().is_some_and(|source| source.name() == "device") => {
                    Self::fail(&feeds, camera, error.error().to_string());
                }
                gst::MessageView::Eos(_) => Self::fail(&feeds, camera, "Capture device stopped".into()),
                _ => {},
            }
            gst::BusSyncReply::Drop
        });
        let input = Self {pipeline};
        input.pipeline.set_state(gst::State::Playing).map_err(|error| error.to_string())?;
        Ok(input)
    }
    fn fail(feeds: &Mutex<HashMap<u128, Feed>>, camera: bool, error: String) {
        for feed in feeds.lock().unwrap().values_mut() {
            if if camera {feed.camera()} else {feed.microphone()} {feed.fail(error.clone());}
        }
    }
    fn frame(sample: &gst::Sample, pipeline: &gst::Pipeline, camera: bool) -> Result<(f64, Frame), String> {
        let buffer = sample.buffer().ok_or("Capture returned no buffer")?;
        let caps = sample.caps().ok_or("Capture returned no format")?;
        let segment = sample.segment().unwrap().downcast_ref::<gst::ClockTime>().unwrap();
        let time = segment.to_running_time(buffer.pts()).ok_or("Capture has no timestamp")?;
        let time = pipeline.base_time().unwrap() + time;
        let frame = if camera {
            let info = VideoInfo::from_caps(caps).map_err(|error| error.to_string())?;
            let image = VideoFrameRef::from_buffer_ref_readable(buffer, &info)
                .map_err(|error| error.to_string())?;
            let bytes = image.plane_data(0).map_err(|error| error.to_string())?;
            let stride = image.plane_stride()[0] as usize;
            let mut pixels = Vec::with_capacity(info.width() as usize * info.height() as usize);
            for row in bytes.chunks(stride).take(info.height() as usize) {
                pixels.extend_from_slice(bytemuck::cast_slice(&row[..info.width() as usize * 4]));
            }
            let duration = match buffer.duration() {
                Some(time) => Duration::from_nanos(time.nseconds()),
                None => {
                    let fps = info.fps();
                    Duration::try_from_secs_f64(fps.denom() as f64 / fps.numer() as f64)
                        .map_err(|_| "Camera did not provide frame timing".to_owned())?
                }
            };
            Frame::Image(Image::new((info.width(), info.height()), pixels), duration)
        } else {
            let format = caps.structure(0).unwrap();
            let channels = format.get::<i32>("channels").unwrap() as u16;
            let rate = format.get::<i32>("rate").unwrap();
            let bytes = buffer.map_readable().map_err(|error| error.to_string())?;
            let samples: Vec<_> = bytes.chunks_exact(4)
                .map(|bytes| Sample(f32::from_le_bytes(bytes.try_into().unwrap()))).collect();
            let duration = Duration::from_secs_f64(samples.len() as f64 / channels as f64 / rate as f64);
            Frame::Sound(Sound {channels, samples}, duration)
        };
        Ok((time.nseconds() as f64 / 1e9, frame))
    }
}
impl Drop for CaptureInput {
    fn drop(&mut self) {
        self.pipeline.bus().unwrap().unset_sync_handler();
        self.pipeline.set_state(gst::State::Null).unwrap();
    }
}
