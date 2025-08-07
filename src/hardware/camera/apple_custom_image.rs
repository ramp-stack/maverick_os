#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

use std::{sync::{Mutex, Arc, atomic::{AtomicBool, Ordering}}, slice::from_raw_parts};
use image::RgbaImage;
use crate::hardware::camera::apple_custom_utils::{BayerPattern, ImageSettings, ImageProcessor};

#[cfg(any(target_os = "ios", target_os = "macos"))]
use dispatch2::DispatchQueue;
#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2::__framework_prelude::NSObject;
#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2::rc::Retained;
#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2::runtime::{NSObjectProtocol, ProtocolObject};
#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2::{define_class, AllocAnyThread, DeclaredClass};
#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};
#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2_core_media::CMSampleBuffer;
#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2_av_foundation::*;
#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2_core_video::*;

#[derive(Debug)]
pub struct ProcessorClass {
    pub last_raw_frame: Arc<Mutex<Option<RgbaImage>>>,
    pub settings: Arc<Mutex<ImageSettings>>,
    pub bayer_format_verified: AtomicBool,
    pub ready: AtomicBool,
    pub frame_buffer: Arc<Mutex<Vec<u8>>>, // Reusable buffer
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
            &self,
            _output: &AVCaptureOutput,
            sample_buffer: &CMSampleBuffer,
            _connection: &AVCaptureConnection,
        ) {
            // Fast atomic check/set for ready state
            if !self.ivars().ready.load(Ordering::Relaxed) {
                self.ivars().ready.store(true, Ordering::Relaxed);
            }
            
            // Process frame asynchronously to avoid blocking the capture thread
            if let Some(raw_image) = self.process_sample_buffer(sample_buffer) {
                // Clone settings once outside the critical section
                let settings = {
                    let settings_lock = self.ivars().settings.lock();
                    if let Ok(s) = settings_lock {
                        s.clone()
                    } else {
                        return; // Skip frame if can't get settings
                    }
                };
                
                // Apply settings and store result
                let processed_image = ImageProcessor::apply_image_settings(raw_image, &settings);
                
                // Quick write to shared buffer - use try_lock to avoid blocking
                if let Ok(mut frame) = self.ivars().last_raw_frame.try_lock() {
                    *frame = Some(processed_image);
                }
            }
        }
    }
);

#[cfg(any(target_os = "ios", target_os = "macos"))]
impl Processor {
    pub fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(ProcessorClass {
            last_raw_frame: Arc::new(Mutex::new(None)),
            settings: Arc::new(Mutex::new(ImageSettings::default())),
            bayer_format_verified: AtomicBool::new(false),
            ready: AtomicBool::new(false),
            frame_buffer: Arc::new(Mutex::new(Vec::with_capacity(1920 * 1080 * 4))),
        });
        unsafe { objc2::msg_send![super(this), init] }
    }

    #[inline(always)]
    fn process_sample_buffer(&self, sample_buffer: &CMSampleBuffer) -> Option<RgbaImage> {
        let pixel_buffer = unsafe { CMSampleBuffer::image_buffer(sample_buffer)? };
        self.process_pixel_buffer(&pixel_buffer)
    }

    #[inline(always)]
    fn process_pixel_buffer(&self, pixel_buffer: &CVPixelBuffer) -> Option<RgbaImage> {
        // Lock once at the beginning
        unsafe { CVPixelBufferLockBaseAddress(pixel_buffer, CVPixelBufferLockFlags(0)) };

        let format = unsafe { CVPixelBufferGetPixelFormatType(pixel_buffer) };
        let (h, w, row_stride) = unsafe {
            (
                CVPixelBufferGetHeight(pixel_buffer),
                CVPixelBufferGetWidth(pixel_buffer),
                CVPixelBufferGetBytesPerRow(pixel_buffer),
            )
        };

        let result = match format {
            kCVPixelFormatType_14Bayer_RGGB => {
                self.ivars().bayer_format_verified.store(true, Ordering::Relaxed);
                self.process_bayer(pixel_buffer, w, h, row_stride, BayerPattern::RGGB)
            },
            kCVPixelFormatType_14Bayer_BGGR => {
                self.ivars().bayer_format_verified.store(true, Ordering::Relaxed);
                self.process_bayer(pixel_buffer, w, h, row_stride, BayerPattern::BGGR)
            },
            kCVPixelFormatType_14Bayer_GRBG => {
                self.ivars().bayer_format_verified.store(true, Ordering::Relaxed);
                self.process_bayer(pixel_buffer, w, h, row_stride, BayerPattern::GRBG)
            },
            kCVPixelFormatType_14Bayer_GBRG => {
                self.ivars().bayer_format_verified.store(true, Ordering::Relaxed);
                self.process_bayer(pixel_buffer, w, h, row_stride, BayerPattern::GBRG)
            },
            kCVPixelFormatType_32BGRA => self.process_bgra(pixel_buffer, w, h, row_stride),
            kCVPixelFormatType_420YpCbCr8BiPlanarFullRange |
            kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange => self.process_yuv(pixel_buffer, w, h),
            _ => None,
        };

        unsafe { CVPixelBufferUnlockBaseAddress(pixel_buffer, CVPixelBufferLockFlags(0)) };
        result
    }

    #[inline(always)]
    fn process_bayer(
        &self,
        pixel_buffer: &CVPixelBuffer,
        width: usize,
        height: usize,
        row_bytes: usize,
        pattern: BayerPattern,
    ) -> Option<RgbaImage> {
        let addr = unsafe { CVPixelBufferGetBaseAddress(pixel_buffer) } as *const u8;
        if addr.is_null() {
            None
        } else {
            ImageProcessor::process_bayer_data(addr, width, height, row_bytes, pattern)
        }
    }

    #[inline(always)]
    fn process_bgra(
        &self,
        pixel_buffer: &CVPixelBuffer,
        width: usize,
        height: usize,
        row_bytes: usize,
    ) -> Option<RgbaImage> {
        let addr = unsafe { CVPixelBufferGetBaseAddress(pixel_buffer) } as *const u8;
        if addr.is_null() {
            return None;
        }

        let data = unsafe { from_raw_parts(addr, height * row_bytes) };
        
        // Try to reuse existing buffer to avoid allocations
        let mut rgba = if let Ok(mut buffer) = self.ivars().frame_buffer.try_lock() {
            buffer.clear();
            if buffer.capacity() < width * height * 4 {
                buffer.reserve(width * height * 4 - buffer.capacity());
            }
            std::mem::take(&mut *buffer)
        } else {
            Vec::with_capacity(width * height * 4)
        };

        // Optimize the pixel conversion loop
        rgba.reserve_exact(width * height * 4);
        for y in 0..height {
            let row_start = y * row_bytes;
            let row_end = row_start + width * 4;
            let row = &data[row_start..row_end];
            
            // Process 4 pixels at a time when possible
            let chunks = row.chunks_exact(16);
            let remainder = chunks.remainder();
            
            for chunk in chunks {
                // Unroll 4 pixels
                rgba.extend_from_slice(&[chunk[2], chunk[1], chunk[0], chunk[3]]);   // px 1
                rgba.extend_from_slice(&[chunk[6], chunk[5], chunk[4], chunk[7]]);   // px 2
                rgba.extend_from_slice(&[chunk[10], chunk[9], chunk[8], chunk[11]]); // px 3
                rgba.extend_from_slice(&[chunk[14], chunk[13], chunk[12], chunk[15]]); // px 4
            }
            
            // Handle remaining pixels
            for px in remainder.chunks_exact(4) {
                rgba.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
            }
        }

        RgbaImage::from_raw(width as u32, height as u32, rgba)
    }

    #[inline(always)]
    fn process_yuv(
        &self,
        pb: &CVPixelBuffer,
        width: usize,
        height: usize,
    ) -> Option<RgbaImage> {
        let y_base = unsafe { CVPixelBufferGetBaseAddressOfPlane(pb, 0) } as *const u8;
        let uv_base = unsafe { CVPixelBufferGetBaseAddressOfPlane(pb, 1) } as *const u8;
        if y_base.is_null() || uv_base.is_null() {
            return None;
        }

        let y_stride = unsafe { CVPixelBufferGetBytesPerRowOfPlane(pb, 0) };
        let uv_stride = unsafe { CVPixelBufferGetBytesPerRowOfPlane(pb, 1) };

        let y_data = unsafe { from_raw_parts(y_base, y_stride * height) };
        let uv_data = unsafe { from_raw_parts(uv_base, uv_stride * height / 2) };
        
        // Try to reuse buffer
        let mut out = if let Ok(mut buffer) = self.ivars().frame_buffer.try_lock() {
            buffer.clear();
            if buffer.capacity() < width * height * 4 {
                buffer.reserve(width * height * 4 - buffer.capacity());
            }
            std::mem::take(&mut *buffer)
        } else {
            Vec::with_capacity(width * height * 4)
        };

        out.reserve_exact(width * height * 4);

        // Optimize YUV to RGB conversion with better loop structure
        for j in 0..height {
            let y_row_start = j * y_stride;
            let uv_row_start = (j / 2) * uv_stride;
            
            for i in 0..width {
                let yv = y_data[y_row_start + i] as f32;
                let uv_idx = uv_row_start + (i & !1);
                let u = uv_data[uv_idx] as f32 - 128.0;
                let v = uv_data[uv_idx + 1] as f32 - 128.0;

                // Optimized YUV to RGB conversion
                let r = (yv + 1.402 * v).clamp(0.0, 255.0) as u8;
                let g = (yv - 0.344 * u - 0.714 * v).clamp(0.0, 255.0) as u8;
                let b = (yv + 1.772 * u).clamp(0.0, 255.0) as u8;

                out.extend_from_slice(&[r, g, b, 255]);
            }
        }

        RgbaImage::from_raw(width as u32, height as u32, out)
    }

    pub fn update_settings<F>(&self, f: F)
    where F: FnOnce(&mut ImageSettings) {
        if let Ok(mut s) = self.ivars().settings.try_lock() {
            f(&mut *s);
            s.clamp_values();
        }
    }

    pub fn get_settings(&self) -> ImageSettings {
        self.ivars().settings.lock().unwrap_or_else(|_| {
            // Return default settings if lock is poisoned
            Mutex::new(ImageSettings::default()).into_inner().unwrap()
        }).clone()
    }

    pub fn is_bayer_verified(&self) -> bool {
        self.ivars().bayer_format_verified.load(Ordering::Relaxed)
    }

    pub fn is_ready(&self) -> bool {
        self.ivars().ready.load(Ordering::Relaxed)
    }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
#[derive(Debug, Clone)]
pub struct AppleCustomCamera {
    pub session: Retained<AVCaptureSession>,
    processor: Retained<Processor>,
    video_output: Option<Retained<AVCaptureVideoDataOutput>>,
    device: Option<Retained<AVCaptureDevice>>,
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
impl AppleCustomCamera {
    pub fn new() -> Self {
        Self {
            session: unsafe { AVCaptureSession::new() },
            processor: Processor::new(),
            video_output: None,
            device: None,
        }
    }

    pub fn open_camera(&mut self) -> Result<(), String> {
        unsafe {
            let device_types = NSArray::from_slice(&[AVCaptureDeviceTypeBuiltInWideAngleCamera]);
            let discovery = AVCaptureDeviceDiscoverySession::discoverySessionWithDeviceTypes_mediaType_position(
                &device_types, AVMediaTypeVideo, AVCaptureDevicePosition::Back);
            let device = discovery.devices().into_iter().next().ok_or("No camera device")?;

            self.device = Some(device.clone());

            let input = AVCaptureDeviceInput::deviceInputWithDevice_error(&device)
                .map_err(|e| format!("Input error: {:?}", e))?;

            self.session.beginConfiguration();

            // Prioritize performance presets
            for preset in [AVCaptureSessionPresetMedium, AVCaptureSessionPresetHigh, AVCaptureSessionPresetPhoto] {
                if self.session.canSetSessionPreset(preset) {
                    self.session.setSessionPreset(preset);
                    break;
                }
            }

            self.session.addInput(&input);

            // Quick device configuration
            if device.lockForConfiguration().is_ok() {
                let _ = device.formats();
                device.unlockForConfiguration();
            }

            let output = AVCaptureVideoDataOutput::new();
            let key = &*(kCVPixelBufferPixelFormatTypeKey as *const _ as *const NSString);
            let supported = output.availableVideoCVPixelFormatTypes()
                .iter()
                .map(|f| f.unsignedIntValue())
                .collect::<Vec<_>>();

            // Prioritize faster formats (BGRA and YUV are typically faster than Bayer)
            for f in [
                kCVPixelFormatType_32BGRA,
                kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
                kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
                kCVPixelFormatType_14Bayer_RGGB,
                kCVPixelFormatType_14Bayer_BGGR,
                kCVPixelFormatType_14Bayer_GRBG,
                kCVPixelFormatType_14Bayer_GBRG,
            ] {
                if supported.contains(&f) {
                    let settings = NSDictionary::from_slices(&[key], &[NSNumber::new_u32(f).as_ref()]);
                    let queue = DispatchQueue::new("CameraQueue", None);
                    output.setVideoSettings(Some(&settings));
                    output.setAlwaysDiscardsLateVideoFrames(true);
                    
                    // Set minimum frame duration for smoother performance
                    if let Some(connection) = output.connectionWithMediaType(AVMediaTypeVideo) {
                        if connection.isVideoMinFrameDurationSupported() {
                            // Set to 30 FPS max for smoother performance
                            let duration = unsafe { 
                                objc2_core_media::CMTimeMake(1, 30)
                            };
                            connection.setVideoMinFrameDuration(duration);
                        }
                    }
                    
                    output.setSampleBufferDelegate_queue(Some(ProtocolObject::from_ref(&*self.processor)), Some(&queue));
                    self.session.addOutput(&output);
                    self.video_output = Some(output);
                    break;
                }
            }

            self.session.commitConfiguration();
            self.session.startRunning();
        }

        Ok(())
    }

    pub fn stop_camera(&self) {
        unsafe {
            self.session.stopRunning();
            
            // Clear state atomically
            self.processor.ivars().ready.store(false, Ordering::Relaxed);
            self.processor.ivars().bayer_format_verified.store(false, Ordering::Relaxed);
            
            // Quick clear of frame buffer
            if let Ok(mut frame) = self.processor.ivars().last_raw_frame.try_lock() {
                *frame = None;
            }
        }
    }

    #[inline(always)]
    pub fn get_latest_raw_frame(&self) -> Option<RgbaImage> {
        if !self.processor.is_ready() {
            return None;
        }

        // Use try_lock to avoid blocking if frame is being written
        self.processor.ivars().last_raw_frame.try_lock().ok()?.clone()
    }

    #[inline(always)]
    pub fn update_settings<F>(&self, f: F) where F: FnOnce(&mut ImageSettings) {
        self.processor.update_settings(f);
    }

    #[inline(always)]
    pub fn get_settings(&self) -> ImageSettings {
        self.processor.get_settings()
    }
}