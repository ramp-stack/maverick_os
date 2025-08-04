#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

use std::{sync::Mutex, slice::from_raw_parts};
use image::RgbaImage;

use photon_rs::{
    PhotonImage,
    effects::{adjust_brightness, adjust_contrast},
};

#[cfg(any(target_os = "ios", target_os = "macos"))]
use {
    dispatch2::DispatchQueue,
    objc2::{__framework_prelude::NSObject, rc::Retained, runtime::{NSObjectProtocol, ProtocolObject}, define_class, AllocAnyThread, DeclaredClass},
    objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString},
    objc2_core_media::CMSampleBuffer,
    objc2_av_foundation::*,
    objc2_core_video::*,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BayerPattern { RGGB, BGGR, GRBG, GBRG }

impl BayerPattern {
    fn pixel_type(&self, x: usize, y: usize) -> PixelType {
        let even_row = y % 2 == 0;
        let even_col = x % 2 == 0;
        
        match self {
            BayerPattern::RGGB => match (even_row, even_col) {
                (true, true) => PixelType::Red,
                (true, false) => PixelType::Green,
                (false, true) => PixelType::Green,
                (false, false) => PixelType::Blue,
            },
            BayerPattern::BGGR => match (even_row, even_col) {
                (true, true) => PixelType::Blue,
                (true, false) => PixelType::Green,
                (false, true) => PixelType::Green,
                (false, false) => PixelType::Red,
            },
            BayerPattern::GRBG => match (even_row, even_col) {
                (true, true) => PixelType::Green,
                (true, false) => PixelType::Red,
                (false, true) => PixelType::Blue,
                (false, false) => PixelType::Green,
            },
            BayerPattern::GBRG => match (even_row, even_col) {
                (true, true) => PixelType::Green,
                (true, false) => PixelType::Blue,
                (false, true) => PixelType::Red,
                (false, false) => PixelType::Green,
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum PixelType { Red, Green, Blue }

#[derive(Debug, Clone)]
pub struct ImageSettings {
    pub brightness: i16, 
    pub contrast: f32,
    pub saturation: f32,
    pub gamma: f32,
    pub white_balance_r: f32,
    pub white_balance_g: f32,
    pub white_balance_b: f32,
    pub exposure: f32,
    pub temperature: f32,
}

impl Default for ImageSettings {
    fn default() -> Self {
        Self {
            brightness: 0,
            contrast: 0.0,
            saturation: 0.0,
            gamma: 2.2,
            white_balance_r: 1.0,
            white_balance_g: 1.0,
            white_balance_b: 1.0,
            exposure: 0.0,
            temperature: 6500.0,
        }
    }
}

impl ImageSettings {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clamp_values(&mut self) {
        self.brightness = self.brightness.clamp(-100, 100);
        self.contrast = self.contrast.clamp(-1.0, 1.0);
        self.saturation = self.saturation.clamp(-1.0, 1.0);
        self.gamma = self.gamma.clamp(0.1, 3.0);
        self.white_balance_r = self.white_balance_r.clamp(0.5, 2.0);
        self.white_balance_g = self.white_balance_g.clamp(0.5, 2.0);
        self.white_balance_b = self.white_balance_b.clamp(0.5, 2.0);
        self.exposure = self.exposure.clamp(-2.0, 2.0);
        self.temperature = self.temperature.clamp(2000.0, 10000.0);
    }

    fn temperature_to_rgb_multipliers(&self) -> [f32; 3] {
        let temp = self.temperature;
        if temp < 6600.0 {
            let r = 1.0;
            let g = (0.39008157 * (temp / 100.0).ln() - 0.63184144).clamp(0.0, 1.0);
            let b = if temp < 2000.0 { 
                0.0 
            } else { 
                (0.54320678 * ((temp / 100.0) - 10.0).ln() - 1.19625408).clamp(0.0, 1.0) 
            };
            [r, g, b]
        } else {
            let r = (1.29293618 * ((temp / 100.0) - 60.0).powf(-0.1332047)).clamp(0.0, 1.0);
            let g = (1.12989086 * ((temp / 100.0) - 60.0).powf(-0.0755148)).clamp(0.0, 1.0);
            let b = 1.0;
            [r, g, b]
        }
    }
}

#[derive(Debug)]
pub struct ProcessorClass {
    pub last_raw_frame: Mutex<Option<RgbaImage>>,
    pub settings: Mutex<ImageSettings>,
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
define_class!(
    #[unsafe(super = NSObject)]
    #[ivars = ProcessorClass]
    #[derive(Debug)]
    struct Processor;

    unsafe impl NSObjectProtocol for Processor {}

    unsafe impl AVCaptureVideoDataOutputSampleBufferDelegate for Processor {
        #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
        fn captureOutput_didOutputSampleBuffer_fromConnection(
            &self, _output: &AVCaptureOutput, sample_buffer: &CMSampleBuffer, _connection: &AVCaptureConnection,
        ) {
            if let Some(rgba_image) = self.process_sample_buffer(sample_buffer) {
                *self.ivars().last_raw_frame.lock().unwrap() = Some(rgba_image);
            }
        }
    }

    unsafe impl AVCapturePhotoCaptureDelegate for Processor {
        #[unsafe(method(captureOutput:didFinishProcessingPhoto:error:))]
        fn captureOutput_didFinishProcessingPhoto_error(
            &self, _output: &AVCapturePhotoOutput, photo: &objc2_av_foundation::AVCapturePhoto, error: Option<&objc2_foundation::NSError>,
        ) {
            if error.is_some() { return; }
            
            if let Some(pixel_buffer) = unsafe { photo.pixelBuffer() } {
                if let Some(rgba_image) = self.process_pixel_buffer(&pixel_buffer) {
                    *self.ivars().last_raw_frame.lock().unwrap() = Some(rgba_image);
                }
            }
        }
    }
);

#[cfg(any(target_os = "ios", target_os = "macos"))]
impl Processor {
    pub fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(ProcessorClass {
            last_raw_frame: Mutex::new(None),
            settings: Mutex::new(ImageSettings::default()),
        });
        unsafe { objc2::msg_send![super(this), init] }
    }

    fn process_sample_buffer(&self, sample_buffer: &CMSampleBuffer) -> Option<RgbaImage> {
        let pixel_buffer = unsafe { CMSampleBuffer::image_buffer(sample_buffer) }?;
        self.process_pixel_buffer(&pixel_buffer)
    }

    fn process_pixel_buffer(&self, pixel_buffer: &CVPixelBuffer) -> Option<RgbaImage> {
        let pixel_format = unsafe { CVPixelBufferGetPixelFormatType(pixel_buffer) };
        let (height, width, bytes_per_row) = unsafe {
            (CVPixelBufferGetHeight(pixel_buffer), CVPixelBufferGetWidth(pixel_buffer), CVPixelBufferGetBytesPerRow(pixel_buffer))
        };
        
        if unsafe { CVPixelBufferLockBaseAddress(pixel_buffer, CVPixelBufferLockFlags(0)) } != 0 {
            return None;
        }
        
        let base_address = unsafe { CVPixelBufferGetBaseAddress(pixel_buffer) } as *const u8;
        if base_address.is_null() {
            unsafe { CVPixelBufferUnlockBaseAddress(pixel_buffer, CVPixelBufferLockFlags(0)) };
            return None;
        }

        let result = match pixel_format {
            kCVPixelFormatType_14Bayer_RGGB => self.process_bayer_data(base_address, width, height, bytes_per_row, BayerPattern::RGGB),
            kCVPixelFormatType_14Bayer_BGGR => self.process_bayer_data(base_address, width, height, bytes_per_row, BayerPattern::BGGR),
            kCVPixelFormatType_14Bayer_GRBG => self.process_bayer_data(base_address, width, height, bytes_per_row, BayerPattern::GRBG),
            kCVPixelFormatType_14Bayer_GBRG => self.process_bayer_data(base_address, width, height, bytes_per_row, BayerPattern::GBRG),
            kCVPixelFormatType_32BGRA => self.process_bgra_data(base_address, width, height, bytes_per_row),
            _ => None,
        };
        
        unsafe { CVPixelBufferUnlockBaseAddress(pixel_buffer, CVPixelBufferLockFlags(0)) };
        result.map(|img| self.apply_image_settings(img))
    }

    fn process_bayer_data(&self, base_address: *const u8, width: usize, height: usize, bytes_per_row: usize, pattern: BayerPattern) -> Option<RgbaImage> {
        let mut bayer_16bit = vec![0u16; width * height];
        
        for y in 0..height {
            let row_start = y * bytes_per_row;
            for x in 0..width {
                let byte_index = row_start + x * 2;
                let pixel_index = y * width + x;
                
                if byte_index + 1 < bytes_per_row * height && pixel_index < bayer_16bit.len() {
                    let raw_slice = unsafe { from_raw_parts(base_address.add(byte_index), 2) };
                    let pixel_14bit = u16::from_le_bytes([raw_slice[0], raw_slice[1]]) & 0x3FFF;
                    bayer_16bit[pixel_index] = pixel_14bit << 2;
                }
            }
        }

        let rgba_data = self.demosaic_bilinear(&bayer_16bit, width, height, pattern);
        RgbaImage::from_raw(width as u32, height as u32, rgba_data)
    }

    fn process_bgra_data(&self, base_address: *const u8, width: usize, height: usize, bytes_per_row: usize) -> Option<RgbaImage> {
        let slice = unsafe { from_raw_parts(base_address, bytes_per_row * height) };
        let mut rgba_data = Vec::with_capacity(width * height * 4);

        for y in 0..height {
            let row_start = y * bytes_per_row;
            for x in 0..width {
                let src_index = row_start + x * 4;
                
                if src_index + 3 < slice.len() {
                    let b = slice[src_index];
                    let g = slice[src_index + 1];
                    let r = slice[src_index + 2];
                    let a = slice[src_index + 3];
                    
                    rgba_data.extend_from_slice(&[r, g, b, a]);
                }
            }
        }

        RgbaImage::from_raw(width as u32, height as u32, rgba_data)
    }

    fn apply_image_settings(&self, rgba_image: RgbaImage) -> RgbaImage {
        let settings = self.ivars().settings.lock().unwrap().clone();
        
        let width = rgba_image.width();
        let height = rgba_image.height();
        let mut photon_img = PhotonImage::new(rgba_image.into_raw(), width, height);
        
        if settings.temperature != 6500.0 {
            let temp_rgb = settings.temperature_to_rgb_multipliers();
            self.apply_white_balance(&mut photon_img, temp_rgb);
        }
        
        if settings.white_balance_r != 1.0 || settings.white_balance_g != 1.0 || settings.white_balance_b != 1.0 {
            self.apply_white_balance(&mut photon_img, [settings.white_balance_r, settings.white_balance_g, settings.white_balance_b]);
        }
        
        if settings.exposure != 0.0 { self.apply_exposure(&mut photon_img, settings.exposure); }
        if settings.brightness != 0 { adjust_brightness(&mut photon_img, settings.brightness); }
        if settings.contrast != 0.0 { adjust_contrast(&mut photon_img, settings.contrast); }
        if settings.saturation != 0.0 { self.apply_saturation(&mut photon_img, settings.saturation); }
        if settings.gamma != 2.2 { self.apply_gamma(&mut photon_img, settings.gamma); }
        
        RgbaImage::from_raw(photon_img.get_width(), photon_img.get_height(), photon_img.get_raw_pixels())
            .unwrap_or_else(|| {
                RgbaImage::new(width, height)
            })
    }

    fn apply_white_balance(&self, photon_img: &mut PhotonImage, rgb_multipliers: [f32; 3]) {
        let raw_pixels = photon_img.get_raw_pixels();
        let mut new_pixels = raw_pixels.to_vec();
        
        for chunk in new_pixels.chunks_exact_mut(4) {
            chunk[0] = (chunk[0] as f32 * rgb_multipliers[0]).clamp(0.0, 255.0) as u8;
            chunk[1] = (chunk[1] as f32 * rgb_multipliers[1]).clamp(0.0, 255.0) as u8;
            chunk[2] = (chunk[2] as f32 * rgb_multipliers[2]).clamp(0.0, 255.0) as u8;
        }
        
        *photon_img = PhotonImage::new(new_pixels, photon_img.get_width(), photon_img.get_height());
    }

    // Moved from floating functions - Demosaicing functions
    fn demosaic_bilinear(&self, bayer_data: &[u16], width: usize, height: usize, pattern: BayerPattern) -> Vec<u8> {
        let mut rgb_data = vec![0u8; width * height * 4]; // RGBA
        
        for y in 1..height-1 {
            for x in 1..width-1 {
                let idx = y * width + x;
                let rgba_idx = idx * 4;
                
                let pixel_val = (bayer_data[idx] >> 8) as u8;
                
                match pattern.pixel_type(x, y) {
                    PixelType::Red => {
                        rgb_data[rgba_idx] = pixel_val;
                        rgb_data[rgba_idx + 1] = self.interpolate_green(bayer_data, x, y, width);
                        rgb_data[rgba_idx + 2] = self.interpolate_blue(bayer_data, x, y, width);
                        rgb_data[rgba_idx + 3] = 255;
                    },
                    PixelType::Green => {
                        rgb_data[rgba_idx] = self.interpolate_red(bayer_data, x, y, width);
                        rgb_data[rgba_idx + 1] = pixel_val;
                        rgb_data[rgba_idx + 2] = self.interpolate_blue(bayer_data, x, y, width);
                        rgb_data[rgba_idx + 3] = 255;
                    },
                    PixelType::Blue => {
                        rgb_data[rgba_idx] = self.interpolate_red(bayer_data, x, y, width);
                        rgb_data[rgba_idx + 1] = self.interpolate_green(bayer_data, x, y, width);
                        rgb_data[rgba_idx + 2] = pixel_val;
                        rgb_data[rgba_idx + 3] = 255;
                    },
                }
            }
        }
        
        rgb_data
    }

    fn interpolate_green(&self, data: &[u16], x: usize, y: usize, width: usize) -> u8 {
        let neighbors = [
            data.get((y-1) * width + x).unwrap_or(&0),
            data.get(y * width + x-1).unwrap_or(&0),
            data.get(y * width + x+1).unwrap_or(&0),
            data.get((y+1) * width + x).unwrap_or(&0),
        ];
        let avg = neighbors.iter().map(|&v| *v as u32).sum::<u32>() / 4;
        (avg >> 8) as u8
    }

    fn interpolate_red(&self, data: &[u16], x: usize, y: usize, width: usize) -> u8 {
        let neighbors = [
            data.get((y-1) * width + x-1).unwrap_or(&0),
            data.get((y-1) * width + x+1).unwrap_or(&0),
            data.get((y+1) * width + x-1).unwrap_or(&0),
            data.get((y+1) * width + x+1).unwrap_or(&0),
        ];
        let avg = neighbors.iter().map(|&v| *v as u32).sum::<u32>() / 4;
        (avg >> 8) as u8
    }

    fn interpolate_blue(&self, data: &[u16], x: usize, y: usize, width: usize) -> u8 {
        let neighbors = [
            data.get((y-1) * width + x-1).unwrap_or(&0),
            data.get((y-1) * width + x+1).unwrap_or(&0),
            data.get((y+1) * width + x-1).unwrap_or(&0),
            data.get((y+1) * width + x+1).unwrap_or(&0),
        ];
        let avg = neighbors.iter().map(|&v| *v as u32).sum::<u32>() / 4;
        (avg >> 8) as u8
    }

    fn apply_saturation(&self, photon_img: &mut PhotonImage, saturation: f32) {
        let raw_pixels = photon_img.get_raw_pixels();
        let mut new_pixels = raw_pixels.to_vec();
        
        for chunk in new_pixels.chunks_exact_mut(4) {
            let r = chunk[0] as f32 / 255.0;
            let g = chunk[1] as f32 / 255.0;
            let b = chunk[2] as f32 / 255.0;
            
            let max = r.max(g).max(b);
            let min = r.min(g).min(b);
            let l = (max + min) / 2.0;
            
            if max == min {
                continue;
            }
            
            let d = max - min;
            let s = if l > 0.5 {
                d / (2.0 - max - min)
            } else {
                d / (max + min)
            };
            
            let new_s = (s + saturation).clamp(0.0, 1.0);
            
            let c = (1.0 - (2.0 * l - 1.0).abs()) * new_s;
            let x = c * (1.0 - ((((r - g).abs() + (g - b).abs() + (b - r).abs()) / d * 60.0) % 2.0 - 1.0).abs());
            let m = l - c / 2.0;
            
            chunk[0] = ((r * c + m) * 255.0).clamp(0.0, 255.0) as u8;
            chunk[1] = ((g * c + m) * 255.0).clamp(0.0, 255.0) as u8;
            chunk[2] = ((b * c + m) * 255.0).clamp(0.0, 255.0) as u8;
        }
        
        *photon_img = PhotonImage::new(new_pixels, photon_img.get_width(), photon_img.get_height());
    }

    fn apply_gamma(&self, photon_img: &mut PhotonImage, gamma: f32) {
        let raw_pixels = photon_img.get_raw_pixels();
        let mut new_pixels = raw_pixels.to_vec();
        
        let inv_gamma = 1.0 / gamma;
        
        for chunk in new_pixels.chunks_exact_mut(4) {
            chunk[0] = (255.0 * (chunk[0] as f32 / 255.0).powf(inv_gamma)).clamp(0.0, 255.0) as u8;
            chunk[1] = (255.0 * (chunk[1] as f32 / 255.0).powf(inv_gamma)).clamp(0.0, 255.0) as u8;
            chunk[2] = (255.0 * (chunk[2] as f32 / 255.0).powf(inv_gamma)).clamp(0.0, 255.0) as u8;
        }
        
        *photon_img = PhotonImage::new(new_pixels, photon_img.get_width(), photon_img.get_height());
    }

    fn apply_exposure(&self, photon_img: &mut PhotonImage, exposure: f32) {
        let raw_pixels = photon_img.get_raw_pixels();
        let mut new_pixels = raw_pixels.to_vec();
        
        let exposure_multiplier = 2.0_f32.powf(exposure);
        
        for chunk in new_pixels.chunks_exact_mut(4) {
            chunk[0] = (chunk[0] as f32 * exposure_multiplier).clamp(0.0, 255.0) as u8;
            chunk[1] = (chunk[1] as f32 * exposure_multiplier).clamp(0.0, 255.0) as u8;
            chunk[2] = (chunk[2] as f32 * exposure_multiplier).clamp(0.0, 255.0) as u8;
        }
        
        *photon_img = PhotonImage::new(new_pixels, photon_img.get_width(), photon_img.get_height());
    }

    pub fn update_settings<F>(&self, update_fn: F) 
    where F: FnOnce(&mut ImageSettings),
    {
        let mut settings = self.ivars().settings.lock().unwrap();
        update_fn(&mut *settings);
        settings.clamp_values();
    }

    pub fn get_settings(&self) -> ImageSettings {
        self.ivars().settings.lock().unwrap().clone()
    }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
#[derive(Debug, Clone)]
pub struct AppleCustomCamera {
    pub session: Retained<AVCaptureSession>,
    processor: Retained<Processor>,
    photo_output: Option<Retained<AVCapturePhotoOutput>>,
    video_output: Option<Retained<AVCaptureVideoDataOutput>>,
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
impl AppleCustomCamera {
    pub fn new() -> Self {
        Self {
            session: unsafe { AVCaptureSession::new() },
            processor: Processor::new(),
            photo_output: None,
            video_output: None,
        }
    }

    pub fn open_camera(&mut self) -> Result<(), String> {
        unsafe {
            let device_types = NSArray::from_slice(&[AVCaptureDeviceTypeBuiltInWideAngleCamera]);
            let discovery_session = AVCaptureDeviceDiscoverySession::discoverySessionWithDeviceTypes_mediaType_position(
                &device_types, AVMediaTypeVideo, AVCaptureDevicePosition::Back,
            );
            let device = discovery_session.devices().into_iter().next().ok_or("No camera found on this device")?;
            
            let input = AVCaptureDeviceInput::deviceInputWithDevice_error(&device)
                .map_err(|_| "Couldn't connect to camera")?;

            self.session.beginConfiguration();
            self.session.setSessionPreset(AVCaptureSessionPresetPhoto);

            if !self.session.canAddInput(&input) { 
                return Err("Camera input isn't compatible".into()); 
            }
            self.session.addInput(&input);

            let video_output = AVCaptureVideoDataOutput::new();
            let pixel_format_key = &*(kCVPixelBufferPixelFormatTypeKey as *const _ as *const NSString);
            let video_settings = NSDictionary::from_slices(
                &[pixel_format_key], 
                &[NSNumber::new_u32(kCVPixelFormatType_32BGRA).as_ref()],
            );
            video_output.setVideoSettings(Some(&video_settings));
            video_output.setAlwaysDiscardsLateVideoFrames(true);
            
            let queue = DispatchQueue::new("CameraQueue", None);
            video_output.setSampleBufferDelegate_queue(Some(ProtocolObject::from_ref(&*self.processor)), Some(&queue));

            if self.session.canAddOutput(&video_output) {
                self.session.addOutput(&video_output);
                self.video_output = Some(video_output);
            }

            let photo_output = AVCapturePhotoOutput::new();
            if self.session.canAddOutput(&photo_output) {
                self.session.addOutput(&photo_output);
                self.photo_output = Some(photo_output);
            }

            self.session.commitConfiguration();
            self.session.startRunning();
            
            Ok(())
        }
    }

    pub fn stop_camera(&self) { 
        unsafe { 
            self.session.stopRunning(); 
            *self.processor.ivars().last_raw_frame.lock().unwrap() = None;
        }
    }

    pub fn get_latest_raw_frame(&self) -> Option<RgbaImage> {
        self.processor.ivars().last_raw_frame.lock().unwrap().clone()
    }

    pub fn update_settings<F>(&self, update_fn: F) 
    where F: FnOnce(&mut ImageSettings),
    {
        self.processor.update_settings(update_fn);
    }

    pub fn get_settings(&self) -> ImageSettings {
        self.processor.get_settings()
    }
}

// // Update individual settings
// camera.update_settings(|settings| {
//     settings.brightness = 25;           // Range: -100 to 100
//     settings.contrast = 0.3;            // Range: -1.0 to 1.0
//     settings.saturation = 0.2;          // Range: -1.0 to 1.0
//     settings.gamma = 2.4;               // Range: 0.1 to 3.0
//     settings.exposure = 0.5;            // Range: -2.0 to 2.0
//     settings.temperature = 5500.0;      // Range: 2000.0 to 10000.0 (Kelvin)
// });

// // Or update multiple settings at once
// camera.update_settings(|settings| {
//     settings.white_balance_r = 1.1;     // Range: 0.5 to 2.0
//     settings.white_balance_g = 1.0;     // Range: 0.5 to 2.0
//     settings.white_balance_b = 0.9;     // Range: 0.5 to 2.0
// });