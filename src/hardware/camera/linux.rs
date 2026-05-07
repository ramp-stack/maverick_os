use image::RgbaImage;

pub struct OsCamera {
    running: bool,
}

impl OsCamera {
    pub fn new() -> Self {
        OsCamera { running: false }
    }

    pub fn start(&mut self) {
        self.running = true;
    }

    pub fn frame(&mut self) -> Option<RgbaImage> {
        if !self.running {
            return None;
        }
        None // replace with actual frame capture
    }

    pub fn stop(&mut self) {
        self.running = false;
    }
}