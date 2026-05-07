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

use image::RgbaImage;
use std::sync::{Arc, Mutex};

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

        #[cfg(target_os = "linux")]
        OsPhotoPicker::open(move |bytes: Vec<u8>| {
            let decoded = image::load_from_memory(&bytes)
                .ok()
                .map(|img| img.to_rgba8());
            *photo_ref.lock().unwrap() = decoded;
        });

        #[cfg(not(target_os = "linux"))]
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