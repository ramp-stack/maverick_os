use image::RgbaImage;

#[cfg(any(target_os = "ios", target_os = "macos"))]
mod apple;

#[cfg(any(target_os = "ios", target_os = "macos"))]
use crate::hardware::camera::apple::*;

#[cfg(target_os = "android")]
mod android;

#[cfg(target_os = "android")]
use crate::hardware::camera::android::*;

#[derive(Debug)]
pub enum CameraError {
    AccessDenied,
    WaitingForAccess,
    FailedToGetFrame
}

#[derive(Debug, Clone)]
pub struct Camera (
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    AppleCamera,
    #[cfg(target_os = "android")]
    AndroidCamera,
);

impl Camera {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn new() -> Self {
        // #[cfg(target_os = "ios")]
        // start_camera_apple();

        let camera = AppleCamera::new();
        camera.open_camera();
        Camera(camera)
    }

    #[cfg(target_os = "android")]
    pub fn new() -> Self {
        let mut camera = AndroidCamera::new().expect("Failed to create Android camera");
        camera.open_camera();
        return Camera(camera)
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
    pub fn new() -> Self {
        todo!("Camera not supported on this platform")
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn get_frame(&mut self) -> Result<RgbaImage, CameraError> {
        self.0.get_latest_frame().ok_or(CameraError::FailedToGetFrame)
    }

    #[cfg(target_os = "android")]
    pub fn get_frame(&mut self) -> Result<RgbaImage, CameraError> {
        #[cfg(target_os = "android")]
        return self.0.get_latest_frame().map_err(|_| CameraError::FailedToGetFrame);

        Err(CameraError::FailedToGetFrame)
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
    pub fn get_frame(&mut self) -> Result<RgbaImage, CameraError> {
        todo!("Camera not supported on this platform")
    }

    /// Opens the camera and immediately gets a frame
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn open_and_get_frame() -> Result<RgbaImage, CameraError> {
        let camera = AppleCamera::new();
        camera.open_camera();
        let mut camera_wrapper = Camera(camera);
        camera_wrapper.get_frame()
    }

    /// Opens the camera and immediately gets a frame
    #[cfg(target_os = "android")]
    pub fn open_and_get_frame() -> Result<RgbaImage, CameraError> {
        let mut camera = AndroidCamera::new().map_err(|_| CameraError::AccessDenied)?;
        camera.open_camera();
        let mut camera_wrapper = Camera(camera);
        camera_wrapper.get_frame()
    }

    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
    pub fn open_and_get_frame() -> Result<RgbaImage, CameraError> {
        todo!("Camera not supported on this platform")
    }
}

impl Default for Camera {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Camera {
    fn drop(&mut self) {
        println!("Stopping Camera");
    }
}