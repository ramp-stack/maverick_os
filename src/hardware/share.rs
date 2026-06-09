#[cfg(any(target_os = "ios", target_os = "macos"))]
mod apple;
#[cfg(any(target_os = "ios", target_os = "macos"))]
use apple::OsShare;

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
use android::OsShare;

use image::RgbaImage;

#[derive(Clone)]
pub struct Share(
    OsShare
);

impl Share {
    pub(crate) fn new() -> Self {
        Self(
            OsShare::new()
        )
    }

    pub fn share(&self, text: &str) {
        self.0.share(text);
    }

    pub fn share_image(&self, rgba_image: RgbaImage) {
        self.0.share_image(rgba_image);
    }
}