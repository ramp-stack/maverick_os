#![allow(non_snake_case)]

use std::{sync::Mutex, slice::from_raw_parts};
use image::{Rgba, RgbaImage};
use bayer::{BayerDepth, CFA, Demosaic, RasterMut, RasterDepth, run_demosaic};
use std::io::Cursor;

#[cfg(any(target_os = "ios", target_os = "macos"))]
use {
    dispatch2::DispatchQueue,
    objc2::{__framework_prelude::NSObject, rc::Retained, runtime::{NSObjectProtocol, ProtocolObject}, define_class, AllocAnyThread, DeclaredClass},
    objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString},
    objc2_core_media::CMSampleBuffer,
    objc2_av_foundation::*,
    objc2_core_video::*,
};

/// Bayer color filter array patterns
#[derive(Debug, Clone, Copy)]
pub enum BayerPattern { RGGB, BGGR, GRBG, GBRG }

impl BayerPattern {
    /// Convert to the bayer crate's CFA enum
    fn to_cfa(self) -> CFA {
        match self {
            BayerPattern::RGGB => CFA::RGGB,
            BayerPattern::BGGR => CFA::BGGR,
            BayerPattern::GRBG => CFA::GRBG,
            BayerPattern::GBRG => CFA::GBRG,
        }
    }
}

#[derive(Debug)]
pub struct ProcessorClass {
    pub last_raw_frame: Mutex<Option<RgbaImage>>,
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
define_class!(
    #[unsafe(super = NSObject)]
    #[ivars = ProcessorClass]
    #[derive(Debug)]
    struct Processor;

    unsafe impl NSObjectProtocol for Processor {}

    //! Point of intrest: This class handles video frame processing from the camera
    unsafe impl AVCaptureVideoDataOutputSampleBufferDelegate for Processor {
        /// Handle incoming video frames from the camera
        #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
        fn captureOutput_didOutputSampleBuffer_fromConnection(
            &self, _output: &AVCaptureOutput, sample_buffer: &CMSampleBuffer, _connection: &AVCaptureConnection,
        ) {
            let Some(pixel_buffer) = (unsafe { CMSampleBuffer::image_buffer(sample_buffer) }) else { 
                return 
            };
            
            let pixel_format = unsafe { CVPixelBufferGetPixelFormatType(&pixel_buffer) };
            
            let (height, width, bytes_per_row) = unsafe {
                (CVPixelBufferGetHeight(&pixel_buffer), CVPixelBufferGetWidth(&pixel_buffer), CVPixelBufferGetBytesPerRow(&pixel_buffer))
            };
            
            if unsafe { CVPixelBufferLockBaseAddress(&pixel_buffer, CVPixelBufferLockFlags(0)) } != 0 { 
                return 
            }
            
            let base_address = unsafe { CVPixelBufferGetBaseAddress(&pixel_buffer) } as *const u8;
            if base_address.is_null() {
                unsafe { CVPixelBufferUnlockBaseAddress(&pixel_buffer, CVPixelBufferLockFlags(0)) };
                return;
            }

            match pixel_format {
                kCVPixelFormatType_14Bayer_RGGB | kCVPixelFormatType_14Bayer_BGGR | 
                kCVPixelFormatType_14Bayer_GRBG | kCVPixelFormatType_14Bayer_GBRG => {
                    let bayer_pattern = match pixel_format {
                        kCVPixelFormatType_14Bayer_RGGB => BayerPattern::RGGB,
                        kCVPixelFormatType_14Bayer_BGGR => BayerPattern::BGGR,
                        kCVPixelFormatType_14Bayer_GRBG => BayerPattern::GRBG,
                        kCVPixelFormatType_14Bayer_GBRG => BayerPattern::GBRG,
                        _ => unreachable!(),
                    };
                    
                    if let Ok(rgba_image) = self.process_bayer_data(base_address, width, height, bytes_per_row, bayer_pattern) {
                        *self.ivars().last_raw_frame.lock().unwrap() = Some(rgba_image);
                    }
                }
                kCVPixelFormatType_32BGRA => {
                    let slice = unsafe { from_raw_parts(base_address, bytes_per_row * height) };
                    let mut rgba_data = Vec::with_capacity((width * height * 4) as usize);

                    for y in 0..height {
                        let row_start = y * bytes_per_row;
                        for x in 0..width {
                            let src_index = row_start + x * 4; 
                            
                            if src_index + 3 < slice.len() {
                                // Direct BGRA → RGBA conversion
                                let b = slice[src_index];
                                let g = slice[src_index + 1]; 
                                let r = slice[src_index + 2];
                                let a = slice[src_index + 3];
                                
                                rgba_data.extend_from_slice(&[r, g, b, a]);
                            }
                        }
                    }

                    // Create RGBA image directly
                    if let Some(rgba_image) = RgbaImage::from_raw(width as u32, height as u32, rgba_data) {
                        *self.ivars().last_raw_frame.lock().unwrap() = Some(rgba_image);
                    }
                }
                _ => {}
            }
            
            unsafe { CVPixelBufferUnlockBaseAddress(&pixel_buffer, CVPixelBufferLockFlags(0)) };
        }
    }

    unsafe impl AVCapturePhotoCaptureDelegate for Processor {
        /// Handle captured photos from the camera
        #[unsafe(method(captureOutput:didFinishProcessingPhoto:error:))]
        fn captureOutput_didFinishProcessingPhoto_error(
            &self, _output: &AVCapturePhotoOutput, photo: &objc2_av_foundation::AVCapturePhoto, error: Option<&objc2_foundation::NSError>,
        ) {
            if error.is_some() {
                return;
            }
            
            let Some(pixel_buffer) = (unsafe { photo.pixelBuffer() }) else { 
                return 
            };
            
            let pixel_format = unsafe { CVPixelBufferGetPixelFormatType(&pixel_buffer) };
            
            let bayer_pattern = match pixel_format {
                kCVPixelFormatType_14Bayer_RGGB => BayerPattern::RGGB,
                kCVPixelFormatType_14Bayer_BGGR => BayerPattern::BGGR,
                kCVPixelFormatType_14Bayer_GRBG => BayerPattern::GRBG,
                kCVPixelFormatType_14Bayer_GBRG => BayerPattern::GBRG,
                _ => return,
            };

            let (height, width, bytes_per_row) = unsafe {
                (CVPixelBufferGetHeight(&pixel_buffer), CVPixelBufferGetWidth(&pixel_buffer), CVPixelBufferGetBytesPerRow(&pixel_buffer))
            };

            if unsafe { CVPixelBufferLockBaseAddress(&pixel_buffer, CVPixelBufferLockFlags(1)) } != 0 { 
                return 
            }
            
            let base_address = unsafe { CVPixelBufferGetBaseAddress(&pixel_buffer) } as *const u8;
            if base_address.is_null() {
                unsafe { CVPixelBufferUnlockBaseAddress(&pixel_buffer, CVPixelBufferLockFlags(1)) };
                return;
            }

            if let Ok(rgba_image) = self.process_bayer_data(base_address, width, height, bytes_per_row, bayer_pattern) {
                *self.ivars().last_raw_frame.lock().unwrap() = Some(rgba_image);
            }
            
            unsafe { CVPixelBufferUnlockBaseAddress(&pixel_buffer, CVPixelBufferLockFlags(1)) };
        }
    }
);

#[cfg(any(target_os = "ios", target_os = "macos"))]
impl Processor {
    pub fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(ProcessorClass {
            last_raw_frame: Mutex::new(None),
        });
        unsafe { objc2::msg_send![super(this), init] }
    }

    /// Process raw Bayer sensor data into RGB
    fn process_bayer_data(&self, base_address: *const u8, width: usize, height: usize, bytes_per_row: usize, pattern: BayerPattern) -> Result<RgbaImage, String> {
        let mut bayer_data = vec![0u16; (width * height) as usize];
        
        for y in 0..height {
            let row_start = y * bytes_per_row;
            for x in 0..width {
                let byte_index = row_start + x * 2;
                let pixel_index = y * width + x;
                
                if byte_index + 1 < bytes_per_row * height && pixel_index < bayer_data.len() {
                    let raw_slice = unsafe { from_raw_parts(base_address.add(byte_index), 2) };
                    let pixel_14bit = u16::from_le_bytes([raw_slice[0], raw_slice[1]]) & 0x3FFF;
                    bayer_data[pixel_index] = pixel_14bit;
                }
            }
        }

        Self::demosaic_bayer(&bayer_data, width, height, pattern)
    }

    /// Convert Bayer pattern data to full color RGB image
    fn demosaic_bayer(bayer_data: &[u16], width: usize, height: usize, pattern: BayerPattern) -> Result<RgbaImage, String> {
        
        let mut bayer_bytes = Vec::with_capacity(bayer_data.len() * 2);
        for &pixel in bayer_data {
            let scaled_pixel = pixel << 2;
            bayer_bytes.extend_from_slice(&scaled_pixel.to_le_bytes());
        }
        
        let mut cursor = Cursor::new(bayer_bytes);
        let mut rgb_output = vec![0u8; width * height * 3];
        
        let mut output_raster = RasterMut::new(width, height, RasterDepth::Depth8, &mut rgb_output);

        run_demosaic(
            &mut cursor,
            BayerDepth::Depth16LE,
            pattern.to_cfa(),
            Demosaic::Linear,
            &mut output_raster
        ).map_err(|e| format!("Demosaicing process failed: {:?}", e))?;

        let mut rgba_data = Vec::with_capacity(width * height * 4);

        //! Point of intrest: Apply gamma correction and white balance
        let gamma = 1.0 / 2.2;
        let white_balance = [1.2, 1.0, 1.8]; 
        
        for chunk in rgb_output.chunks(3) {
            let r = ((chunk[0] as f32 * white_balance[0] / 255.0).powf(gamma) * 255.0).min(255.0) as u8;
            let g = ((chunk[1] as f32 * white_balance[1] / 255.0).powf(gamma) * 255.0).min(255.0) as u8;
            let b = ((chunk[2] as f32 * white_balance[2] / 255.0).powf(gamma) * 255.0).min(255.0) as u8;
            
            rgba_data.extend_from_slice(&[r, g, b, 255]);
        }

        RgbaImage::from_raw(width as u32, height as u32, rgba_data)
            .ok_or_else(|| "Couldn't create final RGBA image from processed data".to_string())
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

            if !self.session.canAddInput(&input) { return Err("Camera input isn't compatible".into()); }
            self.session.addInput(&input);

            let video_output = AVCaptureVideoDataOutput::new();
            let pixel_format_key = &*(kCVPixelBufferPixelFormatTypeKey as *const _ as *const NSString);
            
            let video_settings = NSDictionary::from_slices(
                &[pixel_format_key], &[NSNumber::new_u32(kCVPixelFormatType_32BGRA).as_ref()],
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

    /// Capture a high quality raw photo
    pub fn capture_raw_photo(&self) -> Result<(), String> {
        let Some(photo_output) = &self.photo_output else {
            return Err("Photo capture isn't set up".to_string());
        };

        unsafe {
            let settings = AVCapturePhotoSettings::photoSettings();
            photo_output.capturePhotoWithSettings_delegate(&settings, ProtocolObject::from_ref(&*self.processor));
        }
        
        Ok(())
    }

    /// Get the most recent processed raw image
    pub fn get_latest_raw_frame(&self) -> Option<RgbaImage> {
        self.processor.ivars().last_raw_frame.lock().unwrap().clone()
    }
}