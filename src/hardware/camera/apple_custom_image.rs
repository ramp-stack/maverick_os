#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

use std::{sync::Mutex, slice::from_raw_parts};
use image::RgbaImage;
use crate::hardware::ImageSettings;
use crate::hardware::camera::apple_custom_utils::{BayerPattern, ImageProcessor};

#[cfg(any(target_os = "ios", target_os = "macos"))]
use dispatch2::DispatchQueue;
#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2::__framework_prelude::NSObject;
#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2::rc::Retained;
#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2::runtime::{NSObjectProtocol, ProtocolObject};
#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2::{define_class, AllocAnyThread, DeclaredClass, msg_send};
#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};
#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2_core_media::CMSampleBuffer;
#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2_av_foundation::*;
#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2_core_video::*;
#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2_core_media::CMTime;
#[cfg(any(target_os = "ios", target_os = "macos"))]
use block::ConcreteBlock;
#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2::ffi::nil;

#[derive(Debug)]
pub struct ProcessorClass {
    pub last_raw_frame: Mutex<Option<RgbaImage>>,
    pub settings: Mutex<ImageSettings>,
    pub bayer_format_verified: Mutex<bool>,
    pub ready: Mutex<bool>,  
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
            let mut ready_lock = self.ivars().ready.lock().unwrap();
            if !*ready_lock {
                *ready_lock = true;
            }
            drop(ready_lock);
            
            if let Some(raw_image) = self.process_sample_buffer(sample_buffer) {
                let settings = self.get_settings();
                let processed_image = ImageProcessor::apply_image_settings(raw_image, &settings);

                #[cfg(target_os = "ios")]
                let processed_image = self.rotate_90_cw(&processed_image);

                *self.ivars().last_raw_frame.lock().unwrap() = Some(processed_image);
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
            bayer_format_verified: Mutex::new(false),
            ready: Mutex::new(false), 
        });
        unsafe { objc2::msg_send![super(this), init] }
    }

    fn process_sample_buffer(&self, sample_buffer: &CMSampleBuffer) -> Option<RgbaImage> {
        let pixel_buffer = unsafe { CMSampleBuffer::image_buffer(sample_buffer)? };
        self.process_pixel_buffer(&pixel_buffer)
    }

    fn process_pixel_buffer(&self, pixel_buffer: &CVPixelBuffer) -> Option<RgbaImage> {
        println!("process_pixel_buffer runs");
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
            kCVPixelFormatType_14Bayer_RGGB => self.process_bayer(pixel_buffer, w, h, row_stride, BayerPattern::RGGB),
            kCVPixelFormatType_14Bayer_BGGR => self.process_bayer(pixel_buffer, w, h, row_stride, BayerPattern::BGGR),
            kCVPixelFormatType_14Bayer_GRBG => self.process_bayer(pixel_buffer, w, h, row_stride, BayerPattern::GRBG),
            kCVPixelFormatType_14Bayer_GBRG => self.process_bayer(pixel_buffer, w, h, row_stride, BayerPattern::GBRG),
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
        *self.ivars().bayer_format_verified.lock().unwrap() = true;
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
        let mut rgba = Vec::with_capacity(width * height * 4);

        for y in 0..height {
            let row = &data[y * row_bytes..][..width * 4];
            for px in row.chunks_exact(4) {
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

        let y = unsafe { from_raw_parts(y_base, y_stride * height) };
        let uv = unsafe { from_raw_parts(uv_base, uv_stride * height / 2) };
        let mut out = Vec::with_capacity(width * height * 4);

        for j in 0..height {
            for i in 0..width {
                let yv = y[j * y_stride + i] as f32;
                let uv_idx = (j / 2) * uv_stride + (i & !1);
                let u = uv[uv_idx] as f32;
                let v = uv[uv_idx + 1] as f32;

                let r = (yv + 1.402 * (v - 128.0)).clamp(0.0, 255.0) as u8;
                let g = (yv - 0.344 * (u - 128.0) - 0.714 * (v - 128.0)).clamp(0.0, 255.0) as u8;
                let b = (yv + 1.772 * (u - 128.0)).clamp(0.0, 255.0) as u8;

                out.extend_from_slice(&[r, g, b, 255]);
            }
        }
        RgbaImage::from_raw(width as u32, height as u32, out)
    }

    pub fn update_settings<F>(&self, f: F)
    where F: FnOnce(&mut ImageSettings) {
        let mut s = self.ivars().settings.lock().unwrap();
        f(&mut s);
        s.clamp_values();
    }

    pub fn get_settings(&self) -> ImageSettings {
        self.ivars().settings.lock().unwrap().clone()
    }

    pub fn is_ready(&self) -> bool {
        *self.ivars().ready.lock().unwrap()
    }
    
    fn rotate_90_cw(&self, img: &RgbaImage) -> RgbaImage {
        let (width, height) = img.dimensions();
        let mut rotated = RgbaImage::new(height, width);
        for y in 0..height {
            for x in 0..width {
                let px = *img.get_pixel(x, y);
                rotated.put_pixel(height - 1 - y, x, px);
            }
        }
        rotated
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
        // let start = std::time::Instant::now();

        unsafe {
            let device_types = NSArray::from_slice(&[AVCaptureDeviceTypeBuiltInWideAngleCamera]);
            let discovery = AVCaptureDeviceDiscoverySession::discoverySessionWithDeviceTypes_mediaType_position(
                &device_types, AVMediaTypeVideo, AVCaptureDevicePosition::Back);
            let device = discovery.devices().into_iter().next().ok_or("No camera device")?;

            self.device = Some(device.clone());

            let input = AVCaptureDeviceInput::deviceInputWithDevice_error(&device)
                .map_err(|e| format!("Input error: {e:?}"))?;

            self.session.beginConfiguration();

            for preset in [AVCaptureSessionPresetPhoto, AVCaptureSessionPresetHigh, AVCaptureSessionPresetMedium] {
                if self.session.canSetSessionPreset(preset) {
                    self.session.setSessionPreset(preset);
                    break;
                }
            }

            self.session.addInput(&input);

            if device.lockForConfiguration().is_ok() {
                let _ = device.formats();
                device.unlockForConfiguration();
            }

            let output = AVCaptureVideoDataOutput::new();
            let key = &*(kCVPixelBufferPixelFormatTypeKey as *const _ as *const NSString);
            let supported = output.availableVideoCVPixelFormatTypes().iter().map(|f| f.unsignedIntValue()).collect::<Vec<_>>();

            for f in [
                kCVPixelFormatType_14Bayer_RGGB,
                kCVPixelFormatType_14Bayer_BGGR,
                kCVPixelFormatType_14Bayer_GRBG,
                kCVPixelFormatType_14Bayer_GBRG,
                kCVPixelFormatType_32BGRA,
                kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
                kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
            ] {
                if supported.contains(&f) {
                    let settings = NSDictionary::from_slices(&[key], &[NSNumber::new_u32(f).as_ref()]);
                    let queue = DispatchQueue::new("CameraQueue", None);
                    output.setVideoSettings(Some(&settings));
                    output.setAlwaysDiscardsLateVideoFrames(true);
                    output.setSampleBufferDelegate_queue(Some(ProtocolObject::from_ref(&*self.processor)), Some(&queue));
                    self.session.addOutput(&output);
                    self.video_output = Some(output);
                    break;
                }
            }

            self.session.commitConfiguration();
            self.session.startRunning();
        }

        // let elapsed = start.elapsed().as_millis();
        // println!("open_camera took: {} ms", elapsed);

        Ok(())
    }

    pub fn stop_camera(&self) {
        unsafe {
            self.session.stopRunning();
            *self.processor.ivars().last_raw_frame.lock().unwrap() = None;
            *self.processor.ivars().bayer_format_verified.lock().unwrap() = false;
            *self.processor.ivars().ready.lock().unwrap() = false; 
        }
    }

    pub fn get_latest_raw_frame(&self) -> Option<RgbaImage> {
        if !self.processor.is_ready() {
            return None;
        }
        self.processor.ivars().last_raw_frame.lock().unwrap().clone()
    }

    pub fn update_settings<F>(&self, f: F) where F: FnOnce(&mut ImageSettings) {
        self.processor.update_settings(f);
    }

    pub fn get_settings(&self) -> ImageSettings {
        self.processor.get_settings()
    }

    pub fn set_exposure_and_iso(&self, d: f32, i: f32) -> Result<(), String> {
        unsafe {
            if let Some(device) = &self.device {
                if !device.isExposureModeSupported(objc2_av_foundation::AVCaptureExposureMode::Custom) {
                    return Err("Custom exposure not supported".into());
                }

                device.lockForConfiguration().map_err(|_| "Could not lock device")?;

                let format = device.activeFormat();
                let fmt: &objc2::runtime::Object = format.as_ref();
                let min_d: objc2_core_media::CMTime = msg_send![fmt, minExposureDuration];
                let max_d: objc2_core_media::CMTime = msg_send![fmt, maxExposureDuration];

                let dur = (min_d.value as f64 / min_d.timescale as f64)
                    + ((max_d.value as f64 / max_d.timescale as f64)
                    - (min_d.value as f64 / min_d.timescale as f64))
                    * (d / 100.0).clamp(0.0, 1.0) as f64;

                let duration = objc2_core_media::CMTime {
                    value: (dur * 1_000_000_000.0) as i64,
                    timescale: 1_000_000_000,
                    flags: objc2_core_media::CMTimeFlags(0),
                    epoch: 0,
                };

                let min_iso = format.minISO();
                let max_iso = format.maxISO();

                let iso = match min_iso == 0.0 && max_iso == 0.0 {
                    true => min_iso + (max_iso - min_iso) * (i / 100.0).clamp(0.0, 1.0),
                    false => (min_iso + (max_iso - min_iso) * (i / 100.0).clamp(0.0, 1.0)).clamp(min_iso, max_iso)
                };

                device.setExposureMode(objc2_av_foundation::AVCaptureExposureMode::Custom);
                let () = msg_send![device, setExposureModeCustomWithDuration: duration ISO: iso completionHandler: nil];
                device.unlockForConfiguration();
                Ok(())
            } else {
                Err("No device available".into())
            }
        }
    }

    pub fn disable_custom_exposure(&self) -> Result<(), String> {
        unsafe {
            if let Some(device) = &self.device {
                println!("[Camera] Locking device for configuration to disable custom exposure...");
                if device.lockForConfiguration().is_err() {
                    return Err("Could not lock device for configuration".into());
                }

                if !device.isExposureModeSupported(objc2_av_foundation::AVCaptureExposureMode::ContinuousAutoExposure) {
                    device.unlockForConfiguration();
                    println!("[Camera] Continuous Auto Exposure not supported on this device.");
                    return Err("Continuous Auto Exposure not supported".into());
                }

                println!("[Camera] Switching exposure mode to Continuous Auto Exposure...");
                device.setExposureMode(objc2_av_foundation::AVCaptureExposureMode::ContinuousAutoExposure);

                device.unlockForConfiguration();
                println!("[Camera] Device unlocked, auto exposure enabled.");
                Ok(())
            } else {
                Err("No device available".into())
            }
        }
    }

}
