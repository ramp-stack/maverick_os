use ffmpeg_next as ffmpeg;
use ffmpeg::{Rescale, codec::packet::Mut};

use std::io::{Cursor, Seek, SeekFrom, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::decoder::Decoder;
use super::{AudioFrame, Sound, VideoFrame};

#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Format {
    Mp4_Mpeg4p2_Aac,
    Mp4_Mpeg4p2,
    Mkv_Ffv1_Flac,
    Mkv_Ffv1_PcmF32le,
    Mkv_Ffv1,
    Mov_ProRes422_PcmS16le,
    Mov_ProRes422,
    M4a_Aac,
    M4a_Alac,
    Wav_PcmS16le,
    Wav_PcmF32le,
    Flac_Flac,
}
impl Format {
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Mp4_Mpeg4p2_Aac | Self::Mp4_Mpeg4p2 => "mp4",
            Self::Mkv_Ffv1_Flac | Self::Mkv_Ffv1_PcmF32le | Self::Mkv_Ffv1 => "mkv",
            Self::Mov_ProRes422_PcmS16le | Self::Mov_ProRes422 => "mov",
            Self::M4a_Aac | Self::M4a_Alac => "m4a",
            Self::Wav_PcmS16le | Self::Wav_PcmF32le => "wav",
            Self::Flac_Flac => "flac",
        }
    }
    pub fn video(&self) -> bool {self.video_codec().is_some()}
    pub fn audio(&self) -> bool {self.audio_codec().is_some()}
    fn container(&self) -> &'static str {
        match self {
            Self::Mp4_Mpeg4p2_Aac | Self::Mp4_Mpeg4p2 => "mp4",
            Self::Mkv_Ffv1_Flac | Self::Mkv_Ffv1_PcmF32le | Self::Mkv_Ffv1 => "matroska",
            Self::Mov_ProRes422_PcmS16le | Self::Mov_ProRes422 => "mov",
            Self::M4a_Aac | Self::M4a_Alac => "ipod",
            Self::Wav_PcmS16le | Self::Wav_PcmF32le => "wav",
            Self::Flac_Flac => "flac",
        }
    }
    fn video_codec(&self) -> Option<ffmpeg::codec::Id> {
        match self {
            Self::Mp4_Mpeg4p2_Aac | Self::Mp4_Mpeg4p2 => Some(ffmpeg::codec::Id::MPEG4),
            Self::Mkv_Ffv1_Flac | Self::Mkv_Ffv1_PcmF32le | Self::Mkv_Ffv1 => {
                Some(ffmpeg::codec::Id::FFV1)
            }
            Self::Mov_ProRes422_PcmS16le | Self::Mov_ProRes422 => Some(ffmpeg::codec::Id::PRORES),
            _ => None,
        }
    }
    fn audio_codec(&self) -> Option<ffmpeg::codec::Id> {
        match self {
            Self::Mp4_Mpeg4p2_Aac | Self::M4a_Aac => Some(ffmpeg::codec::Id::AAC),
            Self::Mkv_Ffv1_Flac | Self::Flac_Flac => Some(ffmpeg::codec::Id::FLAC),
            Self::Mkv_Ffv1_PcmF32le | Self::Wav_PcmF32le => Some(ffmpeg::codec::Id::PCM_F32LE),
            Self::Mov_ProRes422_PcmS16le | Self::Wav_PcmS16le => Some(ffmpeg::codec::Id::PCM_S16LE),
            Self::M4a_Alac => Some(ffmpeg::codec::Id::ALAC),
            _ => None,
        }
    }
}

pub struct Archive {
    output: ffmpeg::format::context::Output,
    bytes: Arc<Mutex<Cursor<Vec<u8>>>>,
    video: Option<ffmpeg::encoder::Video>,
    audio: Option<ffmpeg::encoder::Audio>,
    resampler: Option<ffmpeg::software::resampling::Context>,
    samples: *mut ffmpeg::ffi::AVAudioFifo,
    audio_position: i64,
    audio_end: Duration,
}
impl Archive {
    pub fn new(format: Format, dimensions: Option<(u32, u32)>, audio: Option<(u32, u16)>, empty: bool) -> Result<Self, ffmpeg::Error> {
        Decoder::init()?;
        let bytes = Arc::new(Mutex::new(Cursor::new(Vec::new())));
        let io = ffmpeg::format::context::StreamIo::from_write_seek(Output(bytes.clone()))?;
        let filename = format!("media.{}", format.extension());
        let mut output = ffmpeg::format::output_to_stream(io, Some(&filename), Some(format.container()))?;
        let global = output.format().flags().contains(ffmpeg::format::Flags::GLOBAL_HEADER);
        let video = if let Some(((width, height), id)) = dimensions.zip(format.video_codec()) {
            let codec = if id == ffmpeg::codec::Id::PRORES {
                ffmpeg::encoder::find_by_name("prores_ks")
            } else {ffmpeg::encoder::find(id)}.ok_or(ffmpeg::Error::EncoderNotFound)?;
            let mut encoder = ffmpeg::codec::context::Context::new_with_codec(codec).encoder().video()?;
            encoder.set_width(width);
            encoder.set_height(height);
            let (pixels, ticks) = match id {
                ffmpeg::codec::Id::MPEG4 => (ffmpeg::format::Pixel::YUV420P, 60_000),
                ffmpeg::codec::Id::PRORES => (ffmpeg::format::Pixel::YUV422P10LE, 60_000),
                _ => (ffmpeg::format::Pixel::BGRA, 1_000_000),
            };
            encoder.set_format(pixels);
            encoder.set_time_base((1, ticks));
            encoder.set_max_b_frames(0);
            let mut flags = ffmpeg::codec::Flags::from_bits_retain(ffmpeg::ffi::AV_CODEC_FLAG_FRAME_DURATION);
            if global {flags |= ffmpeg::codec::Flags::GLOBAL_HEADER;}
            if id == ffmpeg::codec::Id::MPEG4 {
                flags |= ffmpeg::codec::Flags::QSCALE;
                encoder.set_global_quality(2 * ffmpeg::ffi::FF_QP2LAMBDA);
            }
            encoder.set_flags(flags);
            let mut options = ffmpeg::Dictionary::new();
            if id == ffmpeg::codec::Id::PRORES {options.set("profile", "2");}
            let encoder = encoder.open_as_with(codec, options)?;
            let mut stream = output.add_stream(codec)?;
            stream.set_parameters(&encoder);
            stream.set_time_base(encoder.time_base());
            Some(encoder)
        } else {None};
        let audio = if let Some(((rate, channels), id)) = audio.zip(format.audio_codec()) {
            let codec = ffmpeg::encoder::find(id).ok_or(ffmpeg::Error::EncoderNotFound)?;
            let supported = codec.audio()?;
            let rate = supported.rates().and_then(|rates| {
                rates.min_by_key(|candidate| i64::from(*candidate).abs_diff(i64::from(rate)))
            }).unwrap_or(rate as i32);
            let layout = supported.channel_layouts().map_or_else(
                || ffmpeg::ChannelLayout::default(i32::from(channels)),
                |layouts| layouts.best(i32::from(channels)),
            );
            let packed = ffmpeg::format::sample::Type::Packed;
            let planar = ffmpeg::format::sample::Type::Planar;
            let samples = match id {
                ffmpeg::codec::Id::AAC => ffmpeg::format::Sample::F32(planar),
                ffmpeg::codec::Id::ALAC => ffmpeg::format::Sample::I32(planar),
                ffmpeg::codec::Id::FLAC => ffmpeg::format::Sample::I32(packed),
                ffmpeg::codec::Id::PCM_S16LE => ffmpeg::format::Sample::I16(packed),
                _ => ffmpeg::format::Sample::F32(packed),
            };
            let mut encoder = ffmpeg::codec::context::Context::new_with_codec(codec).encoder().audio()?;
            encoder.set_rate(rate);
            encoder.set_channel_layout(layout);
            encoder.set_format(samples);
            encoder.set_time_base((1, rate));
            if id == ffmpeg::codec::Id::AAC {encoder.set_bit_rate(96_000 * layout.channels() as usize);}
            if matches!(id, ffmpeg::codec::Id::FLAC | ffmpeg::codec::Id::ALAC) {
                unsafe {(*encoder.as_mut_ptr()).bits_per_raw_sample = 24;}
            }
            if global {encoder.set_flags(ffmpeg::codec::Flags::GLOBAL_HEADER);}
            let encoder = encoder.open_as(codec)?;
            let mut stream = output.add_stream(codec)?;
            stream.set_parameters(&encoder);
            stream.set_time_base((1, rate));
            Some(encoder)
        } else {None};
        let mut options = ffmpeg::Dictionary::new();
        if empty && matches!(format, Format::M4a_Aac | Format::M4a_Alac) {
            options.set("movflags", "empty_moov");
            options.set("use_editlist", "0");
        }
        output.write_header_with(options)?;
        if empty && matches!(format, Format::Wav_PcmS16le | Format::Wav_PcmF32le) {
            let mut packet = ffmpeg::Packet::new(0);
            packet.set_stream(0);
            packet.set_pts(Some(0));
            packet.set_dts(Some(0));
            // Initialize WAV timing without adding samples; the wrapper rejects empty packets.
            let result = unsafe {
                ffmpeg::ffi::av_interleaved_write_frame(output.as_mut_ptr(), packet.as_mut_ptr())
            };
            if result < 0 {return Err(ffmpeg::Error::from(result))}
        }
        let samples = audio.as_ref().map_or(std::ptr::null_mut(), |encoder| unsafe {
            let samples = ffmpeg::ffi::av_audio_fifo_alloc(
                encoder.format().into(), i32::from(encoder.channels()), 1,
            );
            assert!(!samples.is_null());
            samples
        });
        Ok(Self {
            output, bytes, video, audio, samples, resampler: None,
            audio_position: 0, audio_end: Duration::ZERO,
        })
    }
    pub fn video(&mut self, frame: &VideoFrame) -> Result<(), ffmpeg::Error> {
        let encoder = self.video.as_mut().ok_or(ffmpeg::Error::InvalidData)?;
        let rgba = &*frame.image;
        let image = if rgba.dimensions() == (encoder.width(), encoder.height()) {
            rgba.clone()
        } else {
            Arc::new(image::imageops::resize(
                &**rgba, encoder.width(), encoder.height(), image::imageops::FilterType::Triangle,
            ))
        };
        let (width, height) = (encoder.width(), encoder.height());
        let mut rgba = ffmpeg::frame::Video::new(ffmpeg::format::Pixel::RGBA, width, height);
        let stride = rgba.stride(0);
        for (y, row) in image.as_raw().chunks_exact(width as usize * 4).enumerate() {
            rgba.data_mut(0)[y * stride..y * stride + row.len()].copy_from_slice(row);
        }
        let mut raw = ffmpeg::frame::Video::empty();
        ffmpeg::software::scaling::Context::get(
            ffmpeg::format::Pixel::RGBA, width, height, encoder.format(), width, height,
            ffmpeg::software::scaling::flag::Flags::BILINEAR,
        )?.run(&rgba, &mut raw)?;
        let position = i64::try_from(frame.position.as_micros()).unwrap()
            .rescale((1, 1_000_000), encoder.time_base());
        let duration = i64::try_from(frame.duration.as_micros()).unwrap()
            .rescale((1, 1_000_000), encoder.time_base());
        raw.set_pts(Some(position));
        unsafe {
            (*raw.as_mut_ptr()).duration = duration;
            (*raw.as_mut_ptr()).quality = (*encoder.as_ptr()).global_quality;
        }
        encoder.send_frame(&raw)?;
        Self::drain(&mut self.output, encoder, 0)
    }
    pub fn audio(&mut self, frame: &AudioFrame) -> Result<(), ffmpeg::Error> {
        self.silence(frame.position)?;
        let format = ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Packed);
        let layout = ffmpeg::ChannelLayout::default(i32::from(frame.sound.channels));
        if self.resampler.as_ref().is_some_and(|resampler| {
            resampler.input().rate != frame.sound.sample_rate || resampler.input().channel_layout != layout
        }) {self.flush_resampler()?;}
        let encoder = self.audio.as_ref().ok_or(ffmpeg::Error::InvalidData)?;
        if self.resampler.is_none() {
            self.resampler = Some(ffmpeg::software::resampling::Context::get(
                format, layout, frame.sound.sample_rate,
                encoder.format(), encoder.channel_layout(), encoder.rate(),
            )?);
        }
        let count = frame.sound.samples.len() / frame.sound.channels as usize;
        let mut input = ffmpeg::frame::Audio::new(format, count, layout);
        input.set_rate(frame.sound.sample_rate);
        input.plane_mut::<f32>(0).copy_from_slice(&frame.sound.samples);
        let resampler = self.resampler.as_mut().unwrap();
        let delay = resampler.delay().map_or(0, |delay| delay.output.max(0) as usize);
        let capacity = (count as f64 * encoder.rate() as f64 / frame.sound.sample_rate as f64).ceil();
        let mut converted = ffmpeg::frame::Audio::new(
            encoder.format(), capacity as usize + delay + 32, encoder.channel_layout(),
        );
        resampler.run(&input, &mut converted)?;
        self.audio_end = frame.position + frame.sound.duration();
        self.queue_audio(&mut converted)
    }
    pub fn silence(&mut self, end: Duration) -> Result<(), ffmpeg::Error> {
        let encoder = self.audio.as_ref().ok_or(ffmpeg::Error::InvalidData)?;
        let (sample_rate, channels) = (encoder.rate(), encoder.channels());
        loop {
            let remaining = end.saturating_sub(self.audio_end).as_secs_f64();
            let count = (remaining * sample_rate as f64).round() as usize;
            if count == 0 {break}
            let count = count.min(sample_rate as usize);
            self.audio(&AudioFrame {
                position: self.audio_end, sound: Sound {
                    sample_rate, channels, samples: Arc::new(vec![0.0; count * channels as usize]),
                },
            })?;
        }
        Ok(())
    }
    pub fn finish(mut self) -> Result<Vec<u8>, ffmpeg::Error> {
        self.flush_resampler()?;
        self.encode_audio(true)?;
        if let Some(encoder) = &mut self.video {
            encoder.send_eof()?;
            Self::drain(&mut self.output, encoder, 0)?;
        }
        if let Some(encoder) = &mut self.audio {
            encoder.send_eof()?;
            Self::drain(&mut self.output, encoder, usize::from(self.video.is_some()))?;
        }
        self.output.write_trailer()?;
        let bytes = self.bytes.clone();
        drop(self);
        Ok(Arc::try_unwrap(bytes).unwrap().into_inner().unwrap().into_inner())
    }
    fn flush_resampler(&mut self) -> Result<(), ffmpeg::Error> {
        let Some(mut resampler) = self.resampler.take() else {return Ok(())};
        let encoder = self.audio.as_ref().unwrap();
        let (format, rate, layout) = (encoder.format(), encoder.rate(), encoder.channel_layout());
        let end = (self.audio_end.as_secs_f64() * rate as f64).round() as i64;
        loop {
            let delay = resampler.delay().map_or(0, |delay| delay.output.max(0) as usize);
            let mut converted = ffmpeg::frame::Audio::new(format, delay + 32, layout);
            resampler.flush(&mut converted)?;
            if converted.samples() == 0 {break}
            let queued = unsafe {ffmpeg::ffi::av_audio_fifo_size(self.samples)};
            let remaining = (end - self.audio_position - i64::from(queued)).max(0) as usize;
            converted.set_samples(converted.samples().min(remaining));
            self.queue_audio(&mut converted)?;
        }
        Ok(())
    }
    fn queue_audio(&mut self, frame: &mut ffmpeg::frame::Audio) -> Result<(), ffmpeg::Error> {
        if frame.samples() == 0 {return Ok(())}
        let written = unsafe {
            ffmpeg::ffi::av_audio_fifo_write(
                self.samples, (*frame.as_mut_ptr()).extended_data.cast(), frame.samples() as i32,
            )
        };
        if written < 0 {return Err(ffmpeg::Error::from(written))}
        self.encode_audio(false)
    }
    fn encode_audio(&mut self, finishing: bool) -> Result<(), ffmpeg::Error> {
        let Some(encoder) = &mut self.audio else {return Ok(())};
        loop {
            let available = unsafe {ffmpeg::ffi::av_audio_fifo_size(self.samples)} as usize;
            let size = encoder.frame_size() as usize;
            if available == 0 || (!finishing && size > 0 && available < size) {return Ok(())}
            let count = if size == 0 {available} else {available.min(size)};
            let mut frame = ffmpeg::frame::Audio::new(encoder.format(), count, encoder.channel_layout());
            frame.set_rate(encoder.rate());
            frame.set_pts(Some(self.audio_position));
            let read = unsafe {
                ffmpeg::ffi::av_audio_fifo_read(
                    self.samples, (*frame.as_mut_ptr()).extended_data.cast(), count as i32,
                )
            };
            if read < 0 {return Err(ffmpeg::Error::from(read))}
            assert_eq!(read as usize, count);
            self.audio_position += count as i64;
            encoder.send_frame(&frame)?;
            Self::drain(&mut self.output, encoder, usize::from(self.video.is_some()))?;
        }
    }
    fn drain(output: &mut ffmpeg::format::context::Output, encoder: &mut ffmpeg::encoder::Encoder, index: usize) -> Result<(), ffmpeg::Error> {
        loop {
            let mut packet = ffmpeg::Packet::empty();
            match encoder.receive_packet(&mut packet) {
                Ok(()) => {},
                Err(ffmpeg::Error::Eof) => return Ok(()),
                Err(ffmpeg::Error::Other {errno}) if errno == ffmpeg::error::EAGAIN => return Ok(()),
                Err(error) => return Err(error),
            }
            packet.set_stream(index);
            packet.rescale_ts(encoder.time_base(), output.stream(index).unwrap().time_base());
            packet.set_position(-1);
            packet.write_interleaved(output)?;
        }
    }
}
impl Drop for Archive {
    fn drop(&mut self) {unsafe {ffmpeg::ffi::av_audio_fifo_free(self.samples);}}
}

struct Output(Arc<Mutex<Cursor<Vec<u8>>>>);
impl Write for Output {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {self.0.lock().unwrap().write(bytes)}
    fn flush(&mut self) -> std::io::Result<()> {self.0.lock().unwrap().flush()}
}
impl Seek for Output {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.0.lock().unwrap().seek(position)
    }
}
