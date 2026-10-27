use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ::media::{Sample, Sound};

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use super::AudioSession;

pub struct Speaker {
    output: Output,
    samples: Arc<Mutex<VecDeque<Sample>>>,
}
impl Speaker {
    pub fn new() -> Result<Self, String> {
        let samples = Arc::new(Mutex::new(VecDeque::new()));
        let output = Output::new(AudioSamples {samples: samples.clone()})?;
        Ok(Self {output, samples})
    }
    pub fn format(&self) -> (u32, u16) {(self.output.sample_rate, self.output.channels)}
    pub fn clear(&self) {self.samples.lock().unwrap().clear();}
    pub fn request(&mut self) -> Result<(Duration, bool), String> {
        if let Ok(error) = self.output.errors.try_recv() {return Err(error)}
        let underrun = self.output.underrun.try_recv().ok();
        if let Some(count) = underrun {
            self.output.buffer_size = self.output.buffer_size.max(count);
        }
        let (rate, channels) = self.format();
        let margin = (rate as usize / 100).max(1) * channels as usize;
        let target = (margin * 2).max(self.output.buffer_size + margin);
        let queued = self.samples.lock().unwrap().len();
        let count = target.saturating_sub(queued) / channels as usize;
        Ok((Duration::from_secs_f64(count as f64 / rate as f64), underrun.is_some()))
    }
    pub fn write(&self, sound: Sound) {
        assert_eq!(sound.channels, self.output.channels);
        assert_eq!(sound.samples.len() % sound.channels as usize, 0);
        self.samples.lock().unwrap().extend(sound.samples);
    }
    pub fn set_playing(&mut self, playing: bool) -> Result<(), String> {
        let queued = !self.samples.lock().unwrap().is_empty();
        self.output.set_playing(playing || queued)
    }
}

enum Command {Play(bool, mpsc::SyncSender<Result<(), String>>), Close}

struct AudioSamples {
    samples: Arc<Mutex<VecDeque<Sample>>>,
}
impl AudioSamples {
    fn fill(&mut self, output: &mut [f32]) -> bool {
        let Ok(mut samples) = self.samples.try_lock() else {return false};
        for sample in output {
            let Some(value) = samples.pop_front() else {return false};
            *sample = value.0;
        }
        true
    }
}

struct Output {
    sample_rate: u32,
    channels: u16,
    playing: bool,
    commands: mpsc::Sender<Command>,
    errors: tokio::sync::mpsc::UnboundedReceiver<String>,
    underrun: mpsc::Receiver<usize>,
    buffer_size: usize,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Output {
    fn new(mut samples: AudioSamples) -> Result<Self, String> {
        let (commands, incoming) = mpsc::channel();
        let (errors, events) = tokio::sync::mpsc::unbounded_channel();
        let (ready, initialized) = mpsc::sync_channel(1);
        let (missing, underrun) = mpsc::sync_channel(1);
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
                let stream = device.build_output_stream(&config, move |buffer: &mut [f32], _| {
                    buffer.fill(0.0);
                    if !samples.fill(buffer) {let _ = missing.try_send(buffer.len());}
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
                commands, errors: events, underrun, buffer_size: 0, thread: Some(thread),
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
