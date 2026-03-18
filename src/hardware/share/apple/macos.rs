use image::RgbaImage;

#[derive(Clone)]
pub struct OsShare;

impl OsShare {
    pub fn new() -> Self {
        Self
    }

    pub fn share(&self, _text: &str) {
        unimplemented!() 
    }

    pub fn share_image(&self, _rgba_image: RgbaImage) {
        unimplemented!() 
    }
}
