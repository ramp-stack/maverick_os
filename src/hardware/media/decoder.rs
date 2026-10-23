use ffmpeg_next as ffmpeg;
use ffmpeg::{decoder::Check, format::stream::Disposition, media::Type};

use std::io::Cursor;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use super::Image;

static START: OnceLock<Result<(), ffmpeg::Error>> = OnceLock::new();

pub enum DecodeError {Corrupt(f64), Failed(ffmpeg::Error)}
impl From<ffmpeg::Error> for DecodeError {
    fn from(error: ffmpeg::Error) -> Self {Self::Failed(error)}
}
impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Corrupt(position) => write!(f, "Corrupt frame at {position:.3}s"),
            Self::Failed(error) => error.fmt(f),
        }
    }
}

pub struct Decoder {
    pub base: f64,
    pub start: f64,
    pub interval: f64,
    pub end: f64,
    input: ffmpeg::format::context::Input,
    decoder: ffmpeg::decoder::Opened,
    index: usize,
    dimensions: (u32, u32),
    rgba: ffmpeg::frame::Video,
    packet: f64,
    draining: bool,
    feed: bool,
}
impl Decoder {
    pub fn init() -> Result<(), ffmpeg::Error> {
        *START.get_or_init(ffmpeg::init)
    }
    pub fn new(bytes: Arc<Vec<u8>>, kind: Type) -> Result<Self, ffmpeg::Error> {
        Self::init()?;
        let io = ffmpeg::format::context::StreamIo::from_read_seek(Cursor::new(Bytes(bytes)))?;
        let input = ffmpeg::format::input_from_stream(io, None, None)?;
        let stream = input.streams().best(kind).ok_or(ffmpeg::Error::StreamNotFound)?;
        if kind == Type::Video && stream.disposition().contains(Disposition::ATTACHED_PIC) {
            return Err(ffmpeg::Error::StreamNotFound)
        }
        let index = stream.index();
        let base = f64::from(stream.time_base());
        let start = if stream.start_time() == i64::MIN {0.0} else {stream.start_time() as f64 * base};
        let rate = f64::from(stream.avg_frame_rate());
        let interval = if rate.is_finite() && rate > 0.0 {1.0 / rate} else {0.0};
        let mut decoder = ffmpeg::codec::context::Context::from_parameters(stream.parameters())?
            .decoder();
        decoder.check(Check::CRC | Check::BITSTREAM | Check::BUFFER | Check::EXPLODE);
        let codec = decoder.codec().or_else(|| ffmpeg::decoder::find(decoder.id()))
            .ok_or(ffmpeg::Error::DecoderNotFound)?;
        let mut decoder = decoder.open_as(codec)?;
        let dimensions = if kind == Type::Video {
            let video = decoder.video()?;
            let dimensions = (video.width(), video.height());
            if dimensions.0 == 0 || dimensions.1 == 0 {return Err(ffmpeg::Error::InvalidData)}
            decoder = video.0;
            dimensions
        } else {(0, 0)};
        Ok(Self {
            input, decoder, index, dimensions, base, start, interval, end: 0.0, packet: 0.0,
            draining: false, feed: false, rgba: ffmpeg::frame::Video::empty(),
        })
    }
    pub fn dimensions(&self) -> (u32, u32) {self.dimensions}
    pub fn duration(&self, offset: f64) -> Option<Duration> {
        let stream = self.input.stream(self.index).unwrap();
        let ticks = stream.duration();
        let end = stream.metadata().get("DURATION").and_then(|value| {
            value.split(':').try_fold(0.0, |time, part| {
                part.parse::<f64>().ok().map(|part| time * 60.0 + part)
            })
        });
        let seconds = if ticks > 0 {
            Some(ticks as f64 * self.base + self.start - offset)
        } else if let Some(end) = end {Some(end - offset)} else {
            let duration = self.input.duration();
            (duration > 0).then_some(duration as f64 / 1_000_000.0)
        };
        seconds.and_then(|seconds| Duration::try_from_secs_f64(seconds).ok())
    }
    pub fn seek(&mut self, position: f64) -> Result<(), ffmpeg::Error> {
        let stamp = ((position + self.start) * 1_000_000.0) as i64;
        self.input.seek(stamp, ..stamp)?;
        self.decoder.flush();
        self.draining = false;
        self.feed = false;
        self.end = position;
        self.packet = position;
        Ok(())
    }
    pub fn next(&mut self, decoded: &mut ffmpeg::Frame) -> Result<Option<f64>, DecodeError> {
        loop {
            let result = if std::mem::take(&mut self.feed) {
                Err(ffmpeg::Error::Other {errno: ffmpeg::error::EAGAIN})
            } else {self.decoder.receive_frame(decoded)};
            match result {
                Ok(()) => {
                    let position = decoded.timestamp().map_or(self.end, |stamp| {
                        stamp as f64 * self.base - self.start
                    });
                    if decoded.packet().duration > 0 {
                        self.interval = decoded.packet().duration as f64 * self.base;
                    }
                    self.end = position + self.interval;
                    if decoded.is_corrupt() || decoded.has_decode_errors() {
                        return Err(DecodeError::Corrupt(position))
                    }
                    return Ok(Some(position))
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
                        let result = if packet.is_corrupt() {
                            Err(ffmpeg::Error::InvalidData)
                        } else {self.decoder.send_packet(&packet)};
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
    pub fn image(&mut self, decoded: &ffmpeg::frame::Video) -> Result<Image, ffmpeg::Error> {
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
        Ok(frame.into())
    }
}

struct Bytes(Arc<Vec<u8>>);
impl AsRef<[u8]> for Bytes {
    fn as_ref(&self) -> &[u8] {self.0.as_slice()}
}
