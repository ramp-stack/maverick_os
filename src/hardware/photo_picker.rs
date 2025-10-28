use std::sync::mpsc::Sender;

#[cfg(any(target_os = "ios", target_os = "macos"))]
mod apple;
#[cfg(any(target_os = "ios", target_os = "macos"))]
use apple::OsPhotoPicker;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux::OsPhotoPicker;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows::OsPhotoPicker;

#[derive(Clone)]
pub struct PhotoPicker(
    #[cfg(any(target_os = "ios", target_os = "macos", target_os = "linux", target_os = "windows"))]
    OsPhotoPicker
);

impl PhotoPicker {
    pub fn open(sender: Sender<(Vec<u8>, ImageOrientation)>) {
        #[cfg(any(target_os = "ios", target_os = "macos", target_os = "linux", target_os = "windows"))]
        OsPhotoPicker::open(sender);
        
        #[cfg(not(any(target_os = "ios", target_os = "macos", target_os = "linux", target_os = "windows")))]
        panic!("not supported os");
    }
}

#[derive(Debug)]
pub enum ImageOrientation {
    Up,
    Down,
    Left,
    Right,
    UpMirrored,
    DownMirrored,
    LeftMirrored,
    RightMirrored,
}

impl ImageOrientation {
    pub fn from_ios_value(orientation: i64) -> Self {
        match orientation {
            0 => ImageOrientation::Up,
            1 => ImageOrientation::Down,
            2 => ImageOrientation::Left,
            3 => ImageOrientation::Right,
            4 => ImageOrientation::UpMirrored,
            5 => ImageOrientation::DownMirrored,
            6 => ImageOrientation::LeftMirrored,
            7 => ImageOrientation::RightMirrored,
            _ => ImageOrientation::Up,
        }
    }

    pub fn apply_to(&self, image: image::DynamicImage) -> image::DynamicImage {
        match self {
            ImageOrientation::Up => image,
            ImageOrientation::Down => image.rotate180(),
            ImageOrientation::Left => image.rotate270(),
            ImageOrientation::Right => image.rotate90(),
            ImageOrientation::UpMirrored => image.fliph(),
            ImageOrientation::DownMirrored => image.fliph().rotate180(),
            ImageOrientation::LeftMirrored => image.fliph().rotate90(),
            ImageOrientation::RightMirrored => image.fliph().rotate270(),
        }
    }
}