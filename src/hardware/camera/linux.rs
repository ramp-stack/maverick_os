use image::RgbaImage;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct OsCamera;

impl OsCamera {
    pub fn new() -> Self {
        todo!()
    }

    pub fn frame(&self) -> Option<RgbaImage> {
        todo!()
    }

    pub fn start(&self) {todo!()}
    pub fn stop(&self) {todo!()}
}
