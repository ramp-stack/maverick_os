use ffmpeg_next as ffmpeg;
use std::io::{Read, Seek, SeekFrom};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use super::Image;

#[derive(Clone, Debug)]
pub struct Video {
    bytes: Arc<Vec<u8>>,
    duration: Option<Duration>,
    dimensions: (u32, u32),
    thumbnail: Image,
    format: String,
    codec: String,
}

impl Video {
    pub fn open(path: impl AsRef<std::path::Path>) -> Option<Self> {Self::new(std::fs::read(path).ok()?)}

    /// Validates the first video frame; later decoding errors are handled during playback.
    pub fn new(bytes: Vec<u8>) -> Option<Self> {
        static START: OnceLock<Result<(), ffmpeg::Error>> = OnceLock::new();
        START.get_or_init(ffmpeg::init).as_ref().expect("Could not initialize FFmpeg");
        let bytes = Arc::new(bytes);
        let io = ffmpeg::format::context::StreamIo::from_read_seek(
            Movie{bytes: bytes.clone(), at: 0},
        ).ok()?;
        let mut input = ffmpeg::format::input_from_stream(io, None, None).ok()?;
        let stream = input.streams().best(ffmpeg::media::Type::Video)?;
        let index = stream.index();
        let base = stream.time_base();
        let ticks = stream.duration();
        let seconds = if ticks > 0 {
            Some(ticks as f64 * base.numerator() as f64 / base.denominator() as f64)
        } else {
            let ticks = input.duration();
            (ticks > 0).then_some(ticks as f64 / 1_000_000.0)
        };
        let duration = seconds.and_then(|seconds| Duration::try_from_secs_f64(seconds).ok());
        let format = input.format().name().to_owned();
        let mut decoder = ffmpeg::codec::context::Context::from_parameters(
            stream.parameters(),
        ).ok()?.decoder();
        decoder.check(ffmpeg::decoder::Check::CRC
            | ffmpeg::decoder::Check::BITSTREAM
            | ffmpeg::decoder::Check::BUFFER
            | ffmpeg::decoder::Check::EXPLODE);
        let mut decoder = decoder.video().ok()?;
        let codec = decoder.id().name().to_owned();
        let mut decoded = ffmpeg::frame::Video::empty();
        loop {
            match decoder.receive_frame(&mut decoded) {
                Ok(()) => break,
                Err(ffmpeg::Error::Other{errno}) if errno == ffmpeg::error::EAGAIN => {},
                Err(_) => return None,
            }
            loop {
                let mut packet = ffmpeg::Packet::empty();
                match packet.read(&mut input) {
                    Ok(()) => {
                        if packet.stream() != index {continue}
                        if packet.is_corrupt() {return None}
                        decoder.send_packet(&packet).ok()?;
                    }
                    Err(ffmpeg::Error::Eof) => decoder.send_eof().ok()?,
                    Err(_) => return None,
                }
                break;
            }
        }
        if decoded.is_corrupt() || decoded.has_decode_errors() {return None}
        let width = decoded.width();
        let height = decoded.height();
        let mut scaler = ffmpeg::software::scaling::context::Context::get(
            decoded.format(), width, height,
            ffmpeg::format::Pixel::RGBA, width, height,
            ffmpeg::software::scaling::flag::Flags::BILINEAR,
        ).ok()?;
        let mut rgba = ffmpeg::frame::Video::empty();
        scaler.run(&decoded, &mut rgba).ok()?;
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            let start = y as usize * rgba.stride(0);
            let end = start + width as usize * 4;
            pixels.extend_from_slice(&rgba.data(0)[start..end]);
        }
        let thumbnail = Arc::new(image::RgbaImage::from_raw(width, height, pixels)?);
        Some(Video{bytes, duration, dimensions: (width, height), thumbnail, format, codec})
    }

    pub fn bytes(&self) -> &Arc<Vec<u8>> {&self.bytes}
    /// The container or track may report an estimated duration.
    pub fn duration(&self) -> Option<Duration> {self.duration}
    pub fn dimensions(&self) -> (u32, u32) {self.dimensions}
    pub fn thumbnail(&self) -> Image {self.thumbnail.clone()}
    /// FFmpeg's detected demuxer name, which may contain format aliases.
    pub fn format(&self) -> &str {&self.format}
    pub fn codec(&self) -> &str {&self.codec}
}

struct Movie {
    bytes: Arc<Vec<u8>>,
    at: u64,
}

impl Read for Movie {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let start = self.at as usize;
        if start >= self.bytes.len() {return Ok(0)}
        let n = buf.len().min(self.bytes.len() - start);
        buf[..n].copy_from_slice(&self.bytes[start..start + n]);
        self.at += n as u64;
        Ok(n)
    }
}

impl Seek for Movie {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let len = self.bytes.len() as i64;
        let next = match pos {
            SeekFrom::Start(n) => n as i64,
            SeekFrom::End(n) => len + n,
            SeekFrom::Current(n) => self.at as i64 + n,
        };
        if next < 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput, "seek",
            ));
        }
        self.at = next as u64;
        Ok(self.at)
    }
}
