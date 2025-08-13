#![allow(non_snake_case)]

use std::{sync::Mutex, slice::from_raw_parts};
use image::{Rgba, RgbaImage};

#[cfg(any(target_os = "ios", target_os = "macos"))]
use {
    dispatch2::DispatchQueue,
    objc2::{__framework_prelude::NSObject, rc::Retained, runtime::{NSObjectProtocol, ProtocolObject}, define_class, AllocAnyThread, DeclaredClass},
    objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString},
    objc2_core_media::CMSampleBuffer,
    objc2_av_foundation::*,
    objc2_core_video::*,
};

#[derive(Debug, Clone, Copy)]
pub enum BayerPattern { RGGB, BGGR, GRBG, GBRG }

#[derive(Debug)]
pub struct ProcessorClass {
    pub last_frame: Mutex<Option<(Vec<u8>, usize, usize)>>,
    pub last_bayer_frame: Mutex<Option<(Vec<u16>, usize, usize, BayerPattern)>>,
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
            let Some(pixel_buffer) = (unsafe { CMSampleBuffer::image_buffer(sample_buffer) }) else { return };
            
            let (height, width, bytes_per_row) = unsafe {
                (CVPixelBufferGetHeight(&pixel_buffer), CVPixelBufferGetWidth(&pixel_buffer), CVPixelBufferGetBytesPerRow(&pixel_buffer))
            };
            
            if unsafe { CVPixelBufferLockBaseAddress(&pixel_buffer, CVPixelBufferLockFlags(0)) } != 0 { return }
            
            let base_address = unsafe { CVPixelBufferGetBaseAddress(&pixel_buffer) } as *const u8;
            if base_address.is_null() || (bytes_per_row * height) > isize::MAX as usize {
                unsafe { CVPixelBufferUnlockBaseAddress(&pixel_buffer, CVPixelBufferLockFlags(0)) };
                return;
            }

            let slice = unsafe { from_raw_parts(base_address, bytes_per_row * height) };
            
            let mut rgb_data = vec![0u8; (width * height * 3) as usize];

            for y in 0..height {
                let row_start = y * bytes_per_row;
                for x in 0..width {
                    let src_index = row_start + x * 4; 
                    let dst_index = (y * width + x) * 3; 
                    
                    if src_index + 3 < slice.len() && dst_index + 2 < rgb_data.len() {
                        rgb_data[dst_index] = slice[src_index + 2];
                        rgb_data[dst_index + 1] = slice[src_index + 1];
                        rgb_data[dst_index + 2] = slice[src_index];
                    }
                }
            }

            {
                let mut frame_guard = self.ivars().last_frame.lock().unwrap();
                *frame_guard = Some((rgb_data, width, height));
            }
            
            unsafe { CVPixelBufferUnlockBaseAddress(&pixel_buffer, CVPixelBufferLockFlags(0)) };
        }
    }

    unsafe impl AVCapturePhotoCaptureDelegate for Processor {
        #[unsafe(method(captureOutput:didFinishProcessingPhoto:error:))]
        fn captureOutput_didFinishProcessingPhoto_error(
            &self, _output: &AVCapturePhotoOutput, photo: &objc2_av_foundation::AVCapturePhoto, _error: Option<&objc2_foundation::NSError>,
        ) {
            let Some(pixel_buffer) = (unsafe { photo.pixelBuffer() }) else { return };
            let pixel_format = unsafe { CVPixelBufferGetPixelFormatType(&pixel_buffer) };
            
            let bayer_pattern = match pixel_format {
                kCVPixelFormatType_14Bayer_RGGB => BayerPattern::RGGB,
                kCVPixelFormatType_14Bayer_BGGR => BayerPattern::BGGR,
                kCVPixelFormatType_14Bayer_GRBG => BayerPattern::GRBG,
                kCVPixelFormatType_14Bayer_GBRG => BayerPattern::GBRG,
                _ => { println!("unknown pixel format..: {}", pixel_format); return; }
            };

            let (height, width, bytes_per_row) = unsafe {
                (CVPixelBufferGetHeight(&pixel_buffer), CVPixelBufferGetWidth(&pixel_buffer), CVPixelBufferGetBytesPerRow(&pixel_buffer))
            };

            if unsafe { CVPixelBufferLockBaseAddress(&pixel_buffer, CVPixelBufferLockFlags(1)) } != 0 { return }
            
            let base_address = unsafe { CVPixelBufferGetBaseAddress(&pixel_buffer) } as *const u8;
            if base_address.is_null() {
                unsafe { CVPixelBufferUnlockBaseAddress(&pixel_buffer, CVPixelBufferLockFlags(1)) };
                return;
            }

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

            {
                let mut bayer_guard = self.ivars().last_bayer_frame.lock().unwrap();
                *bayer_guard = Some((bayer_data, width, height, bayer_pattern));
            }
            
            unsafe { CVPixelBufferUnlockBaseAddress(&pixel_buffer, CVPixelBufferLockFlags(1)) };
        }
    }
);

#[cfg(any(target_os = "ios", target_os = "macos"))]
impl Processor {
    pub fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(ProcessorClass {
            last_frame: Mutex::new(None),
            last_bayer_frame: Mutex::new(None),
        });
        unsafe { objc2::msg_send![super(this), init] }
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
            let device = discovery_session.devices().into_iter().next().ok_or("No camera device found")?;
            let input = AVCaptureDeviceInput::deviceInputWithDevice_error(&device)
                .map_err(|_| "Failed to create AVCaptureDeviceInput")?;

            self.session.beginConfiguration();
            self.session.setSessionPreset(AVCaptureSessionPresetPhoto);

            if !self.session.canAddInput(&input) { return Err("Failed to add input".into()); }
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

            if !self.session.canAddOutput(&video_output) { return Err("Failed to add video output".into()); }
            self.session.addOutput(&video_output);
            self.video_output = Some(video_output);

            let photo_output = AVCapturePhotoOutput::new();
            if !self.session.canAddOutput(&photo_output) { return Err("Failed to add photo output".into()); }
            self.session.addOutput(&photo_output);
            self.photo_output = Some(photo_output);

            self.session.commitConfiguration();
            self.session.startRunning();
            Ok(())
        }
    }

    pub fn stop_camera(&self) { 
        unsafe { 
            self.session.stopRunning(); 
            *self.processor.ivars().last_frame.lock().unwrap() = None;
            *self.processor.ivars().last_bayer_frame.lock().unwrap() = None;
        } 
    }

    pub fn capture_raw_photo(&self) -> Result<(), String> {
        let photo_output = self.photo_output.as_ref().ok_or("Photo output not initialized")?;
        unsafe {
            let raw_formats = photo_output.availableRawPhotoPixelFormatTypes();
            if raw_formats.count() == 0 { return Err("No RAW formats available".into()); }

            let mut chosen_format = raw_formats.objectAtIndex(0).as_u32();
            for i in 0..raw_formats.count() {
                let format_type = raw_formats.objectAtIndex(i).as_u32();
                if format_type == kCVPixelFormatType_14Bayer_RGGB {
                    chosen_format = format_type;
                    break;
                }
            }

            let settings = AVCapturePhotoSettings::photoSettingsWithRawPixelFormatType(chosen_format);
            photo_output.capturePhotoWithSettings_delegate(&settings, ProtocolObject::from_ref(&*self.processor));
        }
        Ok(())
    }

    pub fn get_latest_frame(&self) -> Option<(Vec<u8>, usize, usize)> {
        self.processor.ivars().last_frame.lock().unwrap().clone()
    }

    pub fn get_latest_bayer_frame(&self) -> Option<(Vec<u16>, usize, usize, BayerPattern)> {
        self.processor.ivars().last_bayer_frame.lock().unwrap().clone()
    }

    pub fn clear_frames(&self) {
        *self.processor.ivars().last_frame.lock().unwrap() = None;
        *self.processor.ivars().last_bayer_frame.lock().unwrap() = None;
    }

    pub fn is_raw_supported(&self) -> bool {
        self.photo_output.as_ref().map_or(false, |output| unsafe { output.availableRawPhotoPixelFormatTypes().count() > 0 })
    }

    pub fn get_available_raw_formats(&self) -> Vec<u32> {
        self.photo_output.as_ref().map_or(Vec::new(), |output| unsafe {
            let raw_formats = output.availableRawPhotoPixelFormatTypes();
            (0..raw_formats.count()).map(|i| raw_formats.objectAtIndex(i).as_u32()).collect()
        })
    }
}
