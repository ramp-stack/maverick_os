use std::sync::Arc;
use std::rc::Rc;
use std::ffi::CStr;
use std::path::PathBuf;

use arc_swap::ArcSwap;
use image::{ImageBuffer, RgbaImage};

use dispatch2::{DispatchQueue, DispatchQueueAttr};

use objc2::AnyThread;
use objc2::rc::Retained;
use objc2::runtime::{ProtocolObject, AnyObject, AnyClass};
use objc2::{define_class, msg_send, DefinedClass};
use objc2_av_foundation::{
    AVCaptureConnection, AVCaptureDevice, AVCaptureDeviceInput, AVCaptureDevicePosition,
    AVCaptureOutput, AVCaptureSession, AVCaptureVideoDataOutput,
    AVMediaTypeVideo, AVCaptureVideoDataOutputSampleBufferDelegate,
    AVCaptureSessionPresetHigh, AVCaptureDeviceRotationCoordinator
};
use objc2_core_media::CMSampleBuffer;
use objc2_core_video::{
    kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange, kCVPixelBufferPixelFormatTypeKey,
    CVPixelBuffer, CVPixelBufferLockFlags, CVPixelBufferGetBaseAddressOfPlane,
    CVPixelBufferGetBytesPerRowOfPlane, CVPixelBufferGetHeight, CVPixelBufferGetWidth,
    CVPixelBufferLockBaseAddress, CVPixelBufferUnlockBaseAddress,
};
use objc2_foundation::{NSDictionary, NSNumber, NSObject, NSObjectProtocol, NSString, NSURL};

const NS_APPLICATION_SUPPORT_DIRECTORY: u64 = 14;
const NS_USER_DOMAIN_MASK: u64 = 1;

pub struct Logger;
impl Logger {
    pub fn start (level: Option<log::Level>) {
        let level = level.unwrap_or(log::Level::Info);
        env_logger::builder().filter_level(level.to_level_filter()).init();
    }
}

pub struct ApplicationSupport;
impl ApplicationSupport {
    pub fn get() -> PathBuf {
        unsafe {
            let file_manager_class = AnyClass::get(c"NSFileManager").unwrap();
            let file_manager: *mut AnyObject = msg_send![file_manager_class, defaultManager];

            let mut error: *mut AnyObject = std::ptr::null_mut();
            let url: *mut NSURL = msg_send![
                file_manager,
                URLForDirectory: NS_APPLICATION_SUPPORT_DIRECTORY,
                inDomain: NS_USER_DOMAIN_MASK,
                appropriateForURL: std::ptr::null::<AnyObject>(),
                create: true,
                error: &mut error,
            ];

            if url.is_null() {panic!("Failed to get Documents directory URL");}

            let path_nsstring: *mut NSString = msg_send![url, path];
            if path_nsstring.is_null() {panic!("Failed to get path from Documents URL");}

            let c_str: *const std::os::raw::c_char = msg_send![path_nsstring, UTF8String];
            if c_str.is_null() {panic!("Failed to get UTF8 string from Documents path");}

            let path = CStr::from_ptr(c_str).to_string_lossy().into_owned();
            PathBuf::from(path)
        }
    }
}

pub type Frames = Arc<ArcSwap<RgbaImage>>;
pub type Session = Rc<Retained<AVCaptureSession>>;

impl CameraDelegate {
    fn new(frames: Frames) -> Retained<Self> {
        let this = Self::alloc().set_ivars(frames);
        unsafe { msg_send![super(this), init] }
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[ivars = Frames]
    struct CameraDelegate;

    unsafe impl NSObjectProtocol for CameraDelegate {}

    unsafe impl AVCaptureVideoDataOutputSampleBufferDelegate for CameraDelegate {
        #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
        fn capture_output_did_output_sample_buffer_from_connection(
            &self,
            _output: &AVCaptureOutput,
            sample_buffer: &CMSampleBuffer,
            _connection: &AVCaptureConnection,
        ) {
            // NV12 (420v) → RGBA. Tight, integer-only, stride-aware.
            // Video-range BT.601 coefficients (standard for camera 420v).
            let image_buffer = unsafe { sample_buffer.image_buffer() }.unwrap();
            let pb: &CVPixelBuffer = image_buffer.as_ref();

            let flags = CVPixelBufferLockFlags::ReadOnly;
            if unsafe { CVPixelBufferLockBaseAddress(pb, flags) } != 0 {
                return;
            }

            let width = CVPixelBufferGetWidth(pb) as u32;
            let height = CVPixelBufferGetHeight(pb) as u32;
            if width == 0 || height == 0 {
                unsafe { CVPixelBufferUnlockBaseAddress(pb, flags) };
                return;
            }

            let y_base = CVPixelBufferGetBaseAddressOfPlane(pb, 0);
            let uv_base = CVPixelBufferGetBaseAddressOfPlane(pb, 1);
            let y_stride = CVPixelBufferGetBytesPerRowOfPlane(pb, 0);
            let uv_stride = CVPixelBufferGetBytesPerRowOfPlane(pb, 1);

            if y_base.is_null() || uv_base.is_null() {
                unsafe { CVPixelBufferUnlockBaseAddress(pb, flags) };
                return;
            }

            let y_plane = unsafe { std::slice::from_raw_parts(y_base as *const u8, y_stride * height as usize) };
            let uv_plane = unsafe { std::slice::from_raw_parts(uv_base as *const u8, uv_stride * (height as usize / 2)) };

            let mut dst = vec![0u8; (width * height * 4) as usize];
            let w = width as usize;
            let h = height as usize;

            // Integer BT.601 limited-range conversion
            for y in 0..h {
                let y_row = y * y_stride;
                let uv_row = (y / 2) * uv_stride;
                let dst_row = y * w * 4;

                for x in 0..w {
                    let yi = y_row + x;
                    let uvi = uv_row + (x & !1); // UV is subsampled 2×

                    let y_val = y_plane[yi] as i32;
                    let u = uv_plane[uvi] as i32 - 128;
                    let v = uv_plane[uvi + 1] as i32 - 128;

                    let y_adj = (y_val - 16).max(0);

                    let r = ((298 * y_adj + 409 * v + 128) >> 8).clamp(0, 255) as u8;
                    let g = ((298 * y_adj - 100 * u - 208 * v + 128) >> 8).clamp(0, 255) as u8;
                    let b = ((298 * y_adj + 516 * u + 128) >> 8).clamp(0, 255) as u8;

                    let di = dst_row + x * 4;
                    dst[di] = r;
                    dst[di + 1] = g;
                    dst[di + 2] = b;
                    dst[di + 3] = 255;
                }
            }

            unsafe { CVPixelBufferUnlockBaseAddress(pb, flags) };
            if let Some(image) = ImageBuffer::from_raw(width, height, dst) {
                self.ivars().store(Arc::new(image));
            }
        }
    }
);

pub struct CameraHandle {
    camera: Camera,
    _delegate: Retained<CameraDelegate>,
    _rotation_coordinator: Retained<AVCaptureDeviceRotationCoordinator>,
}

impl CameraHandle {
    pub fn new() -> Self {
        let frames: Frames = Arc::new(ArcSwap::from_pointee(RgbaImage::new(1, 1)));
        let session = unsafe { AVCaptureSession::new() };

        let device = unsafe {
            AVCaptureDevice::defaultDeviceWithDeviceType_mediaType_position(
                objc2_av_foundation::AVCaptureDeviceTypeBuiltInWideAngleCamera,
                AVMediaTypeVideo,
                AVCaptureDevicePosition::Unspecified,
            )
        }.unwrap();

        let rotation_coordinator = unsafe {
            AVCaptureDeviceRotationCoordinator::initWithDevice_previewLayer(
                AVCaptureDeviceRotationCoordinator::alloc(),
                &device, None,
            )
        };

        let input = unsafe { AVCaptureDeviceInput::deviceInputWithDevice_error(&device).unwrap() };
        let output = unsafe { AVCaptureVideoDataOutput::new() };

        // Native biplanar YUV (video range). Far cheaper than BGRA.
        let key: &NSString = unsafe {
            &*(kCVPixelBufferPixelFormatTypeKey as *const objc2_core_foundation::CFString as *const NSString)
        };
        let val = NSNumber::new_u32(kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange);
        let settings = NSDictionary::from_slices::<NSString>(&[key], &[val.as_ref()]);
        unsafe { output.setVideoSettings(Some(&settings)) };

        let queue = DispatchQueue::new("camera.frame.queue", DispatchQueueAttr::SERIAL);
        let delegate = CameraDelegate::new(Arc::clone(&frames));
        let proto = ProtocolObject::from_ref(&*delegate);
        unsafe { output.setSampleBufferDelegate_queue(Some(proto), Some(&queue)) };

        unsafe {
            session.beginConfiguration();
            if session.canSetSessionPreset(AVCaptureSessionPresetHigh) {
                session.setSessionPreset(AVCaptureSessionPresetHigh);
            }
            if !session.canAddInput(&input) || !session.canAddOutput(&output) {
                session.commitConfiguration();
                panic!("Can't add Input || Can't add Output");
            }
            session.addInput(&input);
            session.addOutput(&output);

            if let Some(media_type) = AVMediaTypeVideo
            && let Some(conn) = output.connectionWithMediaType(media_type) {
                let angle = rotation_coordinator.videoRotationAngleForHorizonLevelCapture();
                if conn.isVideoRotationAngleSupported(angle) {
                    conn.setVideoRotationAngle(angle);
                }
            }
            

            session.commitConfiguration();
        }

        CameraHandle {
            camera: Camera{session: Rc::new(session), frames},
            _delegate: delegate,
            _rotation_coordinator: rotation_coordinator
        }
    }

    pub fn start(&self) -> Camera {
        if !unsafe {self.camera.session.isRunning()} {
            unsafe { self.camera.session.startRunning(); }
        }
        self.camera.clone()
    }
}

#[derive(Clone)]
pub struct Camera {
    session: Session,
    frames: Frames
}

impl Camera {
    pub fn frame(&self) -> Arc<RgbaImage> {
        self.frames.load_full()
    }
}

impl Drop for Camera {
    fn drop(&mut self) {
        unsafe {
            if self.session.isRunning() && Arc::strong_count(&self.frames) == 2 {
                self.session.stopRunning();
            }
        }
    }
}

#[link(name = "PhotosUI", kind = "framework")]
unsafe extern "C" {}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {}

#[link(name = "Metal", kind = "framework")]
unsafe extern "C" {}

#[link(name = "CoreVideo", kind = "framework")]
unsafe extern "C" {}

#[link(name = "CoreMedia", kind = "framework")]
unsafe extern "C" {}

#[link(name = "AVKit", kind = "framework")]
unsafe extern "C" {}

#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {}

#[link(name = "Security", kind = "framework")]
unsafe extern "C" {}

#[link(name = "QuartzCore", kind = "framework")]
unsafe extern "C" {}

#[link(name = "c++")]
unsafe extern "C" {}

#[link(name = "AudioToolbox", kind = "framework")]
unsafe extern "C" {}

#[link(name = "Foundation", kind = "framework")]
unsafe extern "C" {}
