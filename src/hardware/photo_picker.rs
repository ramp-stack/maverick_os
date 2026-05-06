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

use std::sync::{Arc, Mutex};
use image::RgbaImage;

pub struct PhotoPicker {
    pub photo: Arc<Mutex<Option<RgbaImage>>>,
}

impl PhotoPicker {
    pub fn new() -> Self {
        Self {
            photo: Arc::new(Mutex::new(None)),
        }
    }

    pub fn open(&self) {
        let photo_ref = self.photo.clone();
        OsPhotoPicker::open(move |rgba| {
            *photo_ref.lock().unwrap() = rgba;
        });
    }

    pub(crate) fn tick(&mut self) -> Option<RgbaImage> {
        self.photo.lock().unwrap().take()
    }
}

impl Default for PhotoPicker {
    fn default() -> Self {
        Self::new()
    }
}