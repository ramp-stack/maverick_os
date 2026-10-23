use serde::{Deserialize, Deserializer, Serialize, Serializer};

use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::shared::{Service, Services, Sharable, Shared};
use super::{Image, Media, PlaybackMode, Reader, VideoFrame};
use super::playback::{Clock, Request};
use super::speaker::Session as SpeakerSession;

#[derive(Clone, Debug)]
pub struct Init {
    id: u128,
    source: Media,
    image: Option<Image>,
    clock: Clock,
}
impl Hash for Init {
    fn hash<H: Hasher>(&self, state: &mut H) {self.id.hash(state)}
}

#[derive(Clone, Debug)]
pub struct Frame {
    pub image: Option<Image>,
    pub position: Duration,
    pub duration: Duration,
    pub mode: PlaybackMode,
    pub speed: f64,
    pub volume: f64,
    pub playing: bool,
    pub error: Option<String>,
}
impl Serialize for Frame {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let image = self.image.as_ref().map(|image| {
            let rgba = image.as_ref();
            (rgba.width(), rgba.height(), rgba.as_raw())
        });
        (
            image, self.position, self.duration, self.mode,
            self.speed, self.volume, self.playing, &self.error,
        ).serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for Frame {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let (image, position, duration, mode, speed, volume, playing, error) =
            <(Option<(u32, u32, Vec<u8>)>, _, _, _, _, _, _, _)>::deserialize(deserializer)?;
        let image = image.map(|(width, height, pixels)| {
            image::RgbaImage::from_raw(width, height, pixels).filter(|image| {
                image.width() > 0 && image.height() > 0
            }).map(Image::from)
                .ok_or_else(|| <D::Error as serde::de::Error>::custom("Invalid cached media frame"))
        }).transpose()?;
        Ok(Self {image, position, duration, mode, speed, volume, playing, error})
    }
}
impl Sharable for Frame {
    type Init = Init;
    type Request = (Clock, Option<(Option<Image>, u64, u64)>);
    type Response = Self;

    fn persistent() -> bool {false}
    fn capacity() -> usize {8}
    async fn init(init: Init) -> Self {init.clock.frame(init.image, None)}
    async fn handle(&mut self, (clock, image): Self::Request) -> Self {
        *self = clock.frame(self.image.clone(), image);
        self.clone()
    }
}

#[derive(Clone, Debug)]
pub struct Player {
    clock: Clock,
    _session: Arc<Session>,
    source: Media,
    frames: Shared<Frame>,
}
impl Player {
    pub fn new(source: impl AsRef<Media>) -> Result<Self, String> {
        let source = source.as_ref().clone();
        let clock = Clock::new(Some(source.duration()));
        clock.source(source.0.status());
        let image = source.0.video_reader().video()?.filter(|frame| frame.position.is_zero())
            .map(|frame| frame.image);
        let init = Init {id: rand::random(), source: source.clone(), image, clock: clock.clone()};
        let frames = Shared::new(init.clone());
        let session = Arc::new(Session {state: frames.clone(), init: init.clone()});
        Services::start::<PlayerService>(init);
        Ok(Self {clock, _session: session, source, frames})
    }
    pub fn source(&self) -> &Media {&self.source}
    pub fn set_mode(&self, mode: PlaybackMode) {
        self.clock.source(self.source.0.status());
        self.clock.set_mode(mode);
        self.publish();
    }
    pub fn error(&self) -> Option<String> {self.clock.error()}
    pub fn start(&self) {
        self.clock.source(self.source.0.status());
        self.clock.start();
        self.publish();
    }
    pub fn stop(&self) {self.clock.stop(); self.publish();}
    pub fn seek(&self, position: Duration) {
        self.clock.source(self.source.0.status());
        self.clock.seek(position);
        self.publish();
    }
    pub fn mode(&self) -> PlaybackMode {self.clock.mode()}
    pub fn set_speed(&self, speed: f64) {self.clock.set_speed(speed); self.publish();}
    pub fn speed(&self) -> f64 {self.clock.speed()}
    pub fn set_volume(&self, volume: f64) {self.clock.set_volume(volume); self.publish();}
    pub fn volume(&self) -> f64 {self.clock.volume()}
    pub fn position(&self) -> Duration {self.clock.position()}
    pub fn duration(&self) -> Duration {self.source.duration()}
    pub fn is_playing(&self) -> bool {self.clock.is_playing()}
    pub fn frame(&self) -> Frame {self.frames.load().as_ref().clone()}
    pub async fn next(&mut self) -> Frame {self.frames.next().await}

    fn publish(&self) {self._session.state.update((self.clock.clone(), None));}
}

#[derive(Debug)]
struct Session {
    state: Shared<Frame>,
    init: Init,
}
impl Drop for Session {
    fn drop(&mut self) {
        Services::shutdown::<PlayerService>(&self.init);
        self.state.shutdown();
    }
}

struct PlayerService {
    state: Shared<Frame>,
    source: Media,
    clock: Clock,
    speaker: Option<Arc<SpeakerSession>>,
    reader: Reader,
    next: Option<VideoFrame>,
    revision: u64,
    cycle: u64,
    edit: u64,
    target: bool,
    progress: Instant,
}
impl PlayerService {
    fn publish(&mut self, image: Option<Image>) {
        let previous = self.state.load();
        let image = (image, self.revision, self.cycle);
        let frame = self.clock.frame(previous.image.clone(), Some(image.clone()));
        let changed = match (&frame.image, &previous.image) {
            (Some(image), Some(old)) => image != old,
            (None, None) => false,
            _ => true,
        } || frame.duration != previous.duration || frame.mode != previous.mode
            || frame.speed != previous.speed || frame.volume != previous.volume
            || frame.playing != previous.playing || frame.error != previous.error;
        let progress = frame.position != previous.position && (
            self.progress.elapsed() >= Duration::from_millis(100)
                || !frame.playing || frame.position >= frame.duration
        );
        drop(previous);
        if changed || progress {
            self.state.update((self.clock.clone(), Some(image)));
            self.progress = Instant::now();
        }
    }
}
impl Service for PlayerService {
    type Init = Init;
    type Predicate = ();

    async fn init(init: Init) -> Self {
        let state = Shared::new(init.clone());
        let reader = init.source.0.video_reader();
        Self {
            state, source: init.source, clock: init.clock, speaker: None,
            reader, next: None, revision: u64::MAX, cycle: 0, edit: 0,
            target: true, progress: Instant::now(),
        }
    }
    async fn predicate(&mut self) {
        tokio::select! {
            _ = self.clock.next() => {},
            _ = tokio::time::sleep(Duration::from_millis(5)) => {},
        }
    }
    async fn handle(&mut self, _: ()) {
        let source = &self.source.0;
        let image = self.state.load().image.clone();
        if let Some(error) = source.error() {
            if self.clock.error().is_none() {self.clock.fail(self.clock.time().revision, error);}
            self.publish(image);
            return
        }
        let status = source.status();
        self.clock.source(status);
        if status.audio && self.speaker.is_none() && self.clock.error().is_none() {
            let result = tokio::task::block_in_place(|| {
                SpeakerSession::new(&self.source, self.clock.clone())
            });
            match result {
                Ok(speaker) => self.speaker = Some(speaker),
                Err(error) => {
                    self.clock.fail(self.clock.time().revision, error);
                    self.publish(image);
                    return
                }
            }
        } else if !status.audio && self.speaker.take().is_some() {
            self.clock.shared().update(Request::Ready(None, Some(status.duration), None));
        }
        let time = self.clock.time();
        let changed = time.revision != self.revision || time.cycle != self.cycle
            || status.edit != self.edit;
        if changed {
            self.reader.seek(time.position);
            self.next = None;
            self.revision = time.revision;
            self.cycle = time.cycle;
            self.edit = status.edit;
            self.target = true;
        }
        if source.dimensions().is_none() {
            self.publish(None);
            return
        }
        if !time.playing && !self.target {self.publish(image); return}
        let result = tokio::task::block_in_place(|| {
            let mut image = None;
            for _ in 0..64 {
                if self.next.is_none() {self.next = self.reader.video()?;}
                let Some(frame) = self.next.as_ref() else {break};
                if frame.position > time.position {break}
                image = self.next.take().map(|frame| frame.image);
            }
            if image.is_none() && changed && time.position > status.duration {
                let mut reader = source.video_reader();
                reader.seek(status.duration);
                image = reader.video()?.map(|frame| frame.image);
            }
            Ok::<_, String>(image)
        });
        if source.status().edit != self.edit {return}
        let current = self.clock.time();
        if current.revision != self.revision || current.cycle != self.cycle {return}
        match result {
            Ok(Some(image)) => {
                self.publish(Some(image));
                self.target = time.position >= status.available;
            }
            Ok(None) if changed => self.publish(None),
            Ok(None) => self.publish(image),
            Err(error) => {
                let placeholder = image::load_from_memory(include_bytes!("corrupted.png")).unwrap();
                self.clock.fail(self.revision, error);
                self.publish(Some(placeholder.into_rgba8().into()));
                self.target = false;
            }
        }
    }
}
