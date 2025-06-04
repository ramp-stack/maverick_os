mod cache;
mod camera;
mod share;

mod clipboard;

#[cfg(target_os = "android")]
use jni;

pub use cache::Cache;
pub use camera::{Camera, CameraError};
use crate::hardware::clipboard::Clipboard;

/// Hardware context contains interfaces to various hardware.
/// All interfaces should be clonable or internally synchronized and safe to call from multiple places.
#[derive(Clone)]
pub struct Context {
    pub cache: Cache,
    pub clipboard: Clipboard,
}

impl Context {
    pub(crate) fn new() -> Self {
        Self {
            cache: Cache::new(),
            clipboard: Clipboard::new(),
        }
    }

    pub fn create_camera(&self) -> Camera {
        Camera::new()
    }

    pub fn get() -> String {
        Clipboard::get()
    }
    pub fn set(text: String) {
        Clipboard::set(text);
    }

    #[cfg(target_os = "android")]
    pub fn initialize(env: &mut JNIEnv, context: JObject) -> Result<(), jni::errors::Error> {
        Clipboard::initialize(env, context)
    }
}