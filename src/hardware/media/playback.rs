use serde::{Deserialize, Deserializer, Serialize, Serializer};

use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use super::super::MediaCommands;
use super::{Image, PlaybackMode, Status};
use super::player::Frame;
use crate::shared::{Sharable, Shared};

static PLAYING: LazyLock<Mutex<HashMap<u128, bool>>> = LazyLock::new(Default::default);

#[derive(Clone, Copy, Debug)]
pub struct PlaybackTime {
    pub position: Duration,
    pub revision: u64,
    pub cycle: u64,
    pub playing: bool,
}

#[derive(Debug)]
pub struct OutputSpan {
    pub at: Instant,
    pub position: f64,
    pub end: f64,
    pub speed: f64,
    pub revision: u64,
    pub loop_duration: Option<f64>,
}

#[derive(Debug)]
pub struct OutputClock {
    pub revision: u64,
    pub edit: u64,
    pub submitted: f64,
    pub presented: f64,
    pub visible_revision: u64,
    pub cycle: u64,
    pub loop_duration: Option<f64>,
    pub playing: bool,
    pub speed: f64,
    pub volume: f64,
    pub drained: bool,
    pub limit: f64,
    pub segments: VecDeque<OutputSpan>,
}
impl OutputClock {
    pub fn submitted_position(&self) -> f64 {
        self.loop_duration.map_or(self.submitted, |duration| self.submitted % duration)
    }
    pub fn time(&mut self) -> PlaybackTime {
        let now = Instant::now();
        while self.segments.get(1).is_some_and(|segment| segment.at <= now) {
            self.segments.pop_front();
        }
        if let Some(segment) = self.segments.front().filter(|segment| segment.at <= now) {
            let elapsed = now.duration_since(segment.at).as_secs_f64() * segment.speed;
            let position = (segment.position + elapsed).min(segment.end);
            self.presented = segment.loop_duration.map_or(position, |duration| position % duration);
            self.cycle = segment.loop_duration.map_or(0, |duration| (position / duration) as u64);
            self.visible_revision = segment.revision;
        }
        PlaybackTime {
            position: Duration::try_from_secs_f64(self.presented).unwrap_or(Duration::MAX),
            revision: self.visible_revision, cycle: self.cycle, playing: self.playing,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Clock {
    state: Shared<Playback>,
    _session: Arc<Session>,
}
impl Clock {
    pub fn new(duration: Option<Duration>) -> Self {
        let init = Init {id: rand::random(), duration};
        let state = Shared::new(init.clone());
        let session = Arc::new(Session {state: state.clone(), id: init.id});
        Self {state, _session: session}
    }
    pub fn shared(&self) -> Shared<Playback> {self.state.clone()}
    pub fn start(&self) {self.state.update(Request::Start);}
    pub fn stop(&self) {self.state.update(Request::Stop);}
    pub fn seek(&self, position: Duration) {self.state.update(Request::Seek(position));}
    pub fn set_mode(&self, mode: PlaybackMode) {self.state.update(Request::Mode(mode));}
    pub fn mode(&self) -> PlaybackMode {self.state.load().mode}
    pub fn source(&self, source: Status) {
        if source.revision > self.state.load().source_revision {
            self.state.update(Request::Source(source));
        }
        let state = self.state.load();
        if state.should_loop() {self.state.update(Request::Loop(state.revision));}
    }
    pub fn set_speed(&self, speed: f64) {
        assert!(speed.is_finite() && speed > 0.0, "Playback speed must be finite and positive");
        self.state.update(Request::Speed(speed));
    }
    pub fn speed(&self) -> f64 {self.state.load().speed}
    pub fn set_volume(&self, volume: f64) {
        assert!(volume.is_finite() && volume >= 0.0, "Volume must be finite and nonnegative");
        self.state.update(Request::Volume(volume));
    }
    pub fn volume(&self) -> f64 {self.state.load().volume}
    pub fn position(&self) -> Duration {self.time().position}
    pub fn is_playing(&self) -> bool {self.state.load().playing}
    pub fn error(&self) -> Option<String> {self.state.load().error.clone()}
    pub fn playing() -> Option<bool> {
        let players = PLAYING.lock().unwrap();
        (!players.is_empty()).then(|| players.values().any(|playing| *playing))
    }
    pub fn time(&self) -> PlaybackTime {self.state.load().time()}
    pub async fn next(&mut self) {self.state.next().await;}
    pub fn frame(&self, image: Option<Image>, replacement: Option<(Option<Image>, u64, u64)>) -> Frame {
        let state = self.state.load();
        let time = state.time();
        let image = match replacement {
            Some((replacement, revision, cycle)) if revision == time.revision && cycle == time.cycle => {
                replacement
            }
            _ => image,
        };
        Frame {
            image, position: time.position, duration: state.duration.unwrap_or_default(),
            mode: state.mode, speed: state.speed, volume: state.volume,
            playing: state.playing, error: state.error.clone(),
        }
    }
    pub fn fail(&self, revision: u64, error: String) {
        self.state.update(Request::Error(revision, error));
    }
}

#[derive(Clone, Debug)]
pub struct Init {
    id: u128,
    duration: Option<Duration>,
}
impl Hash for Init {
    fn hash<H: Hasher>(&self, state: &mut H) {self.id.hash(state)}
}

pub enum Request {
    Start, Stop, Speed(f64), Volume(f64), Seek(Duration), Seeked(u64),
    Source(Status), Mode(PlaybackMode), Loop(u64),
    Ready(Option<Arc<Mutex<OutputClock>>>, Option<Duration>, Option<String>),
    Error(u64, String),
}

#[derive(Clone, Debug)]
pub struct Playback {
    id: u128,
    clock: Option<Arc<Mutex<OutputClock>>>,
    pub position: f64,
    pub duration: Option<Duration>,
    pub speed: f64,
    volume: f64,
    anchor: Instant,
    pub playing: bool,
    pub seek: Option<f64>,
    pub revision: u64,
    available: Duration,
    source_revision: u64,
    source_edit: u64,
    mode: PlaybackMode,
    pub ready: bool,
    pub error: Option<String>,
}
impl Playback {
    fn should_loop(&self) -> bool {
        self.mode == PlaybackMode::Loop && self.clock.is_none() && self.playing
            && !self.available.is_zero() && self.time().position >= self.available
    }
    fn time(&self) -> PlaybackTime {
        let mut time = if let Some(clock) = &self.clock {clock.lock().unwrap().time()} else {
            let elapsed = if self.playing {
                self.anchor.elapsed().as_secs_f64() * self.speed
            } else {0.0};
            PlaybackTime {
                position: Duration::try_from_secs_f64(
                    (self.position + elapsed).min(self.available.as_secs_f64().max(self.position)),
                ).unwrap_or(Duration::MAX),
                revision: self.revision, cycle: 0, playing: self.playing,
            }
        };
        time.playing = self.playing;
        time
    }
}
impl Serialize for Playback {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        (self.time().position, self.duration, self.speed, self.volume).serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for Playback {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let (position, duration, speed, volume) = <(Duration, _, _, _)>::deserialize(deserializer)?;
        Ok(Self {
            id: 0, clock: None, position: position.as_secs_f64(), duration, speed, volume,
            anchor: Instant::now(), playing: false, seek: None, revision: 0,
            available: duration.unwrap_or(Duration::MAX), source_revision: 0, source_edit: 0,
            mode: PlaybackMode::Default, ready: false, error: None,
        })
    }
}
impl Sharable for Playback {
    type Init = Init;
    type Request = Request;
    type Response = bool;

    fn persistent() -> bool {false}
    async fn init(Init {id, duration}: Init) -> Self {
        PLAYING.lock().unwrap().insert(id, false);
        Self {
            id, clock: None, position: 0.0, duration, speed: 1.0, volume: 1.0,
            anchor: Instant::now(), playing: false, seek: None, revision: 0,
            available: duration.unwrap_or(Duration::ZERO), source_revision: 0, source_edit: 0,
            mode: PlaybackMode::Default, ready: false, error: None,
        }
    }
    async fn handle(&mut self, request: Request) -> bool {
        let was_playing = self.playing;
        match request {
            Request::Start => {
                if self.playing {return true}
                if self.error.is_some() {
                    self.seek = self.clock.as_ref().map(|_| self.position);
                    self.revision += 1;
                    self.error = None;
                }
                if let PlaybackMode::Live(delay) = self.mode {
                    self.position = self.available.saturating_sub(delay).as_secs_f64();
                    self.seek = self.clock.as_ref().map(|_| self.position);
                    self.revision += 1;
                }
                self.playing = true;
                self.anchor = Instant::now();
            }
            Request::Stop => {
                if !self.playing {return true}
                self.position = self.time().position.as_secs_f64();
                self.playing = false;
                self.revision += 1;
                self.seek = self.clock.as_ref().map(|_| self.position);
            }
            Request::Speed(speed) => {
                self.position = if let Some(clock) = &self.clock {
                    let mut clock = clock.lock().unwrap();
                    if self.playing {clock.submitted_position()} else {clock.time().position.as_secs_f64()}
                } else {self.time().position.as_secs_f64()};
                self.anchor = Instant::now();
                self.speed = speed;
                self.revision += 1;
                self.seek = self.clock.as_ref().map(|_| self.position);
            }
            Request::Volume(volume) => {self.volume = volume;}
            Request::Seek(position) => {
                if matches!(self.mode, PlaybackMode::Live(_)) {self.mode = PlaybackMode::Default;}
                self.position = self.duration.map_or(position, |end| position.min(end))
                    .as_secs_f64();
                self.anchor = Instant::now();
                self.seek = self.clock.as_ref().map(|_| self.position);
                self.revision += 1;
                self.error = None;
            }
            Request::Source(source) => {
                if source.revision <= self.source_revision {return false}
                let appended = source.available > self.available && source.edit == self.source_edit;
                let loop_changed = self.mode == PlaybackMode::Loop
                    && (source.available != self.available || source.edit != self.source_edit);
                self.source_revision = source.revision;
                self.position = if loop_changed && let Some(clock) = &self.clock {
                    clock.lock().unwrap().submitted_position()
                } else {self.time().position.as_secs_f64()};
                self.anchor = Instant::now();
                if loop_changed {
                    self.revision += 1;
                    self.seek = self.clock.as_ref().map(|_| self.position);
                }
                self.duration = Some(source.duration);
                self.available = source.available;
                self.source_edit = source.edit;
                if let Some(clock) = &self.clock {
                    let mut clock = clock.lock().unwrap();
                    clock.drained = false;
                    clock.edit = source.edit;
                }
                if let PlaybackMode::Live(delay) = self.mode && self.playing && appended {
                    let target = source.available.saturating_sub(delay).as_secs_f64();
                    let queued = self.seek.unwrap_or_else(|| {
                        self.clock.as_ref().map_or(self.position, |clock| {
                            clock.lock().unwrap().submitted
                        })
                    });
                    if target > queued + 0.1 {
                        self.position = target;
                        self.seek = self.clock.as_ref().map(|_| target);
                        self.revision += 1;
                    }
                }
            }
            Request::Mode(mode) => {
                if mode == self.mode {return false}
                self.position = if self.playing && let Some(clock) = &self.clock {
                    clock.lock().unwrap().submitted_position()
                } else {self.time().position.as_secs_f64()};
                self.anchor = Instant::now();
                self.mode = mode;
                if let PlaybackMode::Live(delay) = mode {
                    self.position = self.available.saturating_sub(delay).as_secs_f64();
                }
                self.seek = self.clock.as_ref().map(|_| self.position);
                self.revision += 1;
            }
            Request::Loop(revision) => {
                if revision != self.revision || !self.should_loop() {return false}
                let now = Instant::now();
                let elapsed = now.duration_since(self.anchor).as_secs_f64() * self.speed;
                self.position = (self.position + elapsed) % self.available.as_secs_f64();
                self.anchor = now;
                self.revision += 1;
            }
            Request::Ready(clock, duration, error) => {
                self.position = self.time().position.as_secs_f64();
                self.anchor = Instant::now();
                self.ready = true;
                self.clock = clock;
                self.revision += 1;
                self.seek = self.clock.as_ref().map(|_| self.position);
                if let Some(clock) = &self.clock {
                    let mut clock = clock.lock().unwrap();
                    clock.presented = self.position;
                    clock.visible_revision = self.revision;
                    clock.cycle = 0;
                    clock.edit = self.source_edit;
                }
                self.duration = self.duration.into_iter().chain(duration).max();
                self.error = error;
            }
            Request::Seeked(revision) => {
                if revision != self.revision {return false}
                self.seek = None;
            }
            Request::Error(revision, error) => {
                if revision != self.revision {return false}
                self.position = self.time().position.as_secs_f64();
                self.playing = false;
                self.seek = None;
                self.error = Some(error);
            }
        }
        if let Some(clock) = &self.clock {
            let mut clock = clock.lock().unwrap();
            if clock.revision != self.revision {
                clock.time();
                clock.revision = self.revision;
                clock.submitted = self.position;
                clock.drained = false;
            }
            if !self.playing {
                clock.segments.clear();
                clock.presented = self.position;
                clock.visible_revision = self.revision;
                clock.cycle = 0;
            }
            let available = self.available.as_secs_f64();
            clock.loop_duration = if self.mode == PlaybackMode::Loop && available > 0.0 {
                if clock.limit == available {Some(clock.loop_duration.unwrap_or(available))}
                else {Some(available)}
            } else {None};
            clock.playing = self.playing && self.seek.is_none();
            clock.speed = self.speed;
            clock.volume = self.volume;
            clock.limit = self.available.as_secs_f64();
        }
        if self.playing != was_playing {
            *PLAYING.lock().unwrap().get_mut(&self.id).unwrap() = self.playing;
            MediaCommands::update();
        }
        true
    }
}

#[derive(Debug)]
struct Session {
    state: Shared<Playback>,
    id: u128,
}
impl Drop for Session {
    fn drop(&mut self) {
        self.state.shutdown();
        PLAYING.lock().unwrap().remove(&self.id);
        MediaCommands::update();
    }
}
