use image::RgbaImage;

#[cfg(any(target_os = "ios", target_os = "macos"))]
mod apple;

#[cfg(any(target_os = "ios", target_os = "macos"))]
mod apple_custom_image;

#[cfg(any(target_os = "ios", target_os = "macos"))]
mod apple_custom_utils;

#[cfg(target_os = "android")]
mod android;

#[cfg(any(target_os = "macos", target_os = "ios"))]
use crate::hardware::camera::apple::AppleCamera;

#[cfg(any(target_os = "macos", target_os = "ios"))]
use crate::hardware::camera::apple_custom_image::AppleCustomCamera;
pub use crate::hardware::camera::apple_custom_utils::ImageSettings;

#[cfg(target_os = "android")]
use crate::hardware::camera::android::AndroidCamera;

#[derive(Debug)]
pub enum CameraError {
    AccessDenied,
    WaitingForAccess,
    FailedToGetFrame,
    FailedToOpenCamera,
}

#[derive(Debug, Clone)]
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub enum AppleCameraBackend {
    Standard(AppleCamera),
    Custom(AppleCustomCamera),
}

#[derive(Debug, Clone)]
pub struct Camera(
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    AppleCameraBackend,
    
    #[cfg(target_os = "android")]
    AndroidCamera,
);

impl Camera {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn new() -> Result<Self, CameraError> {
        // println!("Creating standard Apple camera");
        let camera = AppleCamera::new();
        camera.open_camera();
        Ok(Camera(AppleCameraBackend::Standard(camera)))
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn new_custom() -> Result<Self, CameraError> {
        // println!("Creating custom Apple camera");
        let mut camera = AppleCustomCamera::new();
        camera.open_camera().map_err(|_e| {
            // println!("Failed to open custom camera: {}", e);
            CameraError::FailedToOpenCamera
        })?;
        // println!("Custom camera opened successfully");
        Ok(Camera(AppleCameraBackend::Custom(camera)))
    }

    #[cfg(target_os = "android")]
    pub fn new() -> Result<Self, CameraError> {
        let mut camera = AndroidCamera::new().map_err(|_| CameraError::AccessDenied)?;
        camera.open_camera();
        Ok(Camera(camera))
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
    pub fn new() -> Result<Self, CameraError> {
        Err(CameraError::AccessDenied)
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn get_frame(&mut self) -> Option<RgbaImage> {
        match &mut self.0 {
            AppleCameraBackend::Standard(_cam) => {
                // println!("Standard camera not supported for frame output");
                None
            }
            AppleCameraBackend::Custom(cam) => {
                // println!("Getting frame from custom camera");
                cam.get_latest_raw_frame()
            }
        }
    }

    #[cfg(target_os = "android")]
    pub fn get_frame(&mut self) -> Result<(Vec<u8>, usize, usize), CameraError> {
        self.0.get_latest_frame().map_err(|_| CameraError::FailedToGetFrame)
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
    pub fn get_frame(&mut self) -> Option<RgbaImage> {
        None
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn get_latest_raw_frame(&self) -> Option<RgbaImage> {
        match &self.0 {
            AppleCameraBackend::Standard(_cam) => {
                None
            }
            AppleCameraBackend::Custom(cam) => {
                cam.get_latest_raw_frame()
            }
        }
    }

    #[cfg(target_os = "android")]
    pub fn get_latest_raw_frame(&self) -> Option<RgbaImage> {
        None
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
    pub fn get_latest_raw_frame(&self) -> Option<RgbaImage> {
        None
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn update_settings<F>(&self, update_fn: F) -> Result<(), CameraError>
    where F: FnOnce(&mut ImageSettings),
    {
        match &self.0 {
            AppleCameraBackend::Standard(_cam) => {
                // Standard camera doesn't support settings updates
                Err(CameraError::FailedToGetFrame) // Reusing error type, could add a new one
            }
            AppleCameraBackend::Custom(cam) => {
                cam.update_settings(update_fn);
                Ok(())
            }
        }
    }

    // Individual setter methods for all image processing parameters
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn set_brightness(&mut self, brightness: i16) -> Result<(), CameraError> {
        match &self.0 {
            AppleCameraBackend::Standard(_cam) => {
                Err(CameraError::FailedToGetFrame)
            }
            AppleCameraBackend::Custom(cam) => {
                cam.update_settings(|settings| {
                    settings.brightness = brightness;
                });
                Ok(())
            }
        }
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn set_contrast(&mut self, contrast: f32) -> Result<(), CameraError> {
        match &self.0 {
            AppleCameraBackend::Standard(_cam) => {
                Err(CameraError::FailedToGetFrame)
            }
            AppleCameraBackend::Custom(cam) => {
                cam.update_settings(|settings| {
                    settings.contrast = contrast;
                });
                Ok(())
            }
        }
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn set_saturation(&mut self, saturation: f32) -> Result<(), CameraError> {
        match &self.0 {
            AppleCameraBackend::Standard(_cam) => {
                Err(CameraError::FailedToGetFrame)
            }
            AppleCameraBackend::Custom(cam) => {
                cam.update_settings(|settings| {
                    settings.saturation = saturation;
                });
                Ok(())
            }
        }
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn set_gamma(&mut self, gamma: f32) -> Result<(), CameraError> {
        match &self.0 {
            AppleCameraBackend::Standard(_cam) => {
                Err(CameraError::FailedToGetFrame)
            }
            AppleCameraBackend::Custom(cam) => {
                cam.update_settings(|settings| {
                    settings.gamma = gamma;
                });
                Ok(())
            }
        }
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn set_exposure(&mut self, exposure: f32) -> Result<(), CameraError> {
        match &self.0 {
            AppleCameraBackend::Standard(_cam) => {
                Err(CameraError::FailedToGetFrame)
            }
            AppleCameraBackend::Custom(cam) => {
                cam.update_settings(|settings| {
                    settings.exposure = exposure;
                });
                Ok(())
            }
        }
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn set_temperature(&mut self, temperature: f32) -> Result<(), CameraError> {
        match &self.0 {
            AppleCameraBackend::Standard(_cam) => {
                Err(CameraError::FailedToGetFrame)
            }
            AppleCameraBackend::Custom(cam) => {
                cam.update_settings(|settings| {
                    settings.temperature = temperature;
                });
                Ok(())
            }
        }
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn set_white_balance_r(&mut self, white_balance_r: f32) -> Result<(), CameraError> {
        match &self.0 {
            AppleCameraBackend::Standard(_cam) => {
                Err(CameraError::FailedToGetFrame)
            }
            AppleCameraBackend::Custom(cam) => {
                cam.update_settings(|settings| {
                    settings.white_balance_r = white_balance_r;
                });
                Ok(())
            }
        }
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn set_white_balance_g(&mut self, white_balance_g: f32) -> Result<(), CameraError> {
        match &self.0 {
            AppleCameraBackend::Standard(_cam) => {
                Err(CameraError::FailedToGetFrame)
            }
            AppleCameraBackend::Custom(cam) => {
                cam.update_settings(|settings| {
                    settings.white_balance_g = white_balance_g;
                });
                Ok(())
            }
        }
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn set_white_balance_b(&mut self, white_balance_b: f32) -> Result<(), CameraError> {
        match &self.0 {
            AppleCameraBackend::Standard(_cam) => {
                Err(CameraError::FailedToGetFrame)
            }
            AppleCameraBackend::Custom(cam) => {
                cam.update_settings(|settings| {
                    settings.white_balance_b = white_balance_b;
                });
                Ok(())
            }
        }
    }

    /// Set white balance using RGB multipliers all at once
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn set_white_balance_rgb(&mut self, r: f32, g: f32, b: f32) -> Result<(), CameraError> {
        match &self.0 {
            AppleCameraBackend::Standard(_cam) => {
                Err(CameraError::FailedToGetFrame)
            }
            AppleCameraBackend::Custom(cam) => {
                cam.update_settings(|settings| {
                    settings.white_balance_r = r;
                    settings.white_balance_g = g;
                    settings.white_balance_b = b;
                });
                Ok(())
            }
        }
    }

    /// Reset all settings to default values
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn reset_settings(&mut self) -> Result<(), CameraError> {
        match &self.0 {
            AppleCameraBackend::Standard(_cam) => {
                Err(CameraError::FailedToGetFrame)
            }
            AppleCameraBackend::Custom(cam) => {
                cam.update_settings(|settings| {
                    *settings = ImageSettings::default();
                });
                Ok(())
            }
        }
    }

    // Stub implementations for Android
    #[cfg(target_os = "android")]
    pub fn set_brightness(&mut self, _brightness: i16) -> Result<(), CameraError> {
        Err(CameraError::FailedToGetFrame)
    }

    #[cfg(target_os = "android")]
    pub fn set_contrast(&mut self, _contrast: f32) -> Result<(), CameraError> {
        Err(CameraError::FailedToGetFrame)
    }

    #[cfg(target_os = "android")]
    pub fn set_saturation(&mut self, _saturation: f32) -> Result<(), CameraError> {
        Err(CameraError::FailedToGetFrame)
    }

    #[cfg(target_os = "android")]
    pub fn set_gamma(&mut self, _gamma: f32) -> Result<(), CameraError> {
        Err(CameraError::FailedToGetFrame)
    }

    #[cfg(target_os = "android")]
    pub fn set_exposure(&mut self, _exposure: f32) -> Result<(), CameraError> {
        Err(CameraError::FailedToGetFrame)
    }

    #[cfg(target_os = "android")]
    pub fn set_temperature(&mut self, _temperature: f32) -> Result<(), CameraError> {
        Err(CameraError::FailedToGetFrame)
    }

    #[cfg(target_os = "android")]
    pub fn set_white_balance_r(&mut self, _white_balance_r: f32) -> Result<(), CameraError> {
        Err(CameraError::FailedToGetFrame)
    }

    #[cfg(target_os = "android")]
    pub fn set_white_balance_g(&mut self, _white_balance_g: f32) -> Result<(), CameraError> {
        Err(CameraError::FailedToGetFrame)
    }

    #[cfg(target_os = "android")]
    pub fn set_white_balance_b(&mut self, _white_balance_b: f32) -> Result<(), CameraError> {
        Err(CameraError::FailedToGetFrame)
    }

    #[cfg(target_os = "android")]
    pub fn set_white_balance_rgb(&mut self, _r: f32, _g: f32, _b: f32) -> Result<(), CameraError> {
        Err(CameraError::FailedToGetFrame)
    }

    #[cfg(target_os = "android")]
    pub fn reset_settings(&mut self) -> Result<(), CameraError> {
        Err(CameraError::FailedToGetFrame)
    }

    // Stub implementations for unsupported platforms
    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
    pub fn set_brightness(&mut self, _brightness: i16) -> Result<(), CameraError> {
        Err(CameraError::AccessDenied)
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
    pub fn set_contrast(&mut self, _contrast: f32) -> Result<(), CameraError> {
        Err(CameraError::AccessDenied)
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
    pub fn set_saturation(&mut self, _saturation: f32) -> Result<(), CameraError> {
        Err(CameraError::AccessDenied)
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
    pub fn set_gamma(&mut self, _gamma: f32) -> Result<(), CameraError> {
        Err(CameraError::AccessDenied)
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
    pub fn set_exposure(&mut self, _exposure: f32) -> Result<(), CameraError> {
        Err(CameraError::AccessDenied)
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
    pub fn set_temperature(&mut self, _temperature: f32) -> Result<(), CameraError> {
        Err(CameraError::AccessDenied)
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
    pub fn set_white_balance_r(&mut self, _white_balance_r: f32) -> Result<(), CameraError> {
        Err(CameraError::AccessDenied)
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
    pub fn set_white_balance_g(&mut self, _white_balance_g: f32) -> Result<(), CameraError> {
        Err(CameraError::AccessDenied)
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
    pub fn set_white_balance_b(&mut self, _white_balance_b: f32) -> Result<(), CameraError> {
        Err(CameraError::AccessDenied)
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
    pub fn set_white_balance_rgb(&mut self, _r: f32, _g: f32, _b: f32) -> Result<(), CameraError> {
        Err(CameraError::AccessDenied)
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
    pub fn reset_settings(&mut self) -> Result<(), CameraError> {
        Err(CameraError::AccessDenied)
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn get_settings(&self) -> Result<ImageSettings, CameraError> {
        match &self.0 {
            AppleCameraBackend::Standard(_cam) => {
                Err(CameraError::FailedToGetFrame)
            }
            AppleCameraBackend::Custom(cam) => {
                Ok(cam.get_settings())
            }
        }
    }

    #[cfg(target_os = "android")]
    pub fn update_settings<F>(&self, _update_fn: F) -> Result<(), CameraError>
    where F: FnOnce(&mut ImageSettings),
    {
        Err(CameraError::FailedToGetFrame)
    }

    #[cfg(target_os = "android")]
    pub fn get_settings(&self) -> Result<ImageSettings, CameraError> {
        Err(CameraError::FailedToGetFrame)
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
    pub fn update_settings<F>(&self, _update_fn: F) -> Result<(), CameraError>
    where F: FnOnce(&mut ImageSettings),
    {
        Err(CameraError::AccessDenied)
    }

    /// Get current camera image processing settings
    /// Not available for unsupported platforms
    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
    pub fn get_settings(&self) -> Result<ImageSettings, CameraError> {
        Err(CameraError::AccessDenied)
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn open_and_get_frame() -> Result<(Vec<u8>, usize, usize), CameraError> {
        println!("Opening standard camera and getting frame (not supported)");
        Err(CameraError::FailedToGetFrame)
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn open_and_get_custom_frame() -> Option<RgbaImage> {
        // println!("Opening custom camera and getting frame");
        let mut camera = AppleCustomCamera::new();
        if let Err(_e) = camera.open_camera() {
            // println!("Failed to open custom camera: {}", e);
            return None;
        }

        let mut wrapper = Camera(AppleCameraBackend::Custom(camera));
        // println!("Waiting for custom camera to capture first frame...");
        for attempt in 1..=10 {
            std::thread::sleep(std::time::Duration::from_millis(200));
            // println!("Attempt {} to get frame", attempt);
            if let Some(frame) = wrapper.get_frame() {
                // println!("Successfully got frame on attempt {}", attempt);
                return Some(frame);
            }
        }
        // println!("Failed to get frame after 10 attempts");
        None
    }

    #[cfg(target_os = "android")]
    pub fn open_and_get_frame() -> Result<(Vec<u8>, usize, usize), CameraError> {
        let mut camera = AndroidCamera::new().map_err(|_| CameraError::AccessDenied)?;
        camera.open_camera();
        let mut wrapper = Camera(camera);
        wrapper.get_frame()
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
    pub fn open_and_get_frame() -> Option<RgbaImage> {
        None
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn stop_camera(&self) {
        match &self.0 {
            AppleCameraBackend::Standard(_cam) => {
                // println!("Stopping standard camera");
            }
            AppleCameraBackend::Custom(cam) => {
                // println!("Stopping custom camera");
                cam.stop_camera();
            }
        }
    }
}

impl Default for Camera {
    fn default() -> Self {
        Self::new().unwrap_or_else(|_| panic!("Failed to create default camera"))
    }
}

impl Drop for Camera {
    fn drop(&mut self) {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        self.stop_camera();
    }
}