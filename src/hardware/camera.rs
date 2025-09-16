#[cfg(any(target_os = "ios", target_os = "macos"))]
mod apple;
#[cfg(target_os = "android")]
mod android;
#[cfg(any(target_os = "windows", target_os = "linux"))]
mod windows_linux;

#[cfg(any(target_os = "macos", target_os = "ios"))]
use crate::hardware::camera::apple::AppleCamera;
#[cfg(target_os = "android")]
use crate::hardware::camera::android::AndroidCamera;
#[cfg(any(target_os = "windows", target_os = "linux"))]
use crate::hardware::camera::windows_linux::WindowsLinuxCamera;

use image::RgbaImage;
use std::sync::{Arc, Mutex};

// Define CameraError here since it's not exported from the camera module
#[derive(Debug)]
pub enum CameraError {
    FailedToGetFrame,
    InitializationFailed,
    DeviceNotFound,
    PermissionDenied,
    Unknown(String),
}

impl std::error::Error for CameraError {}

impl std::fmt::Display for CameraError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CameraError::InitializationFailed => write!(f, "Camera initialization failed"),
            CameraError::DeviceNotFound => write!(f, "Camera device not found"),
            CameraError::PermissionDenied => write!(f, "Camera permission denied"),
            CameraError::FailedToGetFrame => write!(f, "Camera failed to get frame"),
            CameraError::Unknown(msg) => write!(f, "Unknown camera error: {msg}"),
        }
    }
}

/// Access the device camera.
#[derive(Debug, Clone)]
pub struct Camera(
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    AppleCamera,
    #[cfg(target_os = "android")]
    AndroidCamera,
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    WindowsLinuxCamera,
);

impl Camera {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn new() -> Result<Self, CameraError> {
        Ok(Camera(AppleCamera::new_standard()?))
    }

    #[cfg(target_os = "android")]
    pub fn new() -> Result<Self, CameraError> {
        Ok(Camera(AndroidCamera::new().map_err(CameraError::DeviceNotFound)?))
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    pub fn new() -> Result<Self, CameraError> {
        Ok(Camera(WindowsLinuxCamera::new()?))
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android", target_os = "windows", target_os = "linux")))]
    pub fn new() -> Result<Self, CameraError> {
        Err(CameraError::DeviceNotFound)
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn new_unprocessed() -> Result<Self, CameraError> {
        Ok(Camera(AppleCamera::new_unprocessed()?))
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    pub fn new_unprocessed() -> Result<Self, CameraError> {
        Err(CameraError::DeviceNotFound)
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    pub fn inner(&mut self) -> &mut WindowsLinuxCamera { &mut self.0 }

    #[cfg(target_os = "android")]
    pub fn inner(&mut self) -> &mut AndroidCamera { &mut self.0 }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn inner(&mut self) -> &mut AppleCamera { &mut self.0 }

    // pub fn toggle_flashlight(&mut self) { self.0.toggle_flashlight() }

    pub fn frame(&self) -> Result<RgbaImage, CameraError> { self.0.frame() }

    pub fn start(mut self) -> Self {
        self.0.start();
        self
    }

    pub fn settings(&mut self) -> Option<Arc<Mutex<CameraSettings>>> { 
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        return self.0.settings(); 
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        None
    }
}

impl Drop for Camera {
    fn drop(&mut self) {}
}

#[derive(Debug, Clone)]
pub struct CameraSettings {
    pub exposure_mode: ExposureMode, //
    pub custom_exposure: Option<CustomExposure>, // duration + ISO
    pub exposure_compensation: Option<f32>,      // EV
    pub exposure_stacking: bool,

    pub focus_mode: FocusMode,
    pub focus_distance: Option<f32>,            // 0.0..1.0 lens position for manual
    pub focus_point_of_interest: Option<(f32,f32)>, // normalized x,y for focus

    pub white_balance_mode: WhiteBalanceMode,
    pub white_balance_gains: Option<WhiteBalanceGains>,

    pub zoom_factor: Option<f32>,

    pub frame_rate: Option<f32>,
    pub resolution: Option<Resolution>,
    pub hdr_enabled: bool,
    pub stabilization_enabled: bool,
    
    pub low_light_boost: Option<bool>,
    pub scene_mode_hint: Option<SceneMode>,

    pub brightness: Option<f32>,
    pub contrast: Option<f32>,
    pub saturation: Option<f32>,
    pub sharpness: Option<f32>,
    pub hue: Option<f32>,
    pub noise_reduction: Option<f32>,
    pub gamma: Option<f32>,
    pub color_filter: Option<ColorFilter>,
    
    pub is_updated: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct CustomExposure {
    pub duration: f32, // seconds
    pub iso: f32,
}

impl Default for CustomExposure {
    fn default() -> Self {
        Self {
            iso: 0.0,
            duration: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ExposureMode {
    Auto,
    Continuous,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FocusMode {
    Auto,
    Continuous,
    Locked,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WhiteBalanceMode {
    Auto,
    Locked,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WhiteBalanceGains {
    pub red: f32,
    pub green: f32,
    pub blue: f32,
}

impl WhiteBalanceGains {
    fn from(red: f32, green: f32, blue: f32) -> Self {
        WhiteBalanceGains { red, green, blue }
    }
}

impl Default for WhiteBalanceGains {
    fn default() -> Self {
        Self {
            red: 0.0,
            green: 0.0,
            blue: 0.0
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy)]
pub enum SceneMode {
    Standard,
    Portrait,
    Night,
    Action,
    Backlit,
    Macro,
}

#[derive(Debug, Clone, Copy)]
pub enum ColorFilter {
    None,
    Sepia,
    Mono,
    Vibrant,
    Cool,
    Warm,
}

impl Default for CameraSettings {
    fn default() -> Self {
        Self {
            exposure_mode: ExposureMode::Continuous,
            custom_exposure: None,
            exposure_compensation: Some(0.0),
            exposure_stacking: false,
            focus_mode: FocusMode::Auto,
            focus_distance: Some(0.5),
            focus_point_of_interest: Some((0.5, 0.5)),
            white_balance_mode: WhiteBalanceMode::Auto,
            white_balance_gains: None,
            zoom_factor: Some(1.0),
            frame_rate: Some(30.0),
            resolution: Some(Resolution{width: 1920, height: 1080}),
            hdr_enabled: false,
            stabilization_enabled: true,
            low_light_boost: Some(false),
            scene_mode_hint: Some(SceneMode::Standard),
            brightness: Some(0.5),
            contrast: Some(0.5),
            saturation: Some(0.5),
            sharpness: None,
            hue: Some(0.5),
            noise_reduction: None,
            gamma: Some(0.5),
            color_filter: None,
            is_updated: true,
        }
    }
}


impl CameraSettings {
    pub fn set_brightness(&mut self, value: f32) {
        let v = value.clamp(0.0, 1.0);
        self.brightness = Some(v);
        self.is_updated = true;
    }

    pub fn set_contrast(&mut self, value: f32) {
        let v = value.clamp(0.0, 1.0);
        self.contrast = Some(v);
        self.is_updated = true;
    }

    pub fn set_saturation(&mut self, value: f32) {
        let v = value.clamp(0.0, 1.0);
        self.saturation = Some(v);
        self.is_updated = true;
    }

    pub fn set_sharpness(&mut self, value: f32) {
        let v = value.clamp(0.0, 1.0);
        match v < 0.1 {
            true => self.sharpness = None,
            false => self.sharpness = Some(v)
        }
        self.is_updated = true;
    }

    pub fn set_hue(&mut self, value: f32) {
        let v = value.clamp(0.0, 1.0);
        self.hue = Some(v);
        self.is_updated = true;
    }

    pub fn set_noise_reduction(&mut self, value: f32) {
        let v = value.clamp(0.0, 1.0);
        match v < 0.1 {
            true => self.noise_reduction = None,
            false => self.noise_reduction = Some(v)
        }
        self.is_updated = true;
    }

    pub fn set_gamma(&mut self, value: f32) {
        let v = value.clamp(0.0, 1.0);
        self.gamma = Some(v);
        self.is_updated = true;
    }

    pub fn set_focus_mode(&mut self, mode: FocusMode) {
        if mode != FocusMode::Manual { self.focus_distance = Some(0.5) };
        self.focus_mode = mode;
        self.is_updated = true;
    }

    pub fn set_focus_distance(&mut self, value: f32) {
        if self.focus_mode == FocusMode::Manual {
            let v = value.clamp(0.0, 1.0);
            self.focus_distance = Some(v);
            self.is_updated = true;
        }
    }

    pub fn set_exposure_compensation(&mut self, value: f32) {
        let ev = (value.clamp(0.0, 1.0) * 4.0) - 2.0;
        self.exposure_compensation = Some(ev);
        self.is_updated = true;
    }

    pub fn set_custom_exposure(&mut self, duration_percentage: f32, iso_percentage: f32) {
        self.custom_exposure = Some(CustomExposure { 
            duration: duration_percentage.clamp(0.0, 1.0), 
            iso: iso_percentage.clamp(0.0, 1.0),
        });
        self.exposure_mode = ExposureMode::Custom;
        self.is_updated = true;
    }

    pub fn set_exposure_mode(&mut self, mode: ExposureMode) {
        if mode != ExposureMode::Custom { self.custom_exposure = None };
        self.exposure_mode = mode;
        self.is_updated = true;
    }

    pub fn set_white_balance_mode(&mut self, mode: WhiteBalanceMode) {
        if mode != WhiteBalanceMode::Custom { self.white_balance_gains = None };
        self.white_balance_mode = mode;
        self.is_updated = true;
    }

    pub fn set_white_balance_gains_red(&mut self, red: f32) {
        let g = self.white_balance_gains.unwrap_or_default();
        let gains = WhiteBalanceGains::from(red.clamp(0.0, 1.0), g.green, g.blue);
        self.white_balance_gains = Some(gains);
        self.white_balance_mode = WhiteBalanceMode::Custom;
        self.is_updated = true;
    }

    pub fn set_white_balance_gains_green(&mut self, green: f32) {
        let g = self.white_balance_gains.unwrap_or_default();
        let gains = WhiteBalanceGains::from(g.red, green.clamp(0.0, 1.0), g.blue);
        self.white_balance_gains = Some(gains);
        self.white_balance_mode = WhiteBalanceMode::Custom;
        self.is_updated = true;
    }

    pub fn set_white_balance_gains_blue(&mut self, blue: f32) {
        let g = self.white_balance_gains.unwrap_or_default();
        let gains = WhiteBalanceGains::from(g.red, g.green, blue.clamp(0.0, 1.0));
        self.white_balance_gains = Some(gains);
        self.white_balance_mode = WhiteBalanceMode::Custom;
        self.is_updated = true;
    }

    pub fn set_zoom_factor(&mut self, value: f32) {
        let zoom = 1.0 + value.clamp(0.0, 1.0) * 9.0;
        self.zoom_factor = Some(zoom);
        self.is_updated = true;
    }

    pub fn set_hdr_enabled(&mut self, enabled: bool) {
        self.hdr_enabled = enabled;
        self.is_updated = true;
    }

    pub fn set_stabilization_enabled(&mut self, enabled: bool) {
        self.stabilization_enabled = enabled;
        self.is_updated = true;
    }

    pub fn set_low_light_boost(&mut self, enabled: bool) {
        self.low_light_boost = Some(enabled);
        self.is_updated = true;
    }

    pub fn set_scene_mode(&mut self, scene: SceneMode) {
        self.scene_mode_hint = Some(scene);
        self.is_updated = true;
    }

    pub fn set_focus_point_of_interest(&mut self, x: f32, y: f32) {
        let clamped = (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0));
        self.focus_point_of_interest = Some(clamped);
        self.is_updated = true;
    }
}
