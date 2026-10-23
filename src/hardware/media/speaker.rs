use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ffmpeg_next as ffmpeg;

use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use super::super::AudioSession;
use super::{AudioFrame, Media, Reader};
use super::playback::{Clock, OutputClock, OutputSpan, Playback, Request};
use crate::shared::{Service, Services, Shared};

#[derive(Debug)]
pub struct Session {
    init: Init,
}
impl Session {
    pub fn new(source: &Media, clock: Clock) -> Result<Arc<Self>, String> {
        let init = Init {id: rand::random(), source: source.clone(), clock: clock.clone()};
        let mut ready = clock.shared();
        clock.source(source.0.status());
        let session = Arc::new(Self {init: init.clone()});
        Services::start::<PlaybackService>(init);
        loop {
            let snapshot = ready.load();
            if snapshot.ready {return snapshot.error.clone().map_or(Ok(session), Err)}
            drop(snapshot);
            futures::executor::block_on(ready.next());
        }
    }
}
impl Drop for Session {
    fn drop(&mut self) {Services::shutdown::<PlaybackService>(&self.init);}
}

enum Command {Play(bool, mpsc::SyncSender<Result<(), String>>), Close}

struct AudioChunk {
    revision: u64,
    edit: u64,
    position: f64,
    step: f64,
    samples: Vec<f32>,
}

struct OutputInfo {
    sample_rate: u32,
    channels: u16,
    delay: Duration,
}

#[derive(Clone, Debug)]
struct Init {
    id: u128,
    source: Media,
    clock: Clock,
}
impl Hash for Init {
    fn hash<H: Hasher>(&self, state: &mut H) {self.id.hash(state)}
}

struct PlaybackService {
    reopen: bool,
    source: Media,
    clock: Clock,
    state: Shared<Playback>,
    audio: Option<AudioPlayback>,
}
impl Service for PlaybackService {
    type Init = Init;
    type Predicate = Option<String>;

    async fn init(init: Init) -> Self {
        let state = init.clock.shared();
        let result = tokio::task::block_in_place(|| AudioPlayback::new(&init.source));
        let (audio, error) = match result {
            Ok(audio) => (Some(audio), None), Err(error) => (None, Some(error)),
        };
        let service = Self {reopen: false, source: init.source, clock: init.clock, state, audio};
        let clock = service.audio.as_ref().map(|audio| audio.clock.clone());
        service.state.update(Request::Ready(clock, Some(service.source.duration()), error));
        service
    }
    async fn predicate(&mut self) -> Option<String> {
        if let Some(audio) = &mut self.audio {
            if !self.reopen {
                let playing = self.state.load().playing;
                let result = tokio::task::block_in_place(|| audio.output.set_playing(playing));
                if let Err(error) = result {return Some(error)}
            }
            if let Ok(error) = audio.output.errors.try_recv() {return Some(error)}
        }
        tokio::select! {
            error = async {
                match &mut self.audio {
                    Some(audio) => Some(audio.output.errors.recv().await.expect("Speaker stopped")),
                    None => std::future::pending().await,
                }
            } => error,
            _ = self.state.next() => None,
            _ = tokio::time::sleep(Duration::from_millis(5)) => None,
        }
    }
    async fn handle(&mut self, error: Option<String>) {
        let source = &self.source.0;
        if let Some(error) = source.error() {
            if self.clock.error().is_none() {self.clock.fail(self.clock.time().revision, error);}
            return
        }
        let status = source.status();
        self.clock.source(status);
        let state = self.state.load();
        if let Some(error) = error {
            self.reopen = self.audio.is_some();
            if let Some(audio) = &mut self.audio {let _ = audio.output.set_playing(false);}
            self.state.update(Request::Error(state.revision, error));
            return
        }
        if self.reopen && state.playing {
            if let Some(audio) = &mut self.audio {
                let (samples, sender) = AudioSamples::new(audio.clock.clone());
                let output = tokio::task::block_in_place(|| Output::new(samples));
                match output {
                    Ok(output) => {
                        audio.output = output;
                        audio.sender = sender;
                        audio.revision = u64::MAX;
                        self.reopen = false;
                    }
                    Err(error) => {self.state.update(Request::Error(state.revision, error)); return}
                }
            }
        }
        if let Some(audio) = self.audio.as_mut().filter(|_| !self.reopen) {
            if state.playing || state.seek.is_some() {
                let result = tokio::task::block_in_place(|| {
                    audio.fill(
                        state.revision, state.seek.unwrap_or(state.position),
                        state.speed, status.edit,
                    )
                });
                if let Err(error) = result {
                    self.state.update(Request::Error(state.revision, error));
                    return
                }
                if state.seek.is_some() {self.state.update(Request::Seeked(state.revision));}
            }
        } else if self.reopen && state.seek.is_some() {
            self.state.update(Request::Seeked(state.revision));
        }
    }
}

struct AudioPlayback {
    reader: Reader,
    output: Output,
    clock: Arc<Mutex<OutputClock>>,
    sender: mpsc::SyncSender<AudioChunk>,
    resampler: Option<ffmpeg::software::resampling::Context>,
    pending: Option<AudioChunk>,
    decoded: Option<AudioChunk>,
    incoming: Option<AudioFrame>,
    offset: usize,
    buffered: f64,
    cycle: u64,
    loop_duration: Option<f64>,
    revision: u64,
    edit: u64,
    speed: f64,
    target: f64,
    end: f64,
    decoded_end: f64,
}
impl AudioPlayback {
    fn new(source: &Media) -> Result<Self, String> {
        let media = &source.0;
        let status = media.status();
        let clock = Arc::new(Mutex::new(OutputClock {
            revision: 0, edit: status.edit, submitted: 0.0, presented: 0.0, playing: false,
            speed: 1.0, volume: 1.0, drained: false, segments: VecDeque::new(),
            visible_revision: 0, cycle: 0, loop_duration: None, limit: status.available.as_secs_f64(),
        }));
        let (samples, sender) = AudioSamples::new(clock.clone());
        let output = Output::new(samples)?;
        Ok(Self {
            reader: media.audio_reader(), output, clock, sender,
            resampler: None, pending: None, decoded: None, incoming: None, offset: 0, buffered: 0.0,
            cycle: 0, loop_duration: None, revision: 0, edit: 0, speed: 1.0,
            target: 0.0, end: 0.0, decoded_end: 0.0,
        })
    }
    fn fill(&mut self, revision: u64, position: f64, speed: f64, edit: u64) -> Result<(), String> {
        if revision != self.revision || edit != self.edit {
            let position = if revision == self.revision {
                self.clock.lock().unwrap().submitted_position()
            } else {position};
            self.reader.seek(Duration::from_secs_f64(position));
            self.resampler = None;
            self.pending = None;
            self.decoded = None;
            self.incoming = None;
            self.offset = 0;
            self.buffered = position;
            self.cycle = 0;
            self.loop_duration = self.clock.lock().unwrap().loop_duration;
            self.revision = revision;
            self.edit = edit;
            self.speed = speed;
            self.target = position;
            self.end = position;
            self.decoded_end = position;
        }
        let mut drained = false;
        for _ in 0..8 {
            if let Some(chunk) = self.pending.take() {
                match self.sender.try_send(chunk) {
                    Ok(()) => {},
                    Err(mpsc::TrySendError::Full(chunk)) => {self.pending = Some(chunk); break}
                    Err(mpsc::TrySendError::Disconnected(_)) => {
                        return Err("Audio output device stopped".into())
                    }
                }
            }
            self.pending = self.next()?;
            if self.pending.is_none() {drained = true; break}
        }
        if drained && self.pending.is_none() {
            let mut time = self.clock.lock().unwrap();
            if time.revision == self.revision && time.edit == self.edit {time.drained = true;}
        }
        Ok(())
    }
    fn next(&mut self) -> Result<Option<AudioChunk>, String> {
        let channels = self.output.channels as usize;
        let count = (self.output.sample_rate / 20).max(1) as usize * channels;
        let step = self.speed / self.output.sample_rate as f64;
        let position = self.buffered;
        let mut samples = Vec::with_capacity(count);
        while samples.len() < count {
            if self.decoded.is_none() {self.decoded = self.decode()?; self.offset = 0;}
            let Some(chunk) = &self.decoded else {break};
            let cursor = position + (samples.len() / channels) as f64 * step;
            let start = chunk.position + (self.offset / channels) as f64 * step;
            let gap = ((start - cursor) / step).round().max(0.0) as usize;
            if gap > 0 {
                let count = (count - samples.len()).min(gap.saturating_mul(channels));
                samples.resize(samples.len() + count, 0.0);
                continue
            }
            let skip = ((cursor - start) / step).round().max(0.0) as usize;
            self.offset = self.offset.saturating_add(skip.saturating_mul(channels));
            if self.offset >= chunk.samples.len() {self.decoded = None; continue}
            let count = (count - samples.len()).min(chunk.samples.len() - self.offset);
            samples.extend_from_slice(&chunk.samples[self.offset..self.offset + count]);
            self.offset += count;
        }
        if samples.is_empty() {return Ok(None)}
        self.buffered += (samples.len() / channels) as f64 * step;
        Ok(Some(AudioChunk {
            revision: self.revision, edit: self.edit, position, step, samples,
        }))
    }
    fn decode(&mut self) -> Result<Option<AudioChunk>, String> {
        let (sample_rate, channels) = (self.output.sample_rate, self.output.channels);
        let format = ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Packed);
        let output_layout = ffmpeg::ChannelLayout::default(i32::from(channels));
        loop {
            let frame = match self.incoming.take() {
                Some(frame) => Some(frame), None => self.reader.audio()?,
            };
            let frame = frame.and_then(|mut frame| {
                let rate = frame.sound.sample_rate as f64;
                let mut position = frame.position.as_secs_f64();
                if let Some(mut duration) = self.loop_duration {
                    if self.cycle == 0 && self.decoded_end == self.target && self.resampler.is_none() {
                        let mut clock = self.clock.lock().unwrap();
                        let rounded = (clock.limit * rate).round() / rate;
                        duration = if (rounded - clock.limit).abs() <= 0.5e-9 {
                            rounded
                        } else {clock.limit};
                        self.loop_duration = Some(duration);
                        if clock.revision == self.revision {clock.loop_duration = Some(duration);}
                    }
                    let rounded = (position * rate).round() / rate;
                    if (rounded - position).abs() <= 0.5e-9 {position = rounded;}
                    let first = ((position + self.cycle as f64 * duration) * rate - 1e-9).ceil();
                    let end = (((self.cycle + 1) as f64 * duration) * rate - 1e-9).ceil();
                    let count = (end - first).max(0.0) as usize * frame.sound.channels as usize;
                    if count == 0 {return None}
                    if count < frame.sound.samples.len() {
                        Arc::make_mut(&mut frame.sound.samples).truncate(count);
                    }
                    position = first / rate;
                }
                Some((position, frame))
            });
            match frame {
                Some((position, AudioFrame {position: start, sound})) => {
                    let (input_rate, input_channels) = (sound.sample_rate, sound.channels);
                    let source_rate = input_rate as f64;
                    let layout = ffmpeg::ChannelLayout::default(i32::from(input_channels));
                    let mut frame = ffmpeg::frame::Audio::new(
                        format, sound.samples.len() / input_channels as usize, layout,
                    );
                    frame.plane_mut::<f32>(0).copy_from_slice(&sound.samples);
                    let gap = position > self.decoded_end + 0.5 / source_rate;
                    let rate = (source_rate * self.speed).round();
                    if !(1.0..=i32::MAX as f64).contains(&rate) {
                        return Err("Playback speed exceeds the supported audio sample rate".into())
                    }
                    frame.set_rate(rate as u32);
                    let reset = self.resampler.as_ref().is_none_or(|resampler| {
                        let input = resampler.input();
                        input.format != frame.format() || input.channel_layout != layout
                            || input.rate != rate as u32 || gap
                    });
                    if reset && self.resampler.is_some() {
                        self.incoming = Some(AudioFrame {position: start, sound});
                        if let Some(chunk) = self.flush()? {return Ok(Some(chunk))}
                        continue
                    }
                    self.decoded_end = position + frame.samples() as f64 / source_rate;
                    if reset {
                        self.resampler = Some(ffmpeg::software::resampling::Context::get(
                            frame.format(), layout, rate as u32,
                            format, output_layout, sample_rate,
                        ).map_err(|error| error.to_string())?);
                        self.end = position;
                    }
                    let resampler = self.resampler.as_mut().unwrap();
                    let delay = resampler.delay().map_or(0, |delay| delay.output.max(0) as usize);
                    let capacity = (frame.samples() as f64 * sample_rate as f64 / rate).ceil();
                    let mut converted = ffmpeg::frame::Audio::new(
                        format, capacity as usize + delay + 32, output_layout,
                    );
                    resampler.run(&frame, &mut converted).map_err(|error| error.to_string())?;
                    if converted.samples() == 0 {continue}
                    let step = self.speed / sample_rate as f64;
                    let position = self.end;
                    self.end += converted.samples() as f64 * step;
                    let skip = ((self.target - position).max(0.0) / step).ceil() as usize;
                    if skip >= converted.samples() {continue}
                    let samples = converted.plane::<f32>(0);
                    return Ok(Some(AudioChunk {
                        revision: self.revision, edit: self.edit,
                        position: position + skip as f64 * step, step,
                        samples: samples[skip * channels as usize..].to_vec(),
                    }))
                }
                None => {
                    let end = self.loop_duration.map(|duration| (self.cycle + 1) as f64 * duration);
                    if let Some(end) = end {
                        if self.resampler.is_some() && self.decoded_end >= end - 1e-9 {
                            self.cycle += 1;
                            self.reader.seek(Duration::ZERO);
                            continue
                        }
                    }
                    if let Some(chunk) = self.flush()? {return Ok(Some(chunk))}
                    let Some(end) = end else {return Ok(None)};
                    self.cycle += 1;
                    self.reader.seek(Duration::ZERO);
                    return Ok(Some(AudioChunk {
                        revision: self.revision, edit: self.edit, position: end,
                        step: self.speed / sample_rate as f64, samples: Vec::new(),
                    }))
                }
            }
        }
    }
    fn flush(&mut self) -> Result<Option<AudioChunk>, String> {
        let Some(resampler) = self.resampler.as_mut() else {return Ok(None)};
        let capacity = resampler.delay().map_or(0, |delay| delay.output.max(0) as usize);
        let format = ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Packed);
        let layout = ffmpeg::ChannelLayout::default(i32::from(self.output.channels));
        let mut converted = ffmpeg::frame::Audio::new(format, capacity + 32, layout);
        converted.set_rate(self.output.sample_rate);
        resampler.flush(&mut converted).map_err(|error| error.to_string())?;
        if converted.samples() == 0 {self.resampler = None; return Ok(None)}
        let position = self.end;
        let step = self.speed / self.output.sample_rate as f64;
        self.end += converted.samples() as f64 * step;
        Ok(Some(AudioChunk {
            revision: self.revision, edit: self.edit, position, step,
            samples: converted.plane::<f32>(0).to_vec(),
        }))
    }
}

struct AudioSamples {
    clock: Arc<Mutex<OutputClock>>,
    receiver: mpsc::Receiver<AudioChunk>,
    chunk: Option<AudioChunk>,
    offset: usize,
}
impl AudioSamples {
    fn new(clock: Arc<Mutex<OutputClock>>) -> (Self, mpsc::SyncSender<AudioChunk>) {
        let (sender, receiver) = mpsc::sync_channel(8);
        (Self {clock, receiver, chunk: None, offset: 0}, sender)
    }
    fn fill(&mut self, output: &mut [f32], info: OutputInfo) {
        let started = Instant::now();
        let (rate, channels) = (info.sample_rate, info.channels as usize);
        let Ok(mut time) = self.clock.try_lock() else {return};
        time.time();
        let position = time.submitted;
        let mut advanced = false;
        for frame in output.chunks_exact_mut(channels) {
            if time.loop_duration.is_none() && time.submitted >= time.limit {break}
            let before = time.submitted;
            loop {
                let usable = self.chunk.as_ref().is_some_and(|chunk| {
                    chunk.revision == time.revision && chunk.edit == time.edit
                        && self.offset < chunk.samples.len()
                });
                if !usable {
                    self.chunk = self.receiver.try_recv().ok();
                    self.offset = 0;
                    if self.chunk.is_none() {
                        if time.playing && time.drained {
                            time.submitted += time.speed / rate as f64;
                        }
                        break
                    }
                    continue
                }
                if !time.playing {return}
                let chunk = self.chunk.as_ref().unwrap();
                let skip = ((time.submitted - chunk.position) / chunk.step)
                    .floor().max(0.0) as usize;
                self.offset = self.offset.max(skip.saturating_mul(channels));
                if self.offset >= chunk.samples.len() {continue}
                let position = chunk.position + (self.offset / channels) as f64 * chunk.step;
                if time.submitted + chunk.step * 0.5 < position {
                    time.submitted = (time.submitted + chunk.step).min(position);
                    break
                }
                for (sample, value) in frame.iter_mut().zip(&chunk.samples[self.offset..]) {
                    *sample = (f64::from(*value) * time.volume).clamp(-1.0, 1.0) as f32;
                }
                self.offset += channels;
                time.submitted = position + chunk.step;
                break
            }
            if time.loop_duration.is_none() {time.submitted = time.submitted.min(time.limit);}
            advanced |= time.submitted != before;
        }
        if advanced {
            let segment = OutputSpan {
                at: started + info.delay, position, end: time.submitted,
                speed: time.speed, revision: time.revision, loop_duration: time.loop_duration,
            };
            time.segments.push_back(segment);
        }
    }
}

struct Output {
    sample_rate: u32,
    channels: u16,
    playing: bool,
    commands: mpsc::Sender<Command>,
    errors: tokio::sync::mpsc::UnboundedReceiver<String>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Output {
    fn new(mut samples: AudioSamples) -> Result<Self, String> {
        let (commands, incoming) = mpsc::channel();
        let (errors, events) = tokio::sync::mpsc::unbounded_channel();
        let (ready, initialized) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new().name("speaker".into()).spawn(move || {
            let result = (|| {
                let device = cpal::default_host().default_output_device()
                    .ok_or_else(|| "No audio output device".to_string())?;
                let default = device.default_output_config().map_err(|error| error.to_string())?;
                let supported = device.supported_output_configs()
                    .map_err(|error| error.to_string())?
                    .find(|config| {
                        config.sample_format() == cpal::SampleFormat::F32
                            && config.channels() == default.channels()
                            && config.min_sample_rate() <= default.sample_rate()
                            && config.max_sample_rate() >= default.sample_rate()
                    }).ok_or_else(|| "Audio output does not support float samples".to_string())?;
                let config = supported.with_sample_rate(default.sample_rate()).config();
                let (sample_rate, channels) = (config.sample_rate.0, config.channels);
                let stream = device.build_output_stream(&config, move |buffer: &mut [f32], info| {
                    let stamp = info.timestamp();
                    let delay = stamp.playback.duration_since(&stamp.callback).unwrap_or_default();
                    buffer.fill(0.0);
                    samples.fill(buffer, OutputInfo {sample_rate, channels, delay});
                }, move |error| {let _ = errors.send(error.to_string());}, None)
                    .map_err(|error| error.to_string())?;
                stream.pause().map_err(|error| error.to_string())?;
                Ok((stream, sample_rate, channels))
            })();
            let (stream, sample_rate, channels) = match result {
                Ok(output) => output,
                Err(error) => {let _ = ready.send(Err(error)); return}
            };
            if ready.send(Ok((sample_rate, channels))).is_err() {return}
            let mut session = None;
            while let Ok(Command::Play(playing, reply)) = incoming.recv() {
                let result = (|| {
                    if playing && session.is_none() {
                        let active = AudioSession::new()?;
                        stream.play().map_err(|error| error.to_string())?;
                        session = Some(active);
                    } else if !playing && session.is_some() {
                        stream.pause().map_err(|error| error.to_string())?;
                        session = None;
                    }
                    Ok(())
                })();
                let _ = reply.send(result);
            }
            drop(stream);
        }).map_err(|error| error.to_string())?;
        match initialized.recv() {
            Ok(Ok((sample_rate, channels))) => Ok(Self {
                sample_rate, channels, playing: false,
                commands, errors: events, thread: Some(thread),
            }),
            result => {
                thread.join().unwrap();
                Err(match result {
                    Ok(Err(error)) => error, Err(error) => error.to_string(), _ => unreachable!(),
                })
            }
        }
    }
    fn set_playing(&mut self, playing: bool) -> Result<(), String> {
        if self.playing == playing {return Ok(())}
        let (reply, response) = mpsc::sync_channel(1);
        self.commands.send(Command::Play(playing, reply)).map_err(|error| error.to_string())?;
        response.recv().map_err(|error| error.to_string())??;
        self.playing = playing;
        Ok(())
    }
}
impl Drop for Output {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Close);
        if let Some(thread) = self.thread.take() {thread.join().unwrap();}
    }
}
