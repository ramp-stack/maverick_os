#[cfg(target_os = "ios")]
mod ios;
#[cfg(target_os = "ios")]
use ios::OsPhotoPicker;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos::OsPhotoPicker;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux::OsPhotoPicker;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows::OsPhotoPicker;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use image::RgbaImage;

/// The most a picked video can be, once it's been made smaller to send.
/// Longer videos are cut off there.
pub const MAX_VIDEO_BYTES: usize = 8 * 1024 * 1024;

/// A photo or a video picked from the library.
#[derive(Clone, PartialEq)]
pub enum Media {
    Photo(RgbaImage),
    /// An MP4 (made smaller to send: at most 960x540 and
    /// [`MAX_VIDEO_BYTES`]), a still from its start, and how long it is.
    Video { mp4: Vec<u8>, poster: RgbaImage, duration: Duration },
}

impl std::fmt::Debug for Media {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Media::Photo(photo) => write!(f, "Photo({}x{})", photo.width(), photo.height()),
            Media::Video { mp4, poster, duration } => write!(f, "Video({} bytes, {duration:?}, {}x{} poster)", mp4.len(), poster.width(), poster.height()),
        }
    }
}

#[derive(Clone)]
pub struct PhotoPicker {
    pub photo: Arc<Mutex<Option<RgbaImage>>>,
    /// Picked with `open_media`, oldest first.
    media: Arc<Mutex<VecDeque<Media>>>,
}

impl PhotoPicker {
    pub fn new() -> Self {
        Self {
            photo: Arc::new(Mutex::new(None)),
            media: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    pub fn open(&self) {
        let photo_ref = self.photo.clone();
        OsPhotoPicker::open(move |rgba| {
            *photo_ref.lock().unwrap() = rgba;
        });
    }

    /// Opens the library to pick a photo or a video.
    pub fn open_media(&self) {
        let media_ref = self.media.clone();
        OsPhotoPicker::open_media(move |media| {
            // Kept, not replaced: the library can be opened again while
            // something picked is still loading.
            media_ref.lock().unwrap().extend(media);
        });
    }

    pub(crate) fn tick(&mut self) -> Option<RgbaImage> {
        self.photo.lock().unwrap().take()
    }

    pub(crate) fn tick_media(&mut self) -> Option<Media> {
        self.media.lock().unwrap().pop_front()
    }
}

impl Default for PhotoPicker {
    fn default() -> Self {
        Self::new()
    }
}
