#![allow(non_snake_case)]

use std::sync::Mutex;
use std::slice::from_raw_parts;

use dispatch2::DispatchQueue;
use image::{Rgba, RgbaImage};

use objc2::__framework_prelude::NSObject;
use objc2::rc::Retained;
use objc2::runtime::{NSObjectProtocol, ProtocolObject};
use objc2::{define_class, AllocAnyThread, DeclaredClass};
use objc2_foundation::{ NSArray, NSDictionary, NSNumber, NSString};
use objc2_core_media::CMSampleBuffer;

use objc2_av_foundation::{
    AVCaptureConnection,
    AVCaptureDeviceDiscoverySession,
    AVCaptureOutput,
    AVCaptureSession,
    AVCaptureSessionPresetMedium,
    AVCaptureVideoDataOutput,
    AVCaptureVideoDataOutputSampleBufferDelegate,
    AVMediaTypeVideo,
    AVCaptureDeviceInput,
    AVCaptureDevicePosition
};

use objc2_core_video::{
    kCVPixelFormatType_32BGRA,
    CVPixelBufferGetHeight,
    CVPixelBufferGetWidth,
    kCVPixelBufferPixelFormatTypeKey,
    CVPixelBufferGetBytesPerRow,
    CVPixelBufferGetBaseAddress,
    CVPixelBufferLockFlags,
};

#[derive(Debug)]
pub struct ProcessorClass {
    pub last_frame: Mutex<Option<RgbaImage>>,
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

            let pixel_buffer = unsafe { CMSampleBuffer::image_buffer(sample_buffer) };

            if pixel_buffer.is_none() {
                return;
            }

            let pixel_buffer = pixel_buffer.unwrap();
            let height = unsafe { CVPixelBufferGetHeight(&pixel_buffer) };
            let width = unsafe { CVPixelBufferGetWidth(&pixel_buffer) };
            let bytes_per_row = unsafe { CVPixelBufferGetBytesPerRow(&pixel_buffer) };
            let size = bytes_per_row * height;

            use objc2_core_video::{CVPixelBufferLockBaseAddress, CVPixelBufferUnlockBaseAddress};

            let lock_result =
                unsafe { CVPixelBufferLockBaseAddress(&pixel_buffer, CVPixelBufferLockFlags(0)) };
            if lock_result != 0 {
                return;
            }

            let base_address = unsafe { CVPixelBufferGetBaseAddress(&pixel_buffer) } as *const u8;

            if base_address.is_null() {
                unsafe {
                    CVPixelBufferUnlockBaseAddress(&pixel_buffer, CVPixelBufferLockFlags(0));
                }
                return;
            }

            if size > isize::MAX as usize {
                unsafe {
                    CVPixelBufferUnlockBaseAddress(&pixel_buffer, CVPixelBufferLockFlags(0));
                }
                return;
            }

            let slice = unsafe { from_raw_parts(base_address, size) };


            let mut image = RgbaImage::new(height as u32, width as u32); // rotated canvas!

            for y in 0..height {
                let row_start = y * bytes_per_row;
                for x in 0..width {
                    let src_index = row_start + x * 4;
                    if src_index + 3 >= slice.len() {
                        continue;
                    }

                    let r = slice[src_index + 2];
                    let g = slice[src_index + 1];
                    let b = slice[src_index];
                    let a = slice[src_index + 3];

                    // Rotate -90°: (x, y) → (height - 1 - y, x)
                    let dest_x = height - 1 - y;
                    let dest_y = x;

                    image.put_pixel(dest_x as u32, dest_y as u32, Rgba([r, g, b, a]));
                }
            }



            *self.ivars().last_frame.lock().unwrap() = Some(image);

            unsafe {
                CVPixelBufferUnlockBaseAddress(&pixel_buffer, CVPixelBufferLockFlags(0));
            }
        }
    }
);

#[cfg(any(target_os = "ios", target_os = "macos"))]
impl Processor {
    pub fn new() -> Retained<Self> {
        let this = Self::alloc();
        let this = this.set_ivars(ProcessorClass {
            last_frame: Mutex::new(None),
        });
        unsafe { objc2::msg_send![super(this), init] }
    }

    // pub fn get_latest_frame(&self) -> Option<RgbaImage> {
    //     self.ivars().last_frame.lock().unwrap().clone()
    // }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
#[derive(Debug)]
pub struct AppleCamera {
    pub session: Retained<AVCaptureSession>,
    processor: Retained<Processor>,
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
impl AppleCamera {
    pub fn new() -> Self {
        unsafe {
            AppleCamera {
                session: AVCaptureSession::new(),
                processor: Processor::new(),
            }
        }
    }

    pub fn open_camera(&self) {
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
    }

    pub fn get_latest_frame(&self) -> Option<RgbaImage> {
        let lock = self.processor.ivars().last_frame.lock().unwrap();
        if lock.is_some() {
            println!("Cloning frame from mutex.");
        }
        lock.clone()
    }
}