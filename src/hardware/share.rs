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
    #[cfg(any(target_os = "ios", target_os = "macos", target_os = "android"))]
    OsShare
);

impl Share {
    pub(crate) fn new() -> Self {
        Self(
            #[cfg(any(target_os = "ios", target_os = "macos", target_os = "android"))]
            OsShare::new()
        )
    }

    pub fn share(&self, text: &str) {
        #[cfg(any(target_os = "ios", target_os = "macos", target_os = "android"))]
        self.0.share(text);
        
        #[cfg(not(any(target_os = "ios", target_os = "macos", target_os = "android")))]
        panic!("Share not supported for this platform");
    }

    pub fn share_image(&self, rgba_image: RgbaImage) {
        #[cfg(any(target_os = "ios", target_os = "macos", target_os = "android"))]
        self.0.share_image(rgba_image);
        
        #[cfg(not(any(target_os = "ios", target_os = "macos", target_os = "android")))]
        panic!("Share image not supported for this platform");
    }
}