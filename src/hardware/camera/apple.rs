#![allow(non_snake_case)]

use crate::hardware::{CameraSettings, CameraError};
use image::RgbaImage;
use std::sync::{Arc, Mutex};

mod custom;
mod standard;

use custom::CustomCamera;
use standard::{StandardProcessor, StandardOsCamera};

#[derive(Clone, Debug)]
pub enum OsCamera {
    Standard(StandardOsCamera),
    Custom(CustomCamera)
}

impl OsCamera {
    pub fn new_standard() -> Result<Self, CameraError> {
        StandardOsCamera::new()
            .map(OsCamera::Standard)
            .map_err(|_| CameraError::FailedToGetFrame)
    }

    pub fn new_custom() -> Result<Self, CameraError> {
        Ok(OsCamera::Custom(CustomCamera::new()))
    }

    pub fn frame(&self) -> Result<RgbaImage, CameraError> {
        match self {
            OsCamera::Standard(c) => c.frame().ok_or(CameraError::FailedToGetFrame),
            OsCamera::Custom(c) => c.frame().ok_or(CameraError::FailedToGetFrame)
        }
    }

    pub fn toggle_flashlight(&self) {
        // let _ = match self {
        //     OsCamera::Standard(_) => {},
        //     OsCamera::Custom(c) => c.toggle_flashlight()
        // };
    }

    pub fn settings(&self) -> Option<Arc<Mutex<CameraSettings>>> {
        match self {
            OsCamera::Standard(_) => None,
            OsCamera::Custom(c) => Some(c.settings())
        }
    }
}