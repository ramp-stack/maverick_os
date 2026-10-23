use ffmpeg_next as ffmpeg;
use ffmpeg::media::Type;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use std::borrow::Borrow;
use std::hash::{Hash, Hasher};
use std::collections::VecDeque;
use std::io::{Cursor, Read, Seek, Write};
use std::iter::FusedIterator;
use std::ops::{Bound, Deref, Range, RangeBounds};
use std::sync::{Arc, OnceLock, mpsc};
use std::time::Duration;

use crate::shared::{Sharable, Shared};

mod decoder;
use decoder::{DecodeError, Decoder};
mod archive;
use archive::Archive;
pub use archive::Format;
pub mod playback;
mod speaker;
mod player;
pub use player::{Frame, Player};

#[derive(Clone, Debug)]
pub enum Visual {Image(Image), Video(Video)}

#[derive(Clone, Debug)]
pub struct Video(pub Media);
impl AsRef<Media> for Video {
    fn as_ref(&self) -> &Media {&self.0}
}

#[derive(Clone, Debug)]
pub struct Audio(pub Media);
impl AsRef<Media> for Audio {
    fn as_ref(&self) -> &Media {&self.0}
}

#[derive(Clone, Debug)]
pub struct Sound {
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Arc<Vec<f32>>,
}
impl Sound {
    pub fn duration(&self) -> Duration {
        let frames = self.samples.len() / self.channels as usize;
        Duration::from_secs_f64(frames as f64 / self.sample_rate as f64)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tracks {Audio, Video, Both}
impl Tracks {
    fn audio(self) -> bool {matches!(self, Self::Audio | Self::Both)}
    fn video(self) -> bool {matches!(self, Self::Video | Self::Both)}
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum PlaybackMode {#[default] Default, Live(Duration), Loop}

#[derive(Clone, Debug)]
pub struct AudioFrame {
    pub position: Duration,
    pub sound: Sound,
}

#[derive(Clone, Debug)]
pub struct VideoFrame {
    pub position: Duration,
    pub duration: Duration,
    pub image: Image,
}

#[derive(Clone, Copy, Debug)]
pub struct Status {
    pub revision: u64,
    pub duration: Duration,
    pub available: Duration,
    pub edit: u64,
    pub audio: bool,
}

#[derive(Clone)]
pub struct Image(Arc<ImageData>);
impl Image {
    /// Read encoded content and dimensions without decoding pixels.
    pub fn read<R: Read>(mut reader: R) -> Result<Self, String> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).map_err(|error| error.to_string())?;
        let dimensions = image::io::Reader::new(Cursor::new(&bytes)).with_guessed_format()
            .map_err(|error| error.to_string())?.into_dimensions().map_err(|error| error.to_string())?;
        if dimensions.0 == 0 || dimensions.1 == 0 {return Err("Image has no pixels".into())}
        Ok(Self(Arc::new(ImageData {
            dimensions, content: ImageContent::Encoded {bytes, decoded: OnceLock::new()},
        })))
    }
    pub fn dimensions(&self) -> (u32, u32) {self.0.dimensions}
}
impl From<image::RgbaImage> for Image {
    fn from(image: image::RgbaImage) -> Self {Self::from(Arc::new(image))}
}
impl From<Arc<image::RgbaImage>> for Image {
    fn from(image: Arc<image::RgbaImage>) -> Self {
        let dimensions = image.dimensions();
        assert!(dimensions.0 > 0 && dimensions.1 > 0);
        Self(Arc::new(ImageData {dimensions, content: ImageContent::Rgba(image)}))
    }
}
impl Deref for Image {
    type Target = Arc<image::RgbaImage>;
    fn deref(&self) -> &Self::Target {
        match &self.0.content {
            ImageContent::Rgba(image) => image,
            ImageContent::Encoded {bytes, decoded} => decoded.get_or_init(|| {
                let image = image::load_from_memory(bytes).unwrap_or_else(|_| {
                    image::load_from_memory(include_bytes!("media/corrupted.png")).unwrap()
                });
                Arc::new(image.into_rgba8())
            }),
        }
    }
}
impl PartialEq for Image {
    fn eq(&self, other: &Self) -> bool {Arc::ptr_eq(&self.0, &other.0)}
}
impl Eq for Image {}
impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Image").field(&self.dimensions()).finish()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Media(Source);
impl Media {
    pub fn new() -> Self {Self::default()}
    pub fn read<R: Read + Seek>(mut reader: R) -> Result<Self, String> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).map_err(|error| error.to_string())?;
        let bytes = Arc::new(bytes);
        let mut streams = Vec::new();
        for kind in [Type::Video, Type::Audio] {
            match Decoder::new(bytes.clone(), kind) {
                Ok(decoder) => streams.push((kind, Clip::new(bytes.clone(), kind, decoder)?)),
                Err(ffmpeg::Error::StreamNotFound) => {},
                Err(error) => return Err(error.to_string()),
            }
        }
        if streams.is_empty() {return Err("Media has no audio or video track".into())}
        let offset = streams.iter().filter_map(|(_, clip)| clip.as_ref().map(|clip| clip.offset))
            .reduce(f64::min).unwrap_or(0.0);
        let mut history = History::default();
        for (kind, clip) in streams {
            let Some(mut clip) = clip else {continue};
            clip.end += Duration::from_secs_f64((clip.offset - offset).max(0.0));
            clip.offset = offset;
            if kind == Type::Video {
                history.video_end = clip.end;
                history.video = Arc::new(vec![clip]);
            } else {
                history.audio_end = clip.end;
                history.audio_track = AudioTrack::Embedded;
                history.audio = Arc::new(vec![clip]);
            }
        }
        history.revision = 1;
        Ok(Self(Source::from(history)))
    }
    pub fn duration(&self) -> Duration {self.0.duration()}
    pub fn dimensions(&self) -> Option<(u32, u32)> {self.0.dimensions()}
    pub fn thumbnail(&self) -> Option<Image> {self.0.thumbnail()}
    pub fn copy<R: RangeBounds<Duration>>(&self, range: R, tracks: Tracks) -> Result<Self, String> {
        self.0.copy(range, tracks).map(Self)
    }
    pub fn replace<R: RangeBounds<Duration>>(&self, range: R, replacement: &Self, tracks: Tracks) -> Result<(), String> {
        self.0.replace(range, &replacement.0, tracks)
    }
    pub fn write<W: Write + Seek>(&self, mut writer: W, format: Format) -> Result<(), String> {
        writer.write_all(&self.0.encode(format)?).map_err(|error| error.to_string())
    }
    pub fn append(&self, other: &Self) {self.0.append(&other.0)}
    pub fn chunks(&self, duration: Duration) -> Result<Chunks, String> {
        Chunks::new(&self.0, duration)
    }
}
impl TryFrom<Sound> for Media {
    type Error = String;
    fn try_from(sound: Sound) -> Result<Self, String> {
        Ok(Self(Source::from_clip(Clip::from_sound(sound)?, Type::Audio)))
    }
}
impl TryFrom<(Image, Duration)> for Media {
    type Error = String;
    fn try_from((image, duration): (Image, Duration)) -> Result<Self, String> {
        Ok(Self(Source::from_clip(Clip::from_image(image, duration)?, Type::Video)))
    }
}
impl<T: Borrow<Media>> Extend<T> for Media {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        for item in iter {self.append(item.borrow());}
    }
}
impl<T: Borrow<Media>> FromIterator<T> for Media {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut media = Self::new();
        media.extend(iter);
        media
    }
}
impl AsRef<Media> for Media {
    fn as_ref(&self) -> &Media {self}
}

#[derive(Clone, Debug)]
pub struct Chunks {
    history: History,
    duration: Duration,
    range: Range<usize>,
}
impl Chunks {
    fn new(source: &Source, duration: Duration) -> Result<Self, String> {
        if duration.is_zero() {return Err("Chunk duration must be positive".into())}
        let history = source.0.history.load().as_ref().clone();
        history.check()?;
        let count = history.duration().as_nanos().div_ceil(duration.as_nanos());
        let range = 0..usize::try_from(count).unwrap();
        Ok(Self {history, duration, range})
    }
    fn chunk(&self, index: usize) -> Media {
        let nanos = self.duration.as_nanos() * index as u128;
        let start = Duration::new((nanos / 1_000_000_000) as u64, (nanos % 1_000_000_000) as u32);
        let end = start + self.duration.min(self.history.duration() - start);
        Media(Source::from(self.history.slice(start..end, Tracks::Both).unwrap()))
    }
}
impl Iterator for Chunks {
    type Item = Media;
    fn next(&mut self) -> Option<Media> {self.nth(0)}
    fn nth(&mut self, n: usize) -> Option<Media> {self.range.nth(n).map(|index| self.chunk(index))}
    fn size_hint(&self) -> (usize, Option<usize>) {self.range.size_hint()}
    fn count(self) -> usize {self.range.len()}
    fn last(mut self) -> Option<Media> {self.next_back()}
}
impl DoubleEndedIterator for Chunks {
    fn next_back(&mut self) -> Option<Media> {self.nth_back(0)}
    fn nth_back(&mut self, n: usize) -> Option<Media> {
        self.range.nth_back(n).map(|index| self.chunk(index))
    }
}
impl ExactSizeIterator for Chunks {}
impl FusedIterator for Chunks {}

#[derive(Clone, Debug)]
pub struct Source(Arc<State>);
impl Source {
    pub fn error(&self) -> Option<String> {self.0.history.load().error.clone()}
    pub fn fail(&self, error: String) {self.0.history.update(Request::Fail(error)).unwrap();}
    pub fn end_capture(&self) {self.0.history.update(Request::EndCapture).unwrap();}
    pub fn splice_audio(&self, frame: AudioFrame) -> Result<(), String> {
        self.splice_clip(frame.position, Clip::from_sound(frame.sound)?, Type::Audio)
    }
    pub fn capture_audio(&self) -> Result<Duration, String> {
        self.0.history.update(Request::Capture(false, true))
    }
    pub fn splice_video(&self, frame: VideoFrame) -> Result<(), String> {
        let clip = Clip::from_image(frame.image, frame.duration)?;
        self.splice_clip(frame.position, clip, Type::Video)
    }
    pub fn capture_video(&self, microphone: bool) -> Result<Duration, String> {
        self.0.history.update(Request::Capture(true, microphone))
    }
    fn duration(&self) -> Duration {self.0.history.load().duration()}
    fn copy<R: RangeBounds<Duration>>(&self, range: R, tracks: Tracks) -> Result<Self, String> {
        let history = self.0.history.load();
        Ok(Self::from(history.slice(history.range(range)?, tracks)?))
    }
    fn append(&self, replacement: &Self) {
        let replacement = replacement.0.history.load().as_ref().clone();
        let raw = replacement.audio.iter().chain(replacement.video.iter())
            .any(|clip| !matches!(clip.data, Content::Encoded(_)));
        self.0.history.update(Request::Append(replacement)).unwrap();
        self.record(raw);
    }
    fn replace<R: RangeBounds<Duration>>(&self, range: R, replacement: &Self, tracks: Tracks) -> Result<(), String> {
        let range = (range.start_bound().cloned(), range.end_bound().cloned());
        let replacement = replacement.0.history.load().as_ref().clone();
        let raw = replacement.audio.iter().chain(replacement.video.iter())
            .any(|clip| !matches!(clip.data, Content::Encoded(_)));
        self.0.history.update(Request::Splice(range, replacement, tracks))?;
        self.record(raw);
        Ok(())
    }
    fn record(&self, raw: bool) {
        let recorder = if raw {
            Some(self.0.recorder.get_or_init(|| Recorder::new(self.0.history.clone())))
        } else {self.0.recorder.get()};
        if let Some(recorder) = recorder {
            let _ = recorder.sender.as_ref().unwrap().try_send(());
        }
    }
    fn from_clip(clip: Clip, kind: Type) -> Self {
        let mut history = History::default();
        if kind == Type::Video {
            history.video_end = clip.end;
            history.video = Arc::new(vec![clip]);
        } else {
            history.audio_end = clip.end;
            history.audio_track = AudioTrack::Embedded;
            history.audio = Arc::new(vec![clip]);
        }
        history.revision += 1;
        Self::from(history)
    }
    fn splice_clip(&self, position: Duration, clip: Clip, kind: Type) -> Result<(), String> {
        self.0.history.update(Request::Frame(position, clip, kind))?;
        self.record(true);
        Ok(())
    }
    fn status(&self) -> Status {
        let history = self.0.history.load();
        let duration = history.duration();
        let video = (!history.video.is_empty()).then_some(history.video_end);
        let available = if history.audio_track == AudioTrack::Separate && history.capturing {
            video.map_or(history.audio_end, |video| video.min(history.audio_end))
        } else {duration};
        Status {
            revision: history.revision, duration, available,
            edit: history.edit, audio: history.audio_track != AudioTrack::None,
        }
    }
    fn audio_reader(&self) -> Reader {Reader::new(self.clone(), Type::Audio)}
    fn video_reader(&self) -> Reader {Reader::new(self.clone(), Type::Video)}
    fn dimensions(&self) -> Option<(u32, u32)> {
        self.0.history.load().video.first().map(|clip| clip.dimensions)
    }
    fn thumbnail(&self) -> Option<Image> {
        let history = self.0.history.load();
        if history.video.is_empty() {return None}
        Some(history.thumbnail.get_or_init(|| match history.preview() {
            Ok(Some(image)) => image,
            Ok(None) => image::RgbaImage::new(1, 1).into(),
            Err(_) => Image::read(include_bytes!("media/corrupted.png").as_slice()).unwrap(),
        }).clone())
    }
    fn encode(&self, format: Format) -> Result<Vec<u8>, String> {
        let history = self.0.history.load();
        history.check()?;
        let source = Source::from(History {capturing: false, ..history.as_ref().clone()});
        drop(history);
        let duration = source.duration();
        let dimensions = source.dimensions().filter(|_| format.video());
        let (mut video, mut audio) = (source.video_reader(), source.audio_reader());
        let mut v = if dimensions.is_some() {video.video()?} else {None};
        let mut a = if format.audio() {audio.audio()?} else {None};
        let audio_format = a.as_ref().map(|frame| (frame.sound.sample_rate, frame.sound.channels))
            .or_else(|| (format.audio() && !format.video()).then_some((48_000, 2)));
        let empty = a.is_none() && dimensions.is_none();
        let mut archive = Archive::new(format, dimensions, audio_format, empty)
            .map_err(|error| error.to_string())?;
        if let Some(dimensions) = dimensions {
            let first = v.as_ref().map_or(duration, |frame| frame.position);
            if !first.is_zero() {
                archive.video(&VideoFrame {
                    position: Duration::ZERO, duration: first,
                    image: image::RgbaImage::new(dimensions.0, dimensions.1).into(),
                }).map_err(|error| error.to_string())?;
            }
        }
        while v.is_some() || a.is_some() {
            if v.as_ref().is_some_and(|frame| a.as_ref().is_none_or(|a| frame.position <= a.position)) {
                let mut frame = v.take().unwrap();
                v = video.video()?;
                if v.is_none() {frame.duration = duration.saturating_sub(frame.position);}
                archive.video(&frame).map_err(|error| error.to_string())?;
            } else {
                archive.audio(&a.take().unwrap()).map_err(|error| error.to_string())?;
                a = audio.audio()?;
            }
        }
        if audio_format.is_some() && !empty {
            archive.silence(duration).map_err(|error| error.to_string())?;
        }
        archive.finish().map_err(|error| error.to_string())
    }
}
impl<'a> From<&'a Media> for &'a Source {
    fn from(media: &'a Media) -> Self {&media.0}
}
impl From<History> for Source {
    fn from(history: History) -> Self {
        Self(Arc::new(State {
            history: Shared::temporary(Init(rand::random(), history)), recorder: OnceLock::new(),
        }))
    }
}
impl Default for Source {
    fn default() -> Self {Self::from(History::default())}
}

enum ImageContent {
    Encoded {
        bytes: Vec<u8>,
        decoded: OnceLock<Arc<image::RgbaImage>>,
    },
    Rgba(Arc<image::RgbaImage>),
}

struct ImageData {
    dimensions: (u32, u32),
    content: ImageContent,
}

#[derive(Debug)]
struct State {
    history: Shared<History>,
    recorder: OnceLock<Recorder>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
enum Content {
    Encoded(Arc<Vec<u8>>),
    Image(#[serde(serialize_with = "Content::save_image", deserialize_with = "Content::read_image")] Image),
    Sound(#[serde(serialize_with = "Content::save_sound", deserialize_with = "Content::read_sound")] Sound),
}
impl Content {
    fn save_image<S: Serializer>(image: &Image, serializer: S) -> Result<S::Ok, S::Error> {
        let rgba = image.as_ref();
        (rgba.width(), rgba.height(), rgba.as_raw()).serialize(serializer)
    }
    fn read_image<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Image, D::Error> {
        let (width, height, bytes) = <(u32, u32, Vec<u8>)>::deserialize(deserializer)?;
        image::RgbaImage::from_raw(width, height, bytes).filter(|image| {
            image.width() > 0 && image.height() > 0
        }).map(Image::from)
            .ok_or_else(|| serde::de::Error::custom("Invalid stored image dimensions"))
    }
    fn save_sound<S: Serializer>(sound: &Sound, serializer: S) -> Result<S::Ok, S::Error> {
        (sound.sample_rate, sound.channels, &sound.samples).serialize(serializer)
    }
    fn read_sound<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Sound, D::Error> {
        let (sample_rate, channels, samples) = Deserialize::deserialize(deserializer)?;
        let sound = Sound {sample_rate, channels, samples};
        Clip::from_sound(sound.clone()).map_err(serde::de::Error::custom)?;
        Ok(sound)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
enum AudioTrack {#[default] None, Embedded, Separate}

struct Reader {
    source: Source,
    kind: Type,
    index: usize,
    decoder: Option<Decoder>,
    target: f64,
    audio_end: Option<f64>,
    corrupt: Option<f64>,
    frames: VecDeque<VideoFrame>,
    edit: u64,
}
impl Reader {
    fn new(source: Source, kind: Type) -> Self {
        Self {
            source, kind,
            index: 0, decoder: None, target: 0.0, audio_end: None, corrupt: None,
            frames: VecDeque::new(), edit: 0,
        }
    }
    fn seek(&mut self, position: Duration) {
        let history = self.source.0.history.load().as_ref().clone();
        self.reset(&history, position);
    }
    fn reset(&mut self, history: &History, position: Duration) {
        self.edit = history.edit;
        let clips = if self.kind == Type::Video {&history.video} else {&history.audio};
        self.index = clips.partition_point(|clip| {
            if self.kind == Type::Video {clip.end < position} else {clip.end <= position}
        });
        self.decoder = None;
        self.target = position.as_secs_f64();
        self.audio_end = None;
        self.corrupt = None;
        self.frames.clear();
    }
    fn next(&mut self, history: &History, frame: &mut ffmpeg::Frame) -> Result<Option<(f64, Clip)>, String> {
        loop {
            let clips = if self.kind == Type::Video {&history.video} else {&history.audio};
            let Some(clip) = clips.get(self.index).cloned() else {return Ok(None)};
            if !matches!(clip.data, Content::Encoded(_)) {
                self.index += 1;
                self.decoder = None;
                return Ok(Some((clip.start.as_secs_f64(), clip)))
            }
            if self.decoder.is_none() {
                let Content::Encoded(bytes) = &clip.data else {unreachable!()};
                let mut decoder = Decoder::new(bytes.clone(), self.kind)
                    .map_err(|error| error.to_string())?;
                let trimmed = clip.offset > decoder.start;
                decoder.start = clip.offset;
                let position = self.target - clip.start.as_secs_f64() - decoder.interval * 2.0;
                let position = position.max(0.0);
                if position > 0.0 || trimmed {
                    decoder.seek(position).map_err(|error| error.to_string())?;
                }
                self.decoder = Some(decoder);
                self.audio_end = None;
            }
            let decoder = self.decoder.as_mut().unwrap();
            match decoder.next(frame) {
                Ok(Some(position)) => {
                    let position = position + clip.start.as_secs_f64();
                    if position < clip.end.as_secs_f64() {return Ok(Some((position, clip)))}
                    self.index += 1;
                    self.decoder = None;
                    self.corrupt = None;
                }
                Ok(None) => {
                    if self.kind == Type::Video && let Some(position) = self.corrupt.take() {
                        let position = Duration::from_secs_f64(
                            (position + clip.start.as_secs_f64()).max(0.0),
                        );
                        if position < clip.end {
                            let image = image::load_from_memory(
                                include_bytes!("media/corrupted.png"),
                            ).unwrap();
                            self.queue_video(VideoFrame {
                                position, duration: clip.end - position,
                                image: image.into_rgba8().into(),
                            });
                        }
                    }
                    self.index += 1;
                    self.decoder = None;
                    self.corrupt = None;
                }
                Err(DecodeError::Corrupt(position)) => {
                    self.corrupt.get_or_insert(position + decoder.interval * 2.0);
                }
                Err(DecodeError::Failed(error)) => return Err(error.to_string()),
            }
        }
    }
    fn audio(&mut self) -> Result<Option<AudioFrame>, String> {
        let history = self.source.0.history.load().as_ref().clone();
        if history.edit != self.edit {self.reset(&history, Duration::from_secs_f64(self.target));}
        let mut frame = ffmpeg::frame::Audio::empty();
        loop {
            let Some((mut position, clip)) = self.next(&history, &mut frame)? else {return Ok(None)};
            if let Content::Sound(sound) = &clip.data {
                position -= clip.offset;
                let rate = sound.sample_rate as f64;
                let target = self.target.max(clip.start.as_secs_f64());
                let skip = ((target - position).max(0.0) * rate).round() as usize;
                let count = ((clip.end.as_secs_f64() - position).max(0.0) * rate).round() as usize;
                let range = skip.saturating_mul(sound.channels as usize)
                    ..count.saturating_mul(sound.channels as usize).min(sound.samples.len());
                if range.start >= range.end {continue}
                let samples = if range.start == 0 && range.end == sound.samples.len() {
                    sound.samples.clone()
                } else {Arc::new(sound.samples[range].to_vec())};
                let frame = AudioFrame {
                    position: Duration::from_secs_f64((position + skip as f64 / rate).max(0.0)),
                    sound: Sound {sample_rate: sound.sample_rate, channels: sound.channels, samples},
                };
                self.target = self.target.max((frame.position + frame.sound.duration()).as_secs_f64());
                return Ok(Some(frame))
            }
            let (rate, channels) = (frame.rate(), frame.channels());
            if rate == 0 || channels == 0 {return Err("Invalid decoded audio format".into())}
            let tolerance = self.decoder.as_ref().unwrap().base.abs().max(1.0 / rate as f64);
            if let Some(end) = self.audio_end.filter(|end| (position - end).abs() <= tolerance) {
                position = end;
            }
            let end = position + frame.samples() as f64 / rate as f64;
            self.audio_end = Some(end);
            let target = self.target.max(clip.start.as_secs_f64());
            if end <= target {continue}
            let layout = if frame.channel_layout().is_empty() {
                ffmpeg::ChannelLayout::default(i32::from(channels))
            } else {frame.channel_layout()};
            frame.set_channel_layout(layout);
            let format = ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Packed);
            let mut converted = ffmpeg::frame::Audio::empty();
            ffmpeg::software::resampling::Context::get(
                frame.format(), layout, rate, format, layout, rate,
            ).and_then(|mut resampler| resampler.run(&frame, &mut converted))
                .map_err(|error| error.to_string())?;
            let skip = ((target - position).max(0.0) * rate as f64).round() as usize;
            let count = converted.samples().min(
                ((clip.end.as_secs_f64() - position).max(0.0) * rate as f64).round() as usize,
            );
            if skip >= count {continue}
            let range = skip * channels as usize..count * channels as usize;
            let samples = converted.plane::<f32>(0)[range].to_vec();
            let frame = AudioFrame {
                position: Duration::from_secs_f64((position + skip as f64 / rate as f64).max(0.0)),
                sound: Sound {sample_rate: rate, channels, samples: Arc::new(samples)},
            };
            self.target = self.target.max((frame.position + frame.sound.duration()).as_secs_f64());
            return Ok(Some(frame))
        }
    }
    fn video(&mut self) -> Result<Option<VideoFrame>, String> {
        let history = self.source.0.history.load().as_ref().clone();
        if history.edit != self.edit {self.reset(&history, Duration::from_secs_f64(self.target));}
        while self.frames.len() < 2 {
            let mut raw = ffmpeg::frame::Video::empty();
            let Some((position, clip)) = self.next(&history, &mut raw)? else {break};
            let image = match &clip.data {
                Content::Image(image) => image.clone(),
                Content::Sound(_) => unreachable!(),
                Content::Encoded(_) => self.decoder.as_mut().unwrap().image(&raw)
                    .map_err(|error| error.to_string())?,
            };
            if let Some(start) = self.corrupt.take() {
                let start = start + clip.start.as_secs_f64();
                if start < position {
                    let placeholder = image::load_from_memory(
                        include_bytes!("media/corrupted.png"),
                    ).unwrap();
                    let placeholder = placeholder.resize_exact(
                        raw.width().max(88), raw.height().max(40), image::imageops::FilterType::Triangle,
                    ).into_rgba8();
                    self.queue_video(VideoFrame {
                        position: Duration::from_secs_f64(start.max(0.0)),
                        duration: Duration::from_secs_f64(position - start),
                        image: placeholder.into(),
                    });
                }
            }
            let position = Duration::from_secs_f64(position.max(clip.start.as_secs_f64()));
            if position >= clip.end {continue}
            self.queue_video(VideoFrame {position, duration: clip.end - position, image});
            while self.frames.front().is_some_and(|frame| {
                frame.duration.is_zero() || (frame.position + frame.duration).as_secs_f64() < self.target
            }) {self.frames.pop_front();}
        }
        let frame = self.frames.pop_front();
        if let Some(frame) = &frame {
            self.target = self.target.max((frame.position + frame.duration).as_secs_f64());
        }
        Ok(frame)
    }
    fn queue_video(&mut self, frame: VideoFrame) {
        if let Some(previous) = self.frames.back_mut() {
            previous.duration = previous.duration.min(frame.position.saturating_sub(previous.position));
        }
        self.frames.push_back(frame);
    }
}

struct Init(u128, History);
impl Hash for Init {
    fn hash<H: Hasher>(&self, state: &mut H) {self.0.hash(state)}
}

enum Request {
    Splice((Bound<Duration>, Bound<Duration>), History, Tracks),
    Append(History),
    Frame(Duration, Clip, Type),
    Capture(bool, bool),
    EndCapture,
    Fail(String),
    Compact(bool, usize, u64, Result<Arc<Vec<u8>>, String>),
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct History {
    revision: u64,
    video: Arc<Vec<Clip>>,
    audio: Arc<Vec<Clip>>,
    video_end: Duration,
    audio_end: Duration,
    audio_track: AudioTrack,
    edit: u64,
    error: Option<String>,
    #[serde(skip)]
    thumbnail: Arc<OnceLock<Image>>,
    capturing: bool,
}
impl History {
    fn range<R: RangeBounds<Duration>>(&self, range: R) -> Result<Range<Duration>, String> {
        let start = match range.start_bound() {
            Bound::Included(start) => *start,
            Bound::Excluded(start) => *start + Duration::from_nanos(1),
            Bound::Unbounded => Duration::ZERO,
        };
        let end = match range.end_bound() {
            Bound::Excluded(end) => *end,
            Bound::Included(end) => *end + Duration::from_nanos(1),
            Bound::Unbounded => self.duration(),
        };
        if start > end || end > self.duration() {return Err("Range is outside the media timeline".into())}
        Ok(start..end)
    }
    fn duration(&self) -> Duration {self.video_end.max(self.audio_end)}
    fn slice(&self, range: Range<Duration>, tracks: Tracks) -> Result<Self, String> {
        self.check()?;
        if range.start > range.end || range.end > self.duration() {
            return Err("Slice range is outside the media timeline".into())
        }
        let video = if tracks.video() {
            self.video.iter().filter_map(|clip| clip.slice(range.clone(), Duration::ZERO)).collect()
        } else {Vec::new()};
        let audio = if tracks.audio() {
            self.audio.iter().filter_map(|clip| clip.slice(range.clone(), Duration::ZERO)).collect()
        } else {Vec::new()};
        let audio_track = if audio.is_empty() {AudioTrack::None} else {AudioTrack::Embedded};
        let duration = range.end - range.start;
        Ok(Self {
            video: Arc::new(video), audio: Arc::new(audio), audio_track,
            audio_end: if tracks.audio() {duration} else {Duration::ZERO},
            video_end: if tracks.video() {duration} else {Duration::ZERO},
            revision: 1, ..Self::default()
        })
    }
    fn splice(&mut self, range: Range<Duration>, replacement: &Self, tracks: Tracks) {
        let duration = self.duration();
        let added = replacement.duration();
        if range.is_empty() && added.is_zero() {return}
        if tracks.video() && self.video.first().is_none_or(|clip| range.start < clip.end) {
            self.thumbnail = Arc::default();
        }
        for (clips, end, replacement, selected) in [
            (&mut self.audio, &mut self.audio_end, &replacement.audio, tracks.audio()),
            (&mut self.video, &mut self.video_end, &replacement.video, tracks.video()),
        ] {
            if !selected {continue}
            Clip::splice(Arc::make_mut(clips), range.clone(), replacement, added);
            *end = range.start + added + end.saturating_sub(range.end);
        }
        if !self.capturing || self.audio_track != AudioTrack::Separate {
            self.audio_track = if self.audio.is_empty() {AudioTrack::None} else {AudioTrack::Embedded};
        }
        if range.start < duration {self.edit += 1;}
        self.revision += 1;
    }
    fn preview(&self) -> Result<Option<Image>, String> {
        let Some(clip) = self.video.first() else {return Ok(None)};
        let bytes = match &clip.data {
            Content::Image(image) => return Ok(Some(image.clone())),
            Content::Sound(_) => unreachable!(),
            Content::Encoded(bytes) => bytes,
        };
        let mut decoder = Decoder::new(bytes.clone(), Type::Video)
            .map_err(|error| error.to_string())?;
        let trimmed = clip.offset > decoder.start;
        decoder.start = clip.offset;
        if trimmed {decoder.seek(0.0).map_err(|error| error.to_string())?;}
        let mut frame = ffmpeg::frame::Video::empty();
        let mut image = None;
        while let Some(position) = decoder.next(&mut frame).map_err(|error| error.to_string())? {
            if position > 0.0 && image.is_some() {break}
            if position >= (clip.end - clip.start).as_secs_f64() {break}
            image = Some(decoder.image(&frame).map_err(|error| error.to_string())?);
            if position >= 0.0 {break}
        }
        image.map(Some).ok_or_else(|| "Video has no decodable thumbnail".into())
    }
    fn check(&self) -> Result<(), String> {
        if let Some(error) = &self.error {return Err(error.clone())}
        Ok(())
    }
    fn splice_frame(&mut self, mut position: Duration, clip: Clip, kind: Type) -> Result<(), String> {
        self.check()?;
        let (clips, end) = if kind == Type::Video {
            (&self.video, self.video_end)
        } else {(&self.audio, self.audio_end)};
        if let Some(previous) = clips.last().filter(|previous| previous.end > position) {
            match &clip.data {
                Content::Sound(sound) => {
                    if previous.end - position > Duration::from_secs_f64(1.0 / sound.sample_rate as f64) {
                        return Err("Captured sound precedes existing audio".into())
                    }
                    position = previous.end;
                }
                _ if position <= previous.start => {
                    return Err("Captured image precedes existing video".into())
                }
                _ => {},
            }
        }
        let added = clip.end;
        let new_end = position + added;
        if position < end {self.edit += 1;}
        if kind == Type::Video {
            if self.video.is_empty() {self.thumbnail = Arc::default();}
            Clip::splice(Arc::make_mut(&mut self.video), position..position.max(end), &[clip], added);
            self.video_end = new_end;
        } else {
            Clip::splice(Arc::make_mut(&mut self.audio), position..position.max(end), &[clip], added);
            self.audio_end = new_end;
            if self.audio_track == AudioTrack::None {self.audio_track = AudioTrack::Embedded;}
        }
        self.revision += 1;
        Ok(())
    }
}
impl Sharable for History {
    type Init = Init;
    type Request = Request;
    type Response = Result<Duration, String>;

    fn persistent() -> bool {false}
    async fn init(init: Init) -> Self {init.1}
    async fn handle(&mut self, request: Request) -> Result<Duration, String> {
        match request {
            Request::Splice(range, replacement, tracks) => {
                self.check()?;
                replacement.check()?;
                self.splice(self.range(range)?, &replacement, tracks);
            }
            Request::Append(replacement) => {
                self.splice(self.duration()..self.duration(), &replacement, Tracks::Both);
                if self.error.is_none() {self.error = replacement.error;}
            }
            Request::Frame(position, clip, kind) => self.splice_frame(position, clip, kind)?,
            Request::Capture(video, microphone) => {
                self.check()?;
                if self.capturing {return Err("This recording already has an active capture".into())}
                self.capturing = true;
                self.audio_track = if video && microphone {
                    AudioTrack::Separate
                } else if microphone || !self.audio.is_empty() {
                    AudioTrack::Embedded
                } else {AudioTrack::None};
                self.revision += 1;
            }
            Request::EndCapture => {self.capturing = false; self.revision += 1;}
            Request::Fail(error) => {self.error = Some(error); self.revision += 1;}
            Request::Compact(video, index, edit, result) => {
                if edit == self.edit {
                    match result {
                        Ok(bytes) => {
                            let clips = if video {&mut self.video} else {&mut self.audio};
                            if index < clips.len() {
                                Arc::make_mut(clips)[index].data = Content::Encoded(bytes);
                            }
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
            }
        }
        Ok(self.duration())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Clip {
    data: Content,
    start: Duration,
    end: Duration,
    offset: f64,
    dimensions: (u32, u32),
}
impl Clip {
    fn from_sound(sound: Sound) -> Result<Self, String> {
        if sound.sample_rate == 0 || sound.sample_rate > i32::MAX as u32 || sound.channels == 0
            || sound.samples.is_empty() || sound.samples.len() % sound.channels as usize != 0
            || sound.samples.iter().any(|sample| !sample.is_finite())
        {
            return Err("Sound requires finite samples, a sample rate, and complete channels".into())
        }
        Ok(Self {
            end: sound.duration(), data: Content::Sound(sound),
            start: Duration::ZERO, offset: 0.0, dimensions: (0, 0),
        })
    }
    fn from_image(image: Image, duration: Duration) -> Result<Self, String> {
        if duration.is_zero() {return Err("Video duration must be positive".into())}
        Ok(Self {
            dimensions: image.dimensions(), data: Content::Image(image),
            start: Duration::ZERO, end: duration, offset: 0.0,
        })
    }
    fn splice(clips: &mut Vec<Self>, range: Range<Duration>, replacement: &[Self], added: Duration) {
        let start = clips.partition_point(|clip| clip.end <= range.start);
        let end = clips.partition_point(|clip| clip.start < range.end).max(start);
        let before = clips.get(start).and_then(|clip| {
            clip.slice(Duration::ZERO..range.start, Duration::ZERO)
        });
        let after = clips.get(end.saturating_sub(1)).filter(|_| end > start)
            .and_then(|clip| clip.slice(range.end..clip.end, range.start + added));
        let inserted = [
            before.into_iter().collect::<Vec<_>>(),
            replacement.iter().map(|clip| clip.clone().shift(range.start)).collect(),
            after.into_iter().collect(),
        ].into_iter().flatten();
        for clip in &mut clips[end..] {
            clip.start = range.start + added + (clip.start - range.end);
            clip.end = range.start + added + (clip.end - range.end);
        }
        clips.splice(start..end, inserted);
    }
    fn new(bytes: Arc<Vec<u8>>, kind: Type, mut decoder: Decoder) -> Result<Option<Self>, String> {
        let offset = decoder.start;
        let duration = decoder.duration(offset).filter(|duration| !duration.is_zero());
        if kind == Type::Audio {
            let mut frame = ffmpeg::frame::Audio::empty();
            let Some(position) = decoder.next(&mut frame).map_err(|error| error.to_string())? else {
                return Ok(None)
            };
            decoder.end = position + frame.samples() as f64 / frame.rate() as f64;
        }
        let duration = match duration {
            Some(duration) => duration,
            None => {
                decoder.start = offset;
                let mut frame = ffmpeg::frame::Audio::empty();
                loop {
                    match decoder.next(&mut frame) {
                        Ok(Some(position)) => {
                            if kind == Type::Audio {
                                decoder.end = position + frame.samples() as f64 / frame.rate() as f64;
                            }
                        }
                        Ok(None) => break,
                        Err(DecodeError::Corrupt(_)) => {},
                        Err(DecodeError::Failed(error)) => return Err(error.to_string()),
                    }
                }
                Duration::from_secs_f64(decoder.end.max(0.0))
            }
        };
        if duration.is_zero() {
            if kind == Type::Audio {return Ok(None)}
            return Err("Recording has no usable duration".into())
        }
        Ok(Some(Self {
            data: Content::Encoded(bytes), start: Duration::ZERO, end: duration, offset,
            dimensions: decoder.dimensions(),
        }))
    }
    fn shift(mut self, offset: Duration) -> Self {self.start += offset; self.end += offset; self}
    fn slice(&self, range: Range<Duration>, position: Duration) -> Option<Self> {
        let start = self.start.max(range.start);
        let end = self.end.min(range.end);
        if start >= end {return None}
        let offset = match &self.data {
            Content::Image(_) => 0.0,
            _ => self.offset + (start - self.start).as_secs_f64(),
        };
        Some(Self {
            data: self.data.clone(), start: position + (start - range.start),
            end: position + (end - range.start), offset, dimensions: self.dimensions,
        })
    }
}

#[derive(Debug)]
struct Recorder {
    sender: Option<mpsc::SyncSender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Recorder {
    fn new(history: Shared<History>) -> Self {
        let (sender, receiver) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new().name("video-history".into()).spawn(move || {
            let (mut video, mut audio, mut edit) = (0, 0, 0);
            while receiver.recv().is_ok() {
                loop {
                    if matches!(receiver.try_recv(), Err(mpsc::TryRecvError::Disconnected)) {return}
                    let next = {
                        let history = history.load();
                        if edit != history.edit {
                            video = 0;
                            audio = 0;
                            edit = history.edit;
                        }
                        [(&history.video, &mut video, true), (&history.audio, &mut audio, false)]
                            .into_iter().find_map(|(clips, cursor, video)| {
                                while *cursor < clips.len().saturating_sub(2) {
                                    let index = *cursor;
                                    *cursor += 1;
                                    if matches!(clips[index].data, Content::Image(_) | Content::Sound(_)) {
                                        return Some((video, index, clips[index].clone()))
                                    }
                                }
                                None
                            })
                    };
                    let Some((video, index, clip)) = next else {break};
                    let result = (|| {
                        let archive = match &clip.data {
                            Content::Image(image) => {
                                let mut archive = Archive::new(
                                    Format::Mkv_Ffv1_PcmF32le, Some(image.dimensions()), None, false,
                                )?;
                                archive.video(&VideoFrame {
                                    position: Duration::ZERO, duration: clip.end - clip.start,
                                    image: image.clone(),
                                })?;
                                archive
                            }
                            Content::Sound(sound) => {
                                let mut archive = Archive::new(
                                    Format::Mkv_Ffv1_PcmF32le, None, Some((sound.sample_rate, sound.channels)), false,
                                )?;
                                archive.audio(&AudioFrame {
                                    position: Duration::ZERO, sound: sound.clone(),
                                })?;
                                archive
                            }
                            Content::Encoded(_) => unreachable!(),
                        };
                        archive.finish()
                    })();
                    let result = result.map(Arc::new).map_err(|error| error.to_string());
                    let failed = result.is_err();
                    history.update(Request::Compact(video, index, edit, result)).unwrap();
                    if failed && history.load().error.is_some() {return}
                }
            }
        }).unwrap();
        Self {sender: Some(sender), thread: Some(thread)}
    }
}
impl Drop for Recorder {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(thread) = self.thread.take() {thread.join().unwrap();}
    }
}
