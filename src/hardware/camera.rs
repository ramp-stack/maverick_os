use image::RgbaImage;
use crate::CameraError;

#[cfg(any(target_os = "ios", target_os = "macos"))]
mod apple;

#[cfg(any(target_os = "ios", target_os = "macos"))]
mod apple_custom_image;

#[cfg(any(target_os = "ios", target_os = "macos"))]
mod apple_custom_utils;

#[cfg(target_os = "android")]
mod android;

// #[cfg(any(target_os = "windows", target_os = "linux"))]
// mod windows_linux;

#[cfg(any(target_os = "macos", target_os = "ios"))]
use crate::hardware::camera::apple::AppleCamera;

#[cfg(any(target_os = "macos", target_os = "ios"))]
use crate::hardware::camera::apple_custom_image::AppleCustomCamera;

#[cfg(target_os = "android")]
use crate::hardware::camera::android::AndroidCamera;

// #[cfg(any(target_os = "windows", target_os = "linux"))]
// use crate::hardware::camera::windows_linux::WindowsLinuxCamera;

#[derive(Debug, Clone)]
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub enum AppleCameraBackend {
    Standard(AppleCamera),
    Custom(AppleCustomCamera),
}

/// Access the device camera.
#[derive(Debug, Clone)]
pub struct Camera(
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    AppleCameraBackend,
    
    #[cfg(target_os = "android")]
    AndroidCamera,

    // #[cfg(any(target_os = "windows", target_os = "linux"))]
    // WindowsLinuxCamera,
);

impl Camera {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn new() -> Self {
        let camera = AppleCamera::new();
        camera.open_camera();
        Camera(AppleCameraBackend::Standard(camera))
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn new_unprocessed() -> Self {
        let mut camera = AppleCustomCamera::new();
        camera.open_camera().unwrap_or_else(|_e| {
            panic!("Failed to open camera")
        });
        Camera(AppleCameraBackend::Custom(camera))
    }

    #[cfg(target_os = "android")]
    pub fn new() -> Self {
        let mut camera = AndroidCamera::new().unwrap_or_else(|_| panic!("Access denied to camera"));
        camera.open_camera();
        Camera(camera)
    }

    // Add the missing Android implementation
    #[cfg(target_os = "android")]
    pub fn new_unprocessed() -> Self {
        Self::new()
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    pub fn new() -> Self {
        // let mut camera = WindowsLinuxCamera::new(0); // Default to first camera
        // camera.start();
        // Camera(camera)
        panic!("Windows and linux not currently supported");
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    pub fn new_with_index(index: usize) -> Self {
        // let mut camera = WindowsLinuxCamera::new(index);
        // camera.start();
        // Camera(camera)
        panic!("Windows and linux not currently supported");
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    pub fn new_unprocessed() -> Self {
        // Self::new()
        panic!("Windows and linux not currently supported");
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android", target_os = "windows", target_os = "linux")))]
    pub fn new() -> Self {
        panic!("Camera access denied on this platform")
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android", target_os = "windows", target_os = "linux")))]
    pub fn new_unprocessed() -> Self {
        Self::new()
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn get_frame(&mut self) -> Option<RgbaImage> {
        match &mut self.0 {
            AppleCameraBackend::Standard(_cam) => None,
            AppleCameraBackend::Custom(cam) => cam.get_latest_raw_frame(),
        }
    }

    #[cfg(target_os = "android")]
    pub fn get_frame(&mut self) -> (Vec<u8>, usize, usize) {
        let image = self.0.get_latest_frame().unwrap_or_else(|_| panic!("Failed to get frame"));
        let (width, height) = image.dimensions();
        let pixels = image.into_raw();
        (pixels, width as usize, height as usize)
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    pub fn get_frame(&mut self) -> Option<RgbaImage> {
        self.0.capture_frame()
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android", target_os = "windows", target_os = "linux")))]
    pub fn get_frame(&mut self) -> Option<RgbaImage> {
        None
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn get_latest_raw_frame(&self) -> Option<RgbaImage> {
        match &self.0 {
            AppleCameraBackend::Standard(_cam) => None,
            AppleCameraBackend::Custom(cam) => cam.get_latest_raw_frame(),
        }
    }

    #[cfg(any(target_os = "android", target_os = "windows", target_os = "linux"))]
    pub fn get_latest_raw_frame(&self) -> Option<RgbaImage> {
        None
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android", target_os = "windows", target_os = "linux")))]
    pub fn get_latest_raw_frame(&self) -> Option<RgbaImage> {
        None
    }

    // Image processing settings - only supported on Apple platforms for now
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn update_settings<F>(&self, update_fn: F)
    where F: FnOnce(&mut ImageSettings),
    {
        match &self.0 {
            AppleCameraBackend::Standard(_cam) => {
                panic!("Standard camera doesn't support settings updates")
            }
            AppleCameraBackend::Custom(cam) => {
                cam.update_settings(update_fn);
            }
        }
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn set_exposure_and_iso(&mut self, duration: f32, iso: f32) -> Result<(), CameraError> {
        match &self.0 {
            AppleCameraBackend::Standard(_cam) => {
                Err(CameraError::FailedToGetFrame)
            }
            AppleCameraBackend::Custom(cam) => {
                // match duration_iso {
                //     Some((d, i)) => cam.set_exposure_and_iso(d, i),
                //     None => cam.disable_custom_exposure()
                // }.map_err(|e| CameraError::Error(e))
                cam.set_exposure_and_iso(duration, iso).map_err(CameraError::Unknown)
            }
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    pub fn set_exposure_and_iso(&mut self, duration: f32, iso: f32) -> Result<(), CameraError> {
        Err(CameraError::FailedToGetFrame)
    }

    // Individual setter methods for all image processing parameters
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn get_settings(&self) -> ImageSettings {
        match &self.0 {
            AppleCameraBackend::Standard(_cam) => {
                panic!("Standard camera doesn't support getting settings")
            }
            AppleCameraBackend::Custom(cam) => cam.get_settings(),
        }
    }

    // Individual setter methods for image processing parameters
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn set_brightness(&mut self, brightness: i16) {
        match &self.0 {
            AppleCameraBackend::Standard(_cam) => {
                panic!("Standard camera doesn't support brightness adjustment")
            }
            AppleCameraBackend::Custom(cam) => {
                cam.update_settings(|settings| {
                    settings.brightness = brightness;
                });
            }
        }
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn set_contrast(&mut self, contrast: f32) {
        match &self.0 {
            AppleCameraBackend::Standard(_cam) => {
                panic!("Standard camera doesn't support contrast adjustment")
            }
            AppleCameraBackend::Custom(cam) => {
                cam.update_settings(|settings| {
                    settings.contrast = contrast;
                });
            }
        }
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn set_saturation(&mut self, saturation: f32) {
        match &self.0 {
            AppleCameraBackend::Standard(_cam) => {
                panic!("Standard camera doesn't support saturation adjustment")
            }
            AppleCameraBackend::Custom(cam) => {
                cam.update_settings(|settings| {
                    settings.saturation = saturation;
                });
            }
        }
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn set_gamma(&mut self, gamma: f32) {
        match &self.0 {
            AppleCameraBackend::Standard(_cam) => {
                panic!("Standard camera doesn't support gamma adjustment")
            }
            AppleCameraBackend::Custom(cam) => {
                cam.update_settings(|settings| {
                    settings.gamma = gamma;
                });
            }
        }
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn set_exposure(&mut self, exposure: f32) {
        match &self.0 {
            AppleCameraBackend::Standard(_cam) => {
                panic!("Standard camera doesn't support exposure adjustment")
            }
            AppleCameraBackend::Custom(cam) => {
                cam.update_settings(|settings| {
                    settings.exposure = exposure;
                });
            }
        }
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn set_temperature(&mut self, temperature: f32) {
        match &self.0 {
            AppleCameraBackend::Standard(_cam) => {
                panic!("Standard camera doesn't support temperature adjustment")
            }
            AppleCameraBackend::Custom(cam) => {
                cam.update_settings(|settings| {
                    settings.temperature = temperature;
                });
            }
        }
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn set_white_balance_rgb(&mut self, r: f32, g: f32, b: f32) {
        match &self.0 {
            AppleCameraBackend::Standard(_cam) => {
                panic!("Standard camera doesn't support white balance adjustment")
            }
            AppleCameraBackend::Custom(cam) => {
                cam.update_settings(|settings| {
                    settings.white_balance_r = r;
                    settings.white_balance_g = g;
                    settings.white_balance_b = b;
                });
            }
        }
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn reset_settings(&mut self) {
        match &self.0 {
            AppleCameraBackend::Standard(_cam) => {
                panic!("Standard camera doesn't support settings reset")
            }
            AppleCameraBackend::Custom(cam) => {
                cam.update_settings(|settings| {
                    *settings = ImageSettings::default();
                });
            }
        }
    }

    // Stub implementations for platforms that don't support image processing
    #[cfg(any(target_os = "android", target_os = "windows", target_os = "linux"))]
    pub fn update_settings<F>(&self, _update_fn: F)
    where F: FnOnce(&mut ImageSettings),
    {
        panic!("Camera settings update not supported on this platform")
    }

    #[cfg(any(target_os = "android", target_os = "windows", target_os = "linux"))]
    pub fn get_settings(&self) -> ImageSettings {
        panic!("Camera settings retrieval not supported on this platform")
    }

    #[cfg(any(target_os = "android", target_os = "windows", target_os = "linux"))]
    pub fn set_brightness(&mut self, _brightness: i16) {
        panic!("Camera brightness adjustment not supported on this platform")
    }

    #[cfg(any(target_os = "android", target_os = "windows", target_os = "linux"))]
    pub fn set_contrast(&mut self, _contrast: f32) {
        panic!("Camera contrast adjustment not supported on this platform")
    }

    #[cfg(any(target_os = "android", target_os = "windows", target_os = "linux"))]
    pub fn set_saturation(&mut self, _saturation: f32) {
        panic!("Camera saturation adjustment not supported on this platform")
    }

    #[cfg(any(target_os = "android", target_os = "windows", target_os = "linux"))]
    pub fn set_gamma(&mut self, _gamma: f32) {
        panic!("Camera gamma adjustment not supported on this platform")
    }

    #[cfg(any(target_os = "android", target_os = "windows", target_os = "linux"))]
    pub fn set_exposure(&mut self, _exposure: f32) {
        panic!("Camera exposure adjustment not supported on this platform")
    }

    #[cfg(any(target_os = "android", target_os = "windows", target_os = "linux"))]
    pub fn set_temperature(&mut self, _temperature: f32) {
        panic!("Camera temperature adjustment not supported on this platform")
    }

    #[cfg(any(target_os = "android", target_os = "windows", target_os = "linux"))]
    pub fn set_white_balance_rgb(&mut self, _r: f32, _g: f32, _b: f32) {
        panic!("Camera white balance adjustment not supported on this platform")
    }

    #[cfg(any(target_os = "android", target_os = "windows", target_os = "linux"))]
    pub fn reset_settings(&mut self) {
        panic!("Camera settings reset not supported on this platform")
    }

    // Stub implementations for unsupported platforms
    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android", target_os = "windows", target_os = "linux")))]
    pub fn update_settings<F>(&self, _update_fn: F)
    where F: FnOnce(&mut ImageSettings),
    {
        panic!("Camera access denied on this platform")
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android", target_os = "windows", target_os = "linux")))]
    pub fn get_settings(&self) -> ImageSettings {
        panic!("Camera access denied on this platform")
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android", target_os = "windows", target_os = "linux")))]
    pub fn set_brightness(&mut self, _brightness: i16) {
        panic!("Camera access denied on this platform")
    }

    // Static convenience methods
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn open_and_get_frame() -> (Vec<u8>, usize, usize) {
        panic!("Failed to get frame from standard camera")
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn open_and_get_custom_frame() -> Option<RgbaImage> {
        let mut camera = AppleCustomCamera::new();
        if camera.open_camera().is_err() {
            return None;
        }

        let mut wrapper = Camera(AppleCameraBackend::Custom(camera));
        for _ in 1..=10 {
            std::thread::sleep(std::time::Duration::from_millis(200));
            if let Some(frame) = wrapper.get_frame() {
                return Some(frame);
            }
        }
        None
    }

    #[cfg(target_os = "android")]
    pub fn open_and_get_frame() -> (Vec<u8>, usize, usize) {
        let mut camera = AndroidCamera::new().unwrap_or_else(|_| panic!("Access denied to camera"));
        camera.open_camera();
        let mut wrapper = Camera(camera);
        wrapper.get_frame()
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    pub fn open_and_get_frame() -> Option<RgbaImage> {
        // let mut camera = WindowsLinuxCamera::new(0);
        // camera.start();
        // let mut wrapper = Camera(camera);
        // wrapper.get_frame()
        panic!("Windows and linux not currently supported");
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android", target_os = "windows", target_os = "linux")))]
    pub fn open_and_get_frame() -> Option<RgbaImage> {
        None
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn stop_camera(&self) {
        match &self.0 {
            AppleCameraBackend::Standard(_cam) => {}
            AppleCameraBackend::Custom(cam) => {
                cam.stop_camera();
            }
        }
    }

    #[cfg(any(target_os = "android", target_os = "windows", target_os = "linux"))]
    pub fn stop_camera(&self) {
        // Camera cleanup handled by Drop trait
    }
}

impl Default for Camera {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Camera {
    fn drop(&mut self) {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        self.stop_camera();
        
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        {
            let _ = self.0.stop_stream();
        }
    }
}

/// Settings for configuring camera behavior.
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
    pub exposure_iso: f32,
    pub exposure_duration: f32,
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
            exposure_iso: 0.0,
            exposure_duration: 0.0
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

    pub fn temperature_to_rgb_multipliers(&self) -> [f32; 3] {
        let temp = self.temperature;
        let temp_scaled = temp / 100.0;

        if temp < 6600.0 {
            let r = 1.0;
            let g = (0.39008157 * temp_scaled.ln() - 0.631_841_4).clamp(0.0, 1.0);
            let b = if temp < 2000.0 {
                0.0
            } else {
                (0.54320678 * (temp_scaled - 10.0).ln() - 1.196_254_1).clamp(0.0, 1.0)
            };
            [r, g, b]
        } else {
            let r = (1.292_936_2 * (temp_scaled - 60.0).powf(-0.1332047)).clamp(0.0, 1.0);
            let g = (1.129_890_9 * (temp_scaled - 60.0).powf(-0.0755148)).clamp(0.0, 1.0);
            let b = 1.0;
            [r, g, b]
        }
    }
}