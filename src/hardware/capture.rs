use ::media::{AudioFrame, Image, MediaFrame, Sound, VideoFrame};
use tokio::sync::broadcast;

use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::Duration;

use super::{Hardware, _Hardware};

#[derive(Clone)]
pub enum Frame {
    Image(Image, Duration),
    Sound(Sound, Duration),
}

#[derive(Clone)]
pub struct Feed {
    sender: Option<broadcast::Sender<(Duration, Arc<Frame>)>>,
    camera: bool,
    microphone: bool,
    origin: Option<f64>,
    error: Option<String>,
}
impl Feed {
    pub fn new(camera: bool, microphone: bool) -> (Self, broadcast::Receiver<(Duration, Arc<Frame>)>) {
        let (sender, frames) = broadcast::channel(8);
        (Self {sender: Some(sender), camera, microphone, origin: None, error: None}, frames)
    }
    pub fn camera(&self) -> bool {self.camera}
    pub fn microphone(&self) -> bool {self.microphone}
    pub fn is_open(&self) -> bool {self.sender.is_some()}
    pub fn started(&self) -> bool {self.origin.is_some() && self.is_open()}
    pub fn start(&mut self, origin: f64) {self.origin = Some(origin);}
    pub fn error(&self) -> Option<String> {self.error.clone()}

    pub fn send(&self, timestamp: f64, frame: &Arc<Frame>) {
        let Some(origin) = self.origin else {return};
        let Some(sender) = &self.sender else {return};
        let needed = match frame.as_ref() {
            Frame::Image(_, _) => self.camera, Frame::Sound(_, _) => self.microphone,
        };
        if !needed {return}
        if let Ok(timestamp) = Duration::try_from_secs_f64(timestamp - origin) {
            let _ = sender.send((timestamp, frame.clone()));
        }
    }
    pub fn fail(&mut self, error: String) {
        if self.sender.take().is_some() {self.error = Some(error);}
    }
}

pub struct Camera {
    frames: broadcast::Receiver<(Duration, Arc<Frame>)>,
    subscription: Rc<Subscription>,
}
impl Camera {
    pub(crate) fn new(hardware: &Hardware, id: u128, frames: broadcast::Receiver<(Duration, Arc<Frame>)>) -> Self {
        Self {frames, subscription: Rc::new(Subscription {owner: Rc::downgrade(&hardware.0), id})}
    }
    pub fn error(&self) -> Option<String> {self.subscription.error()}
    pub async fn next(&mut self) -> Option<VideoFrame> {
        self.subscription.next(&mut self.frames).await?.video
    }
}
impl Clone for Camera {
    fn clone(&self) -> Self {
        Self {frames: self.frames.resubscribe(), subscription: self.subscription.clone()}
    }
}

pub struct Microphone {
    frames: broadcast::Receiver<(Duration, Arc<Frame>)>,
    subscription: Rc<Subscription>,
}
impl Microphone {
    pub(crate) fn new(hardware: &Hardware, id: u128, frames: broadcast::Receiver<(Duration, Arc<Frame>)>) -> Self {
        Self {frames, subscription: Rc::new(Subscription {owner: Rc::downgrade(&hardware.0), id})}
    }
    pub fn error(&self) -> Option<String> {self.subscription.error()}
    pub async fn next(&mut self) -> Option<AudioFrame> {
        self.subscription.next(&mut self.frames).await?.audio
    }
}
impl Clone for Microphone {
    fn clone(&self) -> Self {
        Self {frames: self.frames.resubscribe(), subscription: self.subscription.clone()}
    }
}

pub struct Capture {
    frames: broadcast::Receiver<(Duration, Arc<Frame>)>,
    subscription: Rc<Subscription>,
}
impl Capture {
    pub(crate) fn new(hardware: &Hardware, id: u128, frames: broadcast::Receiver<(Duration, Arc<Frame>)>) -> Self {
        Self {frames, subscription: Rc::new(Subscription {owner: Rc::downgrade(&hardware.0), id})}
    }
    pub fn error(&self) -> Option<String> {self.subscription.error()}
    pub async fn next(&mut self) -> Option<MediaFrame> {
        self.subscription.next(&mut self.frames).await
    }
}
impl Clone for Capture {
    fn clone(&self) -> Self {
        Self {frames: self.frames.resubscribe(), subscription: self.subscription.clone()}
    }
}

struct Subscription {
    owner: Weak<_Hardware>,
    id: u128,
}
impl Subscription {
    fn error(&self) -> Option<String> {
        let owner = self.owner.upgrade()?;
        let hub = owner.capture.get().unwrap();
        hub.error(self.id)
    }
    async fn next(&self, frames: &mut broadcast::Receiver<(Duration, Arc<Frame>)>) -> Option<MediaFrame> {
        loop {
            match frames.recv().await {
                Ok((timestamp, frame)) => return Some(match Arc::unwrap_or_clone(frame) {
                    Frame::Image(image, duration) => MediaFrame {
                        video: Some(VideoFrame {
                            timestamp, duration, image, corrupted: false,
                        }), audio: None,
                    },
                    Frame::Sound(sound, duration) => MediaFrame {
                        audio: Some(AudioFrame {
                            timestamp, duration, sound, corrupted: false,
                        }), video: None,
                    },
                }),
                // A slow consumer observes the missing time through the source timestamps.
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    }
}
impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.upgrade() {owner.capture.get().unwrap().remove(self.id);}
    }
}
