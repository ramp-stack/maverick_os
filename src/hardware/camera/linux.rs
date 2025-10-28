use super::{CameraError, CameraSettings};
use image::RgbaImage;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct OsCamera;

impl OsCamera {
    pub fn new_standard() -> Result<Self, CameraError> {
        Err(CameraError::InitializationFailed)
    }

    pub fn new_custom() -> Result<Self, CameraError> {
        Err(CameraError::InitializationFailed)
    }

    pub fn frame(&self) -> Result<RgbaImage, CameraError> {
        Err(CameraError::FailedToGetFrame)
    }

    pub fn settings(&mut self) -> Option<Arc<Mutex<CameraSettings>>> {
        None
    }
}