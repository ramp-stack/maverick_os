#![allow(non_snake_case)]

use crate::hardware::{CameraSettings, CameraError};
use crate::hardware::camera::{ExposureMode, FocusMode, WhiteBalanceMode};
use image::RgbaImage;
// use std::sync::MutexGuard;
use std::sync::{Arc, Mutex};

#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2::rc::{Retained};
#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2::runtime::{ProtocolObject};
#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2::{DeclaredClass, msg_send};
#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2_foundation::{ NSArray, NSDictionary, NSNumber, NSString};
#[cfg(any(target_os = "ios", target_os = "macos"))]
use dispatch2::DispatchQueue;
#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2::ffi::nil;

#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2_av_foundation::{
    AVCaptureVideoDataOutput,
    AVCaptureDeviceDiscoverySession,
    AVCaptureSession,
    AVCaptureSessionPresetMedium,
    AVMediaTypeVideo,
    AVCaptureDeviceInput,
    AVCaptureDevicePosition,
    AVCaptureSessionPresetHigh,
    AVCaptureSessionPresetPhoto,
    AVCaptureDeviceTypeBuiltInWideAngleCamera,
    AVCaptureDevice,
    AVCaptureExposureMode,
    AVCaptureFocusMode,
    AVCaptureWhiteBalanceMode,
};

#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2_core_video::{
    kCVPixelBufferPixelFormatTypeKey,
    kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
    kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
    kCVPixelFormatType_14Bayer_GRBG,
    kCVPixelFormatType_14Bayer_BGGR,
    kCVPixelFormatType_14Bayer_GBRG,
    kCVPixelFormatType_14Bayer_RGGB,
    kCVPixelFormatType_32BGRA,
};

mod apple_custom_processor;
use apple_custom_processor::CustomProcessor;
mod apple_standard_processor;
use apple_standard_processor::StandardProcessor;

#[derive(Clone, Debug)]
pub enum AppleCamera {
    Standard(StandardAppleCamera),
    Unprocessed(UnprocessedAppleCamera)
}

impl AppleCamera {
    pub fn new_standard() -> Result<Self, CameraError> {
        Ok(AppleCamera::Standard(StandardAppleCamera::new()))
    }

    pub fn new_unprocessed() -> Result<Self, CameraError> {
        Ok(AppleCamera::Unprocessed(UnprocessedAppleCamera::new()))
    }

    pub fn frame(&self) -> Result<RgbaImage, CameraError> {
        match self {
            AppleCamera::Standard(c) => c.frame().ok_or(CameraError::FailedToGetFrame),
            AppleCamera::Unprocessed(c) => c.frame().ok_or(CameraError::FailedToGetFrame)
        }
    }

    pub fn start(&mut self) {
        let _ = match self {
            AppleCamera::Standard(c) => c.start(),
            AppleCamera::Unprocessed(c) => c.start()
        };
    }

    pub fn toggle_flashlight(&self) {
        // let _ = match self {
        //     AppleCamera::Standard(_) => {},
        //     AppleCamera::Unprocessed(c) => c.toggle_flashlight()
        // };
    }

    pub fn settings(&self) -> Option<Arc<Mutex<CameraSettings>>> {
        match self {
            AppleCamera::Standard(_) => None,
            AppleCamera::Unprocessed(c) => Some(c.settings())
        }
    }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
#[derive(Debug, Clone)]
pub struct StandardAppleCamera {
    pub session: Retained<AVCaptureSession>,
    processor: Retained<StandardProcessor>,
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
impl StandardAppleCamera {
    pub fn new() -> Self {
        unsafe {
            StandardAppleCamera {
                session: AVCaptureSession::new(),
                processor: StandardProcessor::new(),
            }
        }
    }

    pub fn start(&self) -> Result<(), String> {
        unsafe {
            let device_types = NSArray::from_slice(&[objc2_av_foundation::AVCaptureDeviceTypeBuiltInWideAngleCamera]);

            let discovery_session = AVCaptureDeviceDiscoverySession::discoverySessionWithDeviceTypes_mediaType_position(
                &device_types,
                AVMediaTypeVideo,
                AVCaptureDevicePosition::Back,
            );

            let devices = discovery_session.devices();

            let device = devices.into_iter().next().expect("No device at index 0");

            let input = AVCaptureDeviceInput::deviceInputWithDevice_error(&device)
                .expect("Failed to create AVCaptureDeviceInput");

            self.session.beginConfiguration();

            self.session.setSessionPreset(AVCaptureSessionPresetMedium);

            if self.session.canAddInput(&input) {
                self.session.addInput(&input);
            }

            let output = AVCaptureVideoDataOutput::new();

            let pixel_format_value = NSNumber::new_u32(kCVPixelFormatType_32BGRA);

            let pixel_format_key: &NSString = &*(kCVPixelBufferPixelFormatTypeKey as *const _ as *const NSString);

            let video_settings = NSDictionary::from_slices(
                &[pixel_format_key],
                &[pixel_format_value.as_ref()],
            );

            output.setVideoSettings(Some(&video_settings));

            let queue = DispatchQueue::new("CameraQueue", None);

            output.setSampleBufferDelegate_queue(
                Some(ProtocolObject::from_ref(&*self.processor)),
                Some(&queue),
            );

            if self.session.canAddOutput(&output) {
                self.session.addOutput(&output);
            }

            self.session.commitConfiguration();
            self.session.startRunning();
        }
        Ok(())
    }

    pub fn frame(&self) -> Option<RgbaImage> {
        // if !self.processor.is_ready() { return None; }
        // self.processor.ivars().last_frame.lock().unwrap().clone()
        None
    }

    pub fn stop(&self) {}
}

impl Default for StandardAppleCamera {
    fn default() -> Self { Self::new() }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
#[derive(Debug, Clone)]
pub struct UnprocessedAppleCamera {
    pub session: Retained<AVCaptureSession>,
    processor: Retained<CustomProcessor>,
    video_output: Option<Retained<AVCaptureVideoDataOutput>>,
    device: Option<Retained<AVCaptureDevice>>,
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
impl UnprocessedAppleCamera {
    pub fn new() -> Self {
        Self {
            session: unsafe { AVCaptureSession::new() },
            processor: CustomProcessor::new(),
            video_output: None,
            device: None,
        }
    }

    pub fn start(&mut self) -> Result<(), String> {
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
                device.unlockForConfiguration();
            }

            let output = AVCaptureVideoDataOutput::new();
            let key = &*(kCVPixelBufferPixelFormatTypeKey as *const _ as *const NSString);
            let supported = output.availableVideoCVPixelFormatTypes().iter().map(|f| f.unsignedIntValue()).collect::<Vec<_>>();

            for f in [kCVPixelFormatType_14Bayer_RGGB, kCVPixelFormatType_14Bayer_BGGR, kCVPixelFormatType_14Bayer_GRBG, 
                     kCVPixelFormatType_14Bayer_GBRG, kCVPixelFormatType_32BGRA, kCVPixelFormatType_420YpCbCr8BiPlanarFullRange, 
                     kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange] {
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

        Ok(())
    }

    pub fn stop(&self) {
        unsafe {
            self.session.stopRunning();
            *self.processor.ivars().last_raw_frame.lock().unwrap() = None;
            *self.processor.ivars().bayer_format_verified.lock().unwrap() = false;
            *self.processor.ivars().ready.lock().unwrap() = false;
        }
    }

    pub fn frame(&self) -> Option<RgbaImage> {
        let settings = self.settings().lock().unwrap().clone();
        if settings.is_updated { 
            let _ = self.apply_settings(&settings);
            self.settings().lock().as_mut().unwrap().is_updated = false;
        }
        if !self.processor.is_ready() { return None; }
        self.processor.ivars().last_raw_frame.lock().unwrap().clone()
    }

    pub fn settings(&self) -> Arc<Mutex<CameraSettings>> {
        self.processor.settings()
    }


    pub fn apply_settings(&self, settings: &CameraSettings) -> Result<(), CameraError> {
        unsafe {
            if let Some(device) = &self.device {
                if !device.isExposureModeSupported(objc2_av_foundation::AVCaptureExposureMode::Custom) {
                    return Err(CameraError::FailedToGetFrame);
                }

                device.lockForConfiguration().map_err(|_| CameraError::FailedToGetFrame)?;

                match settings.exposure_mode {
                    ExposureMode::Auto => {
                        device.setExposureMode(AVCaptureExposureMode::AutoExpose);
                    }
                    ExposureMode::Continuous => {
                        device.setExposureMode(AVCaptureExposureMode::ContinuousAutoExposure);
                    }
                    ExposureMode::Custom => {
                        if let Some(custom) = settings.custom_exposure {
                            let format_retained = device.activeFormat();
                            let format: &objc2::runtime::AnyObject = format_retained.as_ref();                            
                            let min_d: objc2_core_media::CMTime = msg_send![format, minExposureDuration];
                            let max_d: objc2_core_media::CMTime = msg_send![format, maxExposureDuration];

                            let dur = (min_d.value as f64 / min_d.timescale as f64)
                                + ((max_d.value as f64 / max_d.timescale as f64)
                                - (min_d.value as f64 / min_d.timescale as f64))
                                * custom.duration.clamp(0.0, 1.0) as f64;

                            let duration = objc2_core_media::CMTime {
                                value: (dur * 1_000_000_000.0) as i64,
                                timescale: 1_000_000_000,
                                flags: objc2_core_media::CMTimeFlags(0),
                                epoch: 0,
                            };

                            let min_iso = device.activeFormat().minISO();
                            let max_iso = device.activeFormat().maxISO();
                            let iso = (min_iso + (max_iso - min_iso) * custom.iso.clamp(0.0, 1.0)).clamp(min_iso, max_iso);

                            device.setExposureMode(objc2_av_foundation::AVCaptureExposureMode::Custom);
                            let () = msg_send![device, setExposureModeCustomWithDuration: duration, ISO: iso, completionHandler: nil];
                        }
                    }
                }

                // Focus
                match settings.focus_mode {
                    FocusMode::Auto => device.setFocusMode(AVCaptureFocusMode::AutoFocus),
                    FocusMode::Continuous => device.setFocusMode(AVCaptureFocusMode::ContinuousAutoFocus),
                    FocusMode::Locked => device.setFocusMode(AVCaptureFocusMode::Locked),
                    FocusMode::Manual => {
                        device.setFocusMode(AVCaptureFocusMode::Locked);
                        if let Some(pos) = settings.focus_distance {
                            let _: () = msg_send![
                                device,
                                setFocusModeLockedWithLensPosition: pos,
                                completionHandler: core::ptr::null::<objc2::runtime::AnyObject>()
                            ];

                        }
                    }
                }

                // White Balance
                match settings.white_balance_mode {
                    WhiteBalanceMode::Auto if device.isWhiteBalanceModeSupported(AVCaptureWhiteBalanceMode::AutoWhiteBalance) => {
                        device.setWhiteBalanceMode(AVCaptureWhiteBalanceMode::AutoWhiteBalance);
                    }
                    WhiteBalanceMode::Locked if device.isWhiteBalanceModeSupported(AVCaptureWhiteBalanceMode::Locked) => {
                        device.setWhiteBalanceMode(AVCaptureWhiteBalanceMode::Locked);
                    }
                    WhiteBalanceMode::Custom if device.isWhiteBalanceModeSupported(AVCaptureWhiteBalanceMode::Locked) => {
                        device.setWhiteBalanceMode(objc2_av_foundation::AVCaptureWhiteBalanceMode::Locked);
                        if let Some(gains) = &settings.white_balance_gains {
                            let max_gain = device.maxWhiteBalanceGain();
                            let wb_gains = objc2_av_foundation::AVCaptureWhiteBalanceGains {
                                redGain: 1.0 + (max_gain - 1.0) * gains.red.clamp(0.0, 1.0),
                                greenGain: 1.0 + (max_gain - 1.0) * gains.green.clamp(0.0, 1.0),
                                blueGain: 1.0 + (max_gain - 1.0) * gains.blue.clamp(0.0, 1.0),
                            };
                            let block = block2::StackBlock::new(|_: *mut objc2::runtime::AnyObject| {});
                            let _: () = msg_send![
                                device,
                                setWhiteBalanceModeLockedWithDeviceWhiteBalanceGains: wb_gains,
                                completionHandler: &*block
                            ];
                        }
                    }
                    _ => {}
                }

                device.unlockForConfiguration();
            }
        }
        Ok(())
    }
}

impl Default for UnprocessedAppleCamera {
    fn default() -> Self { Self::new() }
}

// #[cfg(any(target_os = "ios", target_os = "macos"))]
// pub trait DeviceWhiteBalanceGainsExt {
//     fn setWhiteBalanceModeLockedWithDeviceWhiteBalanceGains(&self, gains: WhiteBalanceGains);
// }

// #[cfg(any(target_os = "ios", target_os = "macos"))]
// impl DeviceWhiteBalanceGainsExt for Retained<AVCaptureDevice> {
//     fn setWhiteBalanceModeLockedWithDeviceWhiteBalanceGains(&self, gains: WhiteBalanceGains) {
//         unsafe {
//             // Create an AVCaptureWhiteBalanceGains object via Objective-C
//             let device_gains: *mut AVCaptureWhiteBalanceGains = msg_send![
//                 class!(AVCaptureWhiteBalanceGains),
//                 deviceWhiteBalanceGainsWithRed: gains.red,
//                 green: gains.green,
//                 blue: gains.blue,
//             ];

//             // Call the lock method with the gains object
//             let _: () = msg_send![self, setWhiteBalanceModeLockedWithDeviceWhiteBalanceGains: device_gains];
//         }
//     }
// }



    // #[cfg(any(target_os = "macos", target_os = "ios"))]
    // pub fn get_frame(&mut self) -> Option<RgbaImage> {
    //     match &mut self.0 {
    //         AppleCameraBackend::Standard(_cam) => None,
    //         AppleCameraBackend::Custom(cam) => cam.get_latest_raw_frame(),
    //     }
    // }

    // #[cfg(target_os = "android")]
    // pub fn get_frame(&mut self) -> (Vec<u8>, usize, usize) {
    //     let image = self.0.get_latest_frame().unwrap_or_else(|_| panic!("Failed to get frame"));
    //     let (width, height) = image.dimensions();
    //     let pixels = image.into_raw();
    //     (pixels, width as usize, height as usize)
    // }

    // #[cfg(any(target_os = "windows", target_os = "linux"))]
    // pub fn get_frame(&mut self) -> Option<RgbaImage> {
    //     self.0.capture_frame()
    // }

    // #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android", target_os = "windows", target_os = "linux")))]
    // pub fn get_frame(&mut self) -> Option<RgbaImage> {
    //     None
    // }

    // #[cfg(any(target_os = "macos", target_os = "ios"))]
    // pub fn get_latest_raw_frame(&self) -> Option<RgbaImage> {
    //     match &self.0 {
    //         AppleCameraBackend::Standard(_cam) => None,
    //         AppleCameraBackend::Custom(cam) => cam.get_latest_raw_frame(),
    //     }
    // }

    // #[cfg(any(target_os = "android", target_os = "windows", target_os = "linux"))]
    // pub fn get_latest_raw_frame(&self) -> Option<RgbaImage> {
    //     None
    // }

    // #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android", target_os = "windows", target_os = "linux")))]
    // pub fn get_latest_raw_frame(&self) -> Option<RgbaImage> {
    //     None
    // }

    // // Image processing settings - only supported on Apple platforms for now
    // #[cfg(any(target_os = "macos", target_os = "ios"))]
    // pub fn update_settings<F>(&self, update_fn: F)
    // where F: FnOnce(&mut ImageSettings),
    // {
    //     match &self.0 {
    //         AppleCameraBackend::Standard(_cam) => {
    //             panic!("Standard camera doesn't support settings updates")
    //         }
    //         AppleCameraBackend::Custom(cam) => {
    //             cam.update_settings(update_fn);
    //         }
    //     }
    // }

    // #[cfg(any(target_os = "macos", target_os = "ios"))]
    // pub fn set_exposure_and_iso(&mut self, duration: f32, iso: f32) -> Result<(), CameraError> {
    //     match &self.0 {
    //         AppleCameraBackend::Standard(_cam) => {
    //             Err(CameraError::FailedToGetFrame)
    //         }
    //         AppleCameraBackend::Custom(cam) => {
    //             // match duration_iso {
    //             //     Some((d, i)) => cam.set_exposure_and_iso(d, i),
    //             //     None => cam.disable_custom_exposure()
    //             // }.map_err(|e| CameraError::Error(e))
    //             cam.set_exposure_and_iso(duration, iso).map_err(CameraError::Unknown)
    //         }
    //     }
    // }

    // #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    // pub fn set_exposure_and_iso(&mut self, duration: f32, iso: f32) -> Result<(), CameraError> {
    //     Err(CameraError::FailedToGetFrame)
    // }

    // // Individual setter methods for all image processing parameters
    // #[cfg(any(target_os = "macos", target_os = "ios"))]
    // pub fn get_settings(&self) -> ImageSettings {
    //     match &self.0 {
    //         AppleCameraBackend::Standard(_cam) => {
    //             panic!("Standard camera doesn't support getting settings")
    //         }
    //         AppleCameraBackend::Custom(cam) => cam.get_settings(),
    //     }
    // }

    // // Individual setter methods for image processing parameters
    // #[cfg(any(target_os = "macos", target_os = "ios"))]
    // pub fn set_brightness(&mut self, brightness: i16) {
    //     match &self.0 {
    //         AppleCameraBackend::Standard(_cam) => {
    //             panic!("Standard camera doesn't support brightness adjustment")
    //         }
    //         AppleCameraBackend::Custom(cam) => {
    //             cam.update_settings(|settings| {
    //                 settings.brightness = brightness;
    //             });
    //         }
    //     }
    // }

    // #[cfg(any(target_os = "macos", target_os = "ios"))]
    // pub fn set_contrast(&mut self, contrast: f32) {
    //     match &self.0 {
    //         AppleCameraBackend::Standard(_cam) => {
    //             panic!("Standard camera doesn't support contrast adjustment")
    //         }
    //         AppleCameraBackend::Custom(cam) => {
    //             cam.update_settings(|settings| {
    //                 settings.contrast = contrast;
    //             });
    //         }
    //     }
    // }

    // #[cfg(any(target_os = "macos", target_os = "ios"))]
    // pub fn set_saturation(&mut self, saturation: f32) {
    //     match &self.0 {
    //         AppleCameraBackend::Standard(_cam) => {
    //             panic!("Standard camera doesn't support saturation adjustment")
    //         }
    //         AppleCameraBackend::Custom(cam) => {
    //             cam.update_settings(|settings| {
    //                 settings.saturation = saturation;
    //             });
    //         }
    //     }
    // }

    // #[cfg(any(target_os = "macos", target_os = "ios"))]
    // pub fn set_gamma(&mut self, gamma: f32) {
    //     match &self.0 {
    //         AppleCameraBackend::Standard(_cam) => {
    //             panic!("Standard camera doesn't support gamma adjustment")
    //         }
    //         AppleCameraBackend::Custom(cam) => {
    //             cam.update_settings(|settings| {
    //                 settings.gamma = gamma;
    //             });
    //         }
    //     }
    // }

    // #[cfg(any(target_os = "macos", target_os = "ios"))]
    // pub fn set_exposure(&mut self, exposure: f32) {
    //     match &self.0 {
    //         AppleCameraBackend::Standard(_cam) => {
    //             panic!("Standard camera doesn't support exposure adjustment")
    //         }
    //         AppleCameraBackend::Custom(cam) => {
    //             cam.update_settings(|settings| {
    //                 settings.exposure = exposure;
    //             });
    //         }
    //     }
    // }

    // #[cfg(any(target_os = "macos", target_os = "ios"))]
    // pub fn set_temperature(&mut self, temperature: f32) {
    //     match &self.0 {
    //         AppleCameraBackend::Standard(_cam) => {
    //             panic!("Standard camera doesn't support temperature adjustment")
    //         }
    //         AppleCameraBackend::Custom(cam) => {
    //             cam.update_settings(|settings| {
    //                 settings.temperature = temperature;
    //             });
    //         }
    //     }
    // }

    // #[cfg(any(target_os = "macos", target_os = "ios"))]
    // pub fn set_white_balance_rgb(&mut self, r: f32, g: f32, b: f32) {
    //     match &self.0 {
    //         AppleCameraBackend::Standard(_cam) => {
    //             panic!("Standard camera doesn't support white balance adjustment")
    //         }
    //         AppleCameraBackend::Custom(cam) => {
    //             cam.update_settings(|settings| {
    //                 settings.white_balance_r = r;
    //                 settings.white_balance_g = g;
    //                 settings.white_balance_b = b;
    //             });
    //         }
    //     }
    // }

    // #[cfg(any(target_os = "macos", target_os = "ios"))]
    // pub fn reset_settings(&mut self) {
    //     match &self.0 {
    //         AppleCameraBackend::Standard(_cam) => {
    //             panic!("Standard camera doesn't support settings reset")
    //         }
    //         AppleCameraBackend::Custom(cam) => {
    //             cam.update_settings(|settings| {
    //                 *settings = ImageSettings::default();
    //             });
    //         }
    //     }
    // }

    // // Stub implementations for platforms that don't support image processing
    // #[cfg(any(target_os = "android", target_os = "windows", target_os = "linux"))]
    // pub fn update_settings<F>(&self, _update_fn: F)
    // where F: FnOnce(&mut ImageSettings),
    // {
    //     panic!("Camera settings update not supported on this platform")
    // }

    // #[cfg(any(target_os = "android", target_os = "windows", target_os = "linux"))]
    // pub fn get_settings(&self) -> ImageSettings {
    //     panic!("Camera settings retrieval not supported on this platform")
    // }

    // #[cfg(any(target_os = "android", target_os = "windows", target_os = "linux"))]
    // pub fn set_brightness(&mut self, _brightness: i16) {
    //     panic!("Camera brightness adjustment not supported on this platform")
    // }

    // #[cfg(any(target_os = "android", target_os = "windows", target_os = "linux"))]
    // pub fn set_contrast(&mut self, _contrast: f32) {
    //     panic!("Camera contrast adjustment not supported on this platform")
    // }

    // #[cfg(any(target_os = "android", target_os = "windows", target_os = "linux"))]
    // pub fn set_saturation(&mut self, _saturation: f32) {
    //     panic!("Camera saturation adjustment not supported on this platform")
    // }

    // #[cfg(any(target_os = "android", target_os = "windows", target_os = "linux"))]
    // pub fn set_gamma(&mut self, _gamma: f32) {
    //     panic!("Camera gamma adjustment not supported on this platform")
    // }

    // #[cfg(any(target_os = "android", target_os = "windows", target_os = "linux"))]
    // pub fn set_exposure(&mut self, _exposure: f32) {
    //     panic!("Camera exposure adjustment not supported on this platform")
    // }

    // #[cfg(any(target_os = "android", target_os = "windows", target_os = "linux"))]
    // pub fn set_temperature(&mut self, _temperature: f32) {
    //     panic!("Camera temperature adjustment not supported on this platform")
    // }

    // #[cfg(any(target_os = "android", target_os = "windows", target_os = "linux"))]
    // pub fn set_white_balance_rgb(&mut self, _r: f32, _g: f32, _b: f32) {
    //     panic!("Camera white balance adjustment not supported on this platform")
    // }

    // #[cfg(any(target_os = "android", target_os = "windows", target_os = "linux"))]
    // pub fn reset_settings(&mut self) {
    //     panic!("Camera settings reset not supported on this platform")
    // }

    // // Stub implementations for unsupported platforms
    // #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android", target_os = "windows", target_os = "linux")))]
    // pub fn update_settings<F>(&self, _update_fn: F)
    // where F: FnOnce(&mut ImageSettings),
    // {
    //     panic!("Camera access denied on this platform")
    // }

    // #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android", target_os = "windows", target_os = "linux")))]
    // pub fn get_settings(&self) -> ImageSettings {
    //     panic!("Camera access denied on this platform")
    // }

    // #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android", target_os = "windows", target_os = "linux")))]
    // pub fn set_brightness(&mut self, _brightness: i16) {
    //     panic!("Camera access denied on this platform")
    // }

    // // Static convenience methods
    // #[cfg(any(target_os = "macos", target_os = "ios"))]
    // pub fn open_and_get_frame() -> (Vec<u8>, usize, usize) {
    //     panic!("Failed to get frame from standard camera")
    // }



    // #[cfg(any(target_os = "macos", target_os = "ios"))]
    // pub fn open_and_get_custom_frame() -> Option<RgbaImage> {
    //     let mut camera = UnprocessedAppleCamera::new();
    //     if camera.open_camera().is_err() {
    //         return None;
    //     }

    //     let mut wrapper = Camera(AppleCameraBackend::Custom(camera));
    //     for _ in 1..=10 {
    //         std::thread::sleep(std::time::Duration::from_millis(200));
    //         if let Some(frame) = wrapper.get_frame() {
    //             return Some(frame);
    //         }
    //     }
    //     None
    // }

    // #[cfg(target_os = "android")]
    // pub fn open_and_get_frame() -> (Vec<u8>, usize, usize) {
    //     let mut camera = AndroidCamera::new().unwrap_or_else(|_| panic!("Access denied to camera"));
    //     camera.open_camera();
    //     let mut wrapper = Camera(camera);
    //     wrapper.get_frame()
    // }

    // #[cfg(any(target_os = "windows", target_os = "linux"))]
    // pub fn open_and_get_frame() -> Option<RgbaImage> {
    //     // let mut camera = WindowsLinuxCamera::new(0);
    //     // camera.start();
    //     // let mut wrapper = Camera(camera);
    //     // wrapper.get_frame()
    //     panic!("Windows and linux not currently supported");
    // }

    // #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android", target_os = "windows", target_os = "linux")))]
    // pub fn open_and_get_frame() -> Option<RgbaImage> {
    //     None
    // }


    // pub fn temperature_to_rgb_multipliers(&self) -> [f32; 3] {
    //     let temp = self.temperature;
    //     let temp_scaled = temp / 100.0;

    //     if temp < 6600.0 {
    //         let r = 1.0;
    //         let g = (0.39008157 * temp_scaled.ln() - 0.631_841_4).clamp(0.0, 1.0);
    //         let b = if temp < 2000.0 {
    //             0.0
    //         } else {
    //             (0.54320678 * (temp_scaled - 10.0).ln() - 1.196_254_1).clamp(0.0, 1.0)
    //         };
    //         [r, g, b]
    //     } else {
    //         let r = (1.292_936_2 * (temp_scaled - 60.0).powf(-0.1332047)).clamp(0.0, 1.0);
    //         let g = (1.129_890_9 * (temp_scaled - 60.0).powf(-0.0755148)).clamp(0.0, 1.0);
    //         let b = 1.0;
    //         [r, g, b]
    //     }
    // }


// Settings for configuring camera behavior.
// #[derive(Debug, Clone)]
// pub struct CameraSettings {
//     pub brightness: i16, 
//     pub contrast: f32,
//     pub saturation: f32,
//     pub gamma: f32,
//     pub white_balance_r: f32,
//     pub white_balance_g: f32,
//     pub white_balance_b: f32,
//     pub exposure: f32,
//     pub temperature: f32,
//     pub exposure_iso: f32,
//     pub exposure_duration: f32,
// }

// impl Default for CameraSettings {
//     fn default() -> Self {
//         Self {
//             brightness: 0,
//             contrast: 0.0,
//             saturation: 0.0,
//             gamma: 2.2,
//             white_balance_r: 1.0,
//             white_balance_g: 1.0,
//             white_balance_b: 1.0,
//             exposure: 0.0,
//             temperature: 6500.0,
//             exposure_iso: 0.0,
//             exposure_duration: 0.0
//         }
//     }
// }

// impl CameraSettings {
//     pub fn new() -> Self {
//         Self::default()
//     }

//     pub fn clamp_values(&mut self) {
//         self.brightness = self.brightness.clamp(-100, 100);
//         self.contrast = self.contrast.clamp(-1.0, 1.0);
//         self.saturation = self.saturation.clamp(-1.0, 1.0);
//         self.gamma = self.gamma.clamp(0.1, 3.0);
//         self.white_balance_r = self.white_balance_r.clamp(0.5, 2.0);
//         self.white_balance_g = self.white_balance_g.clamp(0.5, 2.0);
//         self.white_balance_b = self.white_balance_b.clamp(0.5, 2.0);
//         self.exposure = self.exposure.clamp(-2.0, 2.0);
//         self.temperature = self.temperature.clamp(2000.0, 10000.0);
//     }
// }


    // pub fn update_settings<F>(&self, f: F) where F: FnOnce(&mut ImageSettings) { self.processor.update_settings(f); }
    // pub fn get_settings(&self) -> ImageSettings { self.processor.get_settings() }

    // pub fn set_exposure_and_iso(&self, d: f32, i: f32) -> Result<(), String> {
    //     unsafe {
    //         if let Some(device) = &self.device {
    //             if !device.isExposureModeSupported(objc2_av_foundation::AVCaptureExposureMode::Custom) {
    //                 return Err("Custom exposure not supported".into());
    //             }

    //             device.lockForConfiguration().map_err(|_| "Could not lock device")?;

    //             let format = device.activeFormat();
    //             let fmt: &objc2::runtime::Object = format.as_ref();
    //             let min_d: objc2_core_media::CMTime = msg_send![fmt, minExposureDuration];
    //             let max_d: objc2_core_media::CMTime = msg_send![fmt, maxExposureDuration];

    //             let dur = (min_d.value as f64 / min_d.timescale as f64)
    //                 + ((max_d.value as f64 / max_d.timescale as f64)
    //                 - (min_d.value as f64 / min_d.timescale as f64))
    //                 * (d / 100.0).clamp(0.0, 1.0) as f64;

    //             let duration = objc2_core_media::CMTime {
    //                 value: (dur * 1_000_000_000.0) as i64,
    //                 timescale: 1_000_000_000,
    //                 flags: objc2_core_media::CMTimeFlags(0),
    //                 epoch: 0,
    //             };

    //             let min_iso = format.minISO();
    //             let max_iso = format.maxISO();

    //             let iso = match min_iso == 0.0 && max_iso == 0.0 {
    //                 true => min_iso + (max_iso - min_iso) * (i / 100.0).clamp(0.0, 1.0),
    //                 false => (min_iso + (max_iso - min_iso) * (i / 100.0).clamp(0.0, 1.0)).clamp(min_iso, max_iso)
    //             };

    //             device.setExposureMode(objc2_av_foundation::AVCaptureExposureMode::Custom);
    //             let () = msg_send![device, setExposureModeCustomWithDuration: duration ISO: iso completionHandler: nil];
    //             device.unlockForConfiguration();
    //             Ok(())
    //         } else {
    //             Err("No device available".into())
    //         }
    //     }
    // }

    // pub fn disable_custom_exposure(&self) -> Result<(), String> {
    //     unsafe {
    //         if let Some(device) = &self.device {
    //             println!("[Camera] Locking device for configuration to disable custom exposure...");
    //             if device.lockForConfiguration().is_err() {
    //                 return Err("Could not lock device for configuration".into());
    //             }

    //             if !device.isExposureModeSupported(objc2_av_foundation::AVCaptureExposureMode::ContinuousAutoExposure) {
    //                 device.unlockForConfiguration();
    //                 println!("[Camera] Continuous Auto Exposure not supported on this device.");
    //                 return Err("Continuous Auto Exposure not supported".into());
    //             }

    //             println!("[Camera] Switching exposure mode to Continuous Auto Exposure...");
    //             device.setExposureMode(objc2_av_foundation::AVCaptureExposureMode::ContinuousAutoExposure);

    //             device.unlockForConfiguration();
    //             println!("[Camera] Device unlocked, auto exposure enabled.");
    //             Ok(())
    //         } else {
    //             Err("No device available".into())
    //         }
    //     }
    // }