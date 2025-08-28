use nokhwa::utils::{CameraIndex, RequestedFormat, RequestedFormatType};
use nokhwa::pixel_format::RgbFormat;
use nokhwa::{Camera, native_api_backend};

pub struct WindowsLinuxCamera {
    camera: Camera,
}

impl WindowsLinuxCamera {
    pub fn new(index: usize) -> Self {
        let index = CameraIndex::Index(index.try_into().unwrap());
        let requested = RequestedFormat::new::<RgbFormat>(
            RequestedFormatType::AbsoluteHighestFrameRate,
        );

        let _backend = native_api_backend().unwrap();
        let camera = Camera::new(index, requested).unwrap();

        Self { camera }
    }

    pub fn start(&mut self) {
        self.camera.open_stream().unwrap();
    }

    pub fn capture(&mut self) {
        let frame = self.camera.frame().unwrap();
        println!("[Captured]<-->[frame]: {} bytes", frame.buffer().len());

        let decoded = frame.decode_image::<RgbFormat>().unwrap();
        println!("[Decoded]<-->[frame]: {} pixels", decoded.len());
    }
}

fn main() {
    let mut cam = WindowsLinuxCamera::new(0);
    cam.start();

    loop {
        cam.capture();
    }
}
