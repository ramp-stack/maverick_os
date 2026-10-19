use super::{Image, Video};
use crate::shared::{Service, Services, Sharable, Shared};
use ffmpeg_next as ffmpeg;
use font8x8::UnicodeFonts;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::hash::{Hash, Hasher};
use std::io::Cursor;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct Player(Arc<Session>);
impl Player {
    pub fn new(video: &Video) -> Result<Self, String> {
        let init = Init {id: rand::random(), video: video.clone()};
        let state = Shared::new(init.clone());
        let mut ready = state.clone();
        let session = Arc::new(Session {state, init: init.clone()});
        Services::start::<PlaybackService>(init);
        loop {
            let snapshot = ready.load();
            if snapshot.ready {
                return snapshot.error.clone().map_or_else(|| Ok(Self(session)), Err)
            }
            drop(snapshot);
            futures::executor::block_on(ready.next());
        }
    }
    pub fn video(&self) -> &Video {&self.0.init.video}
    pub fn start(&self) {self.0.state.update(PlaybackRequest::Start);}
    pub fn stop(&self) {self.0.state.update(PlaybackRequest::Stop);}
    pub fn set_speed(&self, speed: f64) {
        assert!(speed.is_finite() && speed > 0.0, "Playback speed must be finite and positive");
        self.0.state.update(PlaybackRequest::Speed(speed));
    }
    pub fn speed(&self) -> f64 {self.0.state.load().speed}
    pub fn seek(&self, position: Duration) {self.0.state.update(PlaybackRequest::Seek(position));}
    pub fn position(&self) -> Duration {
        let state = self.0.state.load();
        let position = Duration::try_from_secs_f64(state.position()).unwrap_or(Duration::MAX);
        state.duration.map_or(position, |duration| position.min(duration))
    }
    pub fn duration(&self) -> Option<Duration> {self.0.state.load().duration}
    pub fn is_playing(&self) -> bool {self.0.state.load().playing}
    pub fn frame(&self) -> Image {self.0.state.load().frame.clone()}
}

enum PlaybackRequest {
    Start, Stop, Speed(f64), Seek(Duration), Ready(Option<String>),
    Frame {revision: u64, position: f64, frame: Image, seeking: bool},
    Ended(u64, f64), Error(u64, String, Image),
}

enum DecodeError {Corrupt(f64), Failed(ffmpeg::Error)}
impl From<ffmpeg::Error> for DecodeError {
    fn from(error: ffmpeg::Error) -> Self {Self::Failed(error)}
}

#[derive(Clone, Debug)]
struct Init {id: u128, video: Video}
impl Hash for Init {
    fn hash<H: Hasher>(&self, state: &mut H) {self.id.hash(state)}
}

#[derive(Debug)]
struct Session {state: Shared<Playback>, init: Init}
impl Drop for Session {
    fn drop(&mut self) {Services::shutdown::<PlaybackService>(&self.init);}
}

struct Bytes(Arc<Vec<u8>>);
impl AsRef<[u8]> for Bytes {
    fn as_ref(&self) -> &[u8] {self.0.as_slice()}
}

#[derive(Clone, Debug)]
struct Playback {
    frame: Image, position: f64, duration: Option<Duration>, speed: f64, anchor: Instant,
    playing: bool, seek: Option<f64>, revision: u64, ended: bool, ready: bool, error: Option<String>,
}
impl Playback {
    fn position(&self) -> f64 {
        let elapsed = if self.playing && self.seek.is_none() {
            self.anchor.elapsed().as_secs_f64() * self.speed
        } else {0.0};
        (self.position + elapsed).min(Duration::MAX.as_secs_f64())
    }
}
impl Serialize for Playback {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        (
            (self.frame.width(), self.frame.height(), self.frame.as_raw()),
            (self.position(), self.duration, self.speed, self.playing, self.ended), &self.error,
        ).serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for Playback {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let ((width, height, pixels), (position, duration, speed, playing, ended), error) =
            Deserialize::deserialize(deserializer)?;
        let frame = image::RgbaImage::from_raw(width, height, pixels)
            .ok_or_else(|| serde::de::Error::custom("Invalid cached video frame"))?;
        Ok(Self {
            frame: Arc::new(frame), position, duration, speed, anchor: Instant::now(), playing,
            seek: Some(position), revision: 1, ended, ready: false, error,
        })
    }
}
impl Sharable for Playback {
    type Init = Init;
    type Request = PlaybackRequest;
    type Response = bool;

    fn persistent() -> bool {false}
    async fn init(Init {video, ..}: Self::Init) -> Self {
        Self {
            frame: video.thumbnail(), position: 0.0, duration: video.duration(), speed: 1.0,
            anchor: Instant::now(), playing: false, seek: None, revision: 0, ended: false,
            ready: false, error: None,
        }
    }
    async fn handle(&mut self, request: PlaybackRequest) -> bool {
        match request {
            PlaybackRequest::Start => {
                if self.playing {return true}
                let ended = self.ended || self.duration.is_some_and(|d| self.position() >= d.as_secs_f64());
                if ended || self.error.is_some() {
                    if ended {self.position = 0.0;}
                    self.seek = Some(self.position);
                    self.revision += 1;
                    self.ended = false;
                    self.error = None;
                }
                self.playing = true;
                self.anchor = Instant::now();
            }
            PlaybackRequest::Stop => {
                self.position = self.position();
                self.playing = false;
            }
            PlaybackRequest::Speed(speed) => {
                self.position = self.position();
                self.anchor = Instant::now();
                self.speed = speed;
            }
            PlaybackRequest::Seek(position) => {
                let position = self.duration.map_or(position, |duration| position.min(duration));
                self.position = position.as_secs_f64();
                self.anchor = Instant::now();
                self.seek = Some(self.position);
                self.revision += 1;
                self.ended = false;
                self.error = None;
            }
            PlaybackRequest::Ready(error) => {self.ready = true; self.error = error;}
            PlaybackRequest::Frame {revision, position, frame, seeking} => {
                if revision != self.revision {return false}
                if seeking {
                    if self.seek.take().is_none() {return false}
                    self.anchor = Instant::now();
                } else if !self.playing || position > self.position() {return false}
                self.frame = frame;
            }
            PlaybackRequest::Ended(revision, end) => {
                if revision != self.revision || self.seek.is_some() || self.position() < end {
                    return false
                }
                self.position = end;
                self.duration = Duration::try_from_secs_f64(end).ok();
                self.playing = false;
                self.ended = true;
            }
            PlaybackRequest::Error(revision, error, frame) => {
                if revision != self.revision || (!self.playing && self.seek.is_none()) {return false}
                self.position = self.position();
                self.playing = false;
                self.seek = None;
                self.error = Some(error);
                self.frame = frame;
            }
        }
        true
    }
}

struct PlaybackService {
    state: Shared<Playback>, decoder: Option<Decoder>,
    next: Option<(f64, Image)>, target: Option<f64>, last: Option<ffmpeg::frame::Video>,
    revision: u64, ended: bool,
    corrupt_at: Option<f64>, corrupted: bool, placeholder: Image,
}
impl Service for PlaybackService {
    type Init = Init;
    type Predicate = ();

    async fn init(init: Init) -> Self {
        let state = Shared::new(init.clone());
        let decoder = tokio::task::block_in_place(|| Decoder::new(init.video.bytes().clone()));
        let error = decoder.as_ref().err().map(ToString::to_string);
        let (width, height) = init.video.dimensions();
        let (width, height) = (width.max(88), height.max(40));
        let scale = (width / 88).min(height / 40).clamp(1, 4);
        let mut placeholder = image::RgbaImage::from_pixel(
            width, height, image::Rgba([32, 32, 32, 255]),
        );
        for (line, text) in ["This frame", "was", "corrupted"].into_iter().enumerate() {
            let left = (width - text.len() as u32 * 8 * scale) / 2;
            let top = (height - 30 * scale) / 2 + line as u32 * 10 * scale;
            for (column, letter) in text.chars().enumerate() {
                let glyph = font8x8::BASIC_FONTS.get(letter).unwrap();
                for (row, bits) in glyph.into_iter().enumerate() {
                    for bit in 0..8 {
                        if bits & (1 << bit) == 0 {continue}
                        for y in 0..scale {for x in 0..scale {
                            let x = left + (column as u32 * 8 + bit) * scale + x;
                            let y = top + row as u32 * scale + y;
                            placeholder.put_pixel(x, y, image::Rgba([240, 240, 240, 255]));
                        }}
                    }
                }
            }
        }
        let service = Self {
            state, decoder: decoder.ok(), next: None, target: None, last: None,
            revision: 0, ended: false, corrupt_at: None, corrupted: false,
            placeholder: Arc::new(placeholder),
        };
        service.state.update(PlaybackRequest::Ready(error));
        service
    }
    async fn predicate(&mut self) {
        let delay = {
            let state = self.state.load();
            if state.seek.is_some() && state.revision != self.revision {return}
            if self.target.is_some() {return}
            if state.playing {
                let mut position = if self.ended {
                    state.duration.map_or(0.0, |d| d.as_secs_f64())
                        .max(self.decoder.as_ref().map_or(0.0, |decoder| decoder.end))
                } else if let Some((position, _)) = self.next {position} else {return};
                if !self.corrupted {
                    if let Some(corrupt_at) = self.corrupt_at {position = position.min(corrupt_at);}
                }
                let remaining = position - state.position();
                if remaining <= 0.0 {return}
                Some(Duration::from_secs_f64((remaining / state.speed).min(1.0)))
            } else {None}
        };
        if let Some(delay) = delay {
            tokio::select! {
                _ = self.state.next() => {},
                _ = tokio::time::sleep(delay) => {},
            }
        } else {self.state.next().await;}
    }
    async fn handle(&mut self, _: ()) {
        let state = self.state.load();
        let Some(decoder) = self.decoder.as_mut() else {return};
        if let Some(position) = state.seek.filter(|_| state.revision != self.revision) {
            self.revision = state.revision;
            self.target = Some(position);
            self.next = None;
            self.last = None;
            self.ended = false;
            self.corrupt_at = None;
            self.corrupted = false;
            let result = tokio::task::block_in_place(|| {
                let preroll = (position - decoder.interval * 2.0).max(0.0);
                let stamp = ((preroll + decoder.start) * 1_000_000.0) as i64;
                let result = decoder.input.seek(stamp, ..stamp);
                decoder.decoder.flush();
                decoder.draining = false;
                decoder.feed = false;
                decoder.end = preroll;
                result
            });
            if let Err(error) = result {
                self.state.update(PlaybackRequest::Error(
                    self.revision, error.to_string(), self.placeholder.clone(),
                ));
                self.target = None;
            }
            return
        }
        if !state.playing && self.target.is_none() {return}
        let position = self.target.unwrap_or_else(|| state.position());
        let due = self.next.as_ref().is_some_and(|(stamp, _)| *stamp <= position);
        let end = state.duration.map_or(decoder.end, |d| d.as_secs_f64().max(decoder.end));
        let next = self.next.as_ref().map_or(end, |(stamp, _)| *stamp);
        let corrupt_at = self.corrupt_at.filter(|stamp| next > *stamp + 0.000001);
        if (self.next.is_some() || self.ended) && !due && !self.corrupted
            && corrupt_at.is_some_and(|stamp| stamp <= position + 0.000001) {
            if self.state.update(PlaybackRequest::Frame {
                revision: self.revision, position, frame: self.placeholder.clone(),
                seeking: self.target.is_some(),
            }) {
                self.corrupted = true;
                self.target = None;
                self.last = None;
            }
            return
        }
        if self.ended || (self.corrupt_at.is_some() && self.target.is_some_and(|target| {
            self.next.as_ref().is_some_and(|(position, _)| target < *position)
        })) {
            if let Some(target) = self.target {
                let frame = tokio::task::block_in_place(|| {
                    self.last.take().map(|frame| decoder.image(&frame)).transpose()
                });
                let frame = match frame {
                    Ok(frame) => frame.unwrap_or_else(|| state.frame.clone()),
                    Err(error) => {
                        self.state.update(PlaybackRequest::Error(
                            self.revision, error.to_string(), self.placeholder.clone(),
                        ));
                        self.target = None;
                        return
                    }
                };
                let position = if self.ended {decoder.end} else {target};
                self.state.update(PlaybackRequest::Frame {
                    revision: self.revision, position, frame, seeking: true,
                });
                self.target = None;
            }
            if self.ended {self.state.update(PlaybackRequest::Ended(self.revision, end));}
            return
        }
        if self.next.is_none() {
            match tokio::task::block_in_place(|| {
                let frame = decoder.next()?;
                self.ended = frame.is_none();
                let Some((position, frame)) = frame else {return Ok(None)};
                if self.target.is_some_and(|target| position < target) {
                    self.last = Some(frame);
                    self.corrupt_at = None;
                    self.corrupted = false;
                    return Ok(None)
                }
                decoder.image(&frame).map(|frame| Some((position, frame))).map_err(DecodeError::from)
            }) {
                Ok(frame) => {
                    self.next = frame;
                    let missing = decoder.end + decoder.interval * 2.0;
                    if self.ended && state.duration.is_some_and(|d| d.as_secs_f64() > missing + 0.000001) {
                        self.corrupt_at.get_or_insert(missing);
                    }
                }
                Err(DecodeError::Corrupt(position)) => {
                    self.corrupt_at.get_or_insert(position + decoder.interval * 2.0);
                }
                Err(DecodeError::Failed(error)) => {
                    self.state.update(PlaybackRequest::Error(
                        self.revision, error.to_string(), self.placeholder.clone(),
                    ));
                    self.target = None;
                }
            }
            return
        }
        let (position, frame) = self.next.as_ref().unwrap();
        if self.state.update(PlaybackRequest::Frame {
            revision: self.revision, position: *position, frame: frame.clone(),
            seeking: self.target.is_some(),
        }) {
            self.next = None;
            self.target = None;
            self.last = None;
            self.corrupt_at = None;
            self.corrupted = false;
        }
    }
}

impl Drop for PlaybackService {
    fn drop(&mut self) {self.state.shutdown();}
}

struct Decoder {
    input: ffmpeg::format::context::Input, decoder: ffmpeg::decoder::Video, index: usize,
    rgba: ffmpeg::frame::Video,
    base: f64, start: f64, interval: f64, end: f64, packet: f64, draining: bool, feed: bool,
}
impl Decoder {
    fn new(bytes: Arc<Vec<u8>>) -> Result<Self, ffmpeg::Error> {
        let io = ffmpeg::format::context::StreamIo::from_read_seek(Cursor::new(Bytes(bytes)))?;
        let input = ffmpeg::format::input_from_stream(io, None, None)?;
        let stream = input.streams().best(ffmpeg::media::Type::Video)
            .ok_or(ffmpeg::Error::StreamNotFound)?;
        let index = stream.index();
        let base = f64::from(stream.time_base());
        let start = if stream.start_time() == i64::MIN {0.0} else {stream.start_time() as f64 * base};
        let rate = f64::from(stream.avg_frame_rate());
        let interval = if rate.is_finite() && rate > 0.0 {1.0 / rate} else {0.0};
        let mut decoder = ffmpeg::codec::context::Context::from_parameters(stream.parameters())?
            .decoder();
        decoder.check(ffmpeg::decoder::Check::CRC | ffmpeg::decoder::Check::BITSTREAM
            | ffmpeg::decoder::Check::BUFFER | ffmpeg::decoder::Check::EXPLODE);
        let decoder = decoder.video()?;
        Ok(Self {
            input, decoder, index, base, start, interval, end: 0.0, packet: 0.0,
            draining: false, feed: false, rgba: ffmpeg::frame::Video::empty(),
        })
    }
    fn next(&mut self) -> Result<Option<(f64, ffmpeg::frame::Video)>, DecodeError> {
        let mut decoded = ffmpeg::frame::Video::empty();
        loop {
            let result = if std::mem::take(&mut self.feed) {
                Err(ffmpeg::Error::Other {errno: ffmpeg::error::EAGAIN})
            } else {self.decoder.receive_frame(&mut decoded)};
            match result {
                Ok(()) => {
                    let position = decoded.timestamp().map_or(self.end, |stamp| {
                        (stamp as f64 * self.base - self.start).max(0.0)
                    });
                    if decoded.packet().duration > 0 {
                        self.interval = decoded.packet().duration as f64 * self.base;
                    }
                    self.end = position + self.interval;
                    if decoded.is_corrupt() || decoded.has_decode_errors() {
                        return Err(DecodeError::Corrupt(position))
                    }
                    return Ok(Some((position, decoded)))
                }
                Err(ffmpeg::Error::Eof) => return Ok(None),
                Err(ffmpeg::Error::Other {errno}) if errno == ffmpeg::error::EAGAIN => {},
                Err(ffmpeg::Error::InvalidData) => {
                    self.feed = true;
                    self.end = self.end.max(self.packet + self.interval);
                    return Err(DecodeError::Corrupt(self.packet))
                }
                Err(error) => return Err(error.into()),
            }
            if self.draining {return Ok(None)}
            loop {
                let mut packet = ffmpeg::Packet::empty();
                match packet.read(&mut self.input) {
                    Ok(()) => {
                        if packet.stream() != self.index {continue}
                        self.packet = packet.pts().or(packet.dts()).map_or(self.end, |stamp| {
                            (stamp as f64 * self.base - self.start).max(0.0)
                        });
                        if packet.duration() > 0 {
                            self.interval = packet.duration() as f64 * self.base;
                        }
                        let result = if packet.is_corrupt() {Err(ffmpeg::Error::InvalidData)}
                            else {self.decoder.send_packet(&packet)};
                        match result {
                            Ok(()) => {},
                            Err(ffmpeg::Error::InvalidData) => {
                                self.end = self.end.max(self.packet + self.interval);
                                return Err(DecodeError::Corrupt(self.packet))
                            }
                            Err(error) => return Err(error.into()),
                        }
                    }
                    Err(ffmpeg::Error::Eof) => {
                        self.draining = true;
                        self.decoder.send_eof()?;
                    }
                    Err(error) => return Err(error.into()),
                }
                break
            }
        }
    }
    fn image(&mut self, decoded: &ffmpeg::frame::Video) -> Result<Image, ffmpeg::Error> {
        let (width, height) = (decoded.width(), decoded.height());
        let mut scaler = ffmpeg::software::scaling::context::Context::get(
            decoded.format(), width, height, ffmpeg::format::Pixel::RGBA, width, height,
            ffmpeg::software::scaling::flag::Flags::BILINEAR,
        )?;
        if self.rgba.width() != width || self.rgba.height() != height {
            self.rgba = ffmpeg::frame::Video::empty();
        }
        scaler.run(decoded, &mut self.rgba)?;
        let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
        for row in self.rgba.data(0).chunks(self.rgba.stride(0)).take(height as usize) {
            pixels.extend_from_slice(&row[..width as usize * 4]);
        }
        let frame = image::RgbaImage::from_raw(width, height, pixels)
            .ok_or(ffmpeg::Error::InvalidData)?;
        Ok(Arc::new(frame))
    }
}
