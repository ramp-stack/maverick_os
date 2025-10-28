#![allow(non_snake_case, non_upper_case_globals)]

use std::{sync::Mutex, slice::from_raw_parts};
use image::RgbaImage;
use image::Rgba;

use objc2::__framework_prelude::NSObject;
use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::{define_class, AllocAnyThread, DeclaredClass};
use objc2_core_media::CMSampleBuffer;
use objc2_av_foundation::*;
use objc2_core_video::*;
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};
use dispatch2::DispatchQueue;
use objc2::runtime::ProtocolObject;

#[derive(Debug)]
pub struct ProcessorClass {
    pub last_frame: Mutex<Option<RgbaImage>>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[ivars = ProcessorClass]
    #[derive(Debug)]
    pub struct StandardProcessor;

    unsafe impl NSObjectProtocol for StandardProcessor {}

    unsafe impl AVCaptureVideoDataOutputSampleBufferDelegate for StandardProcessor {
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

impl StandardProcessor {
    pub fn new() -> Retained<Self> {
        let this = Self::alloc();
        let this = this.set_ivars(ProcessorClass {
            last_frame: Mutex::new(None),
        });
        unsafe { objc2::msg_send![super(this), init] }
    }
}

#[derive(Debug, Clone)]
pub struct StandardOsCamera {
    pub session: Retained<AVCaptureSession>,
    processor: Retained<StandardProcessor>,
}

impl StandardOsCamera {
    pub fn new() -> Result<Self, String> {
        let camera = unsafe {
            StandardOsCamera {
                session: AVCaptureSession::new(),
                processor: StandardProcessor::new(),
            }
        };

        // Start the camera immediately and propagate errors
        camera.start()?;

        Ok(camera)
    }

    pub fn start(&self) -> Result<(), String> {
        unsafe {
            let device_types = NSArray::from_slice(&[AVCaptureDeviceTypeBuiltInWideAngleCamera]);

            let discovery_session = AVCaptureDeviceDiscoverySession::discoverySessionWithDeviceTypes_mediaType_position(
                &device_types,
                AVMediaTypeVideo,
                AVCaptureDevicePosition::Back,
            );

            let devices = discovery_session.devices();

            let device = devices.into_iter().next().ok_or("No camera device found")?;

            // Fix: Use map_err instead of ok_or since deviceInputWithDevice_error returns a Result
            let input = AVCaptureDeviceInput::deviceInputWithDevice_error(&device)
                .map_err(|e| format!("Failed to create AVCaptureDeviceInput: {:?}", e))?;

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
        self.processor.ivars().last_frame.lock().unwrap().clone()
    }

    pub fn stop(&self) {
        unsafe {
            self.session.stopRunning();
        }
    }
}

impl Default for StandardOsCamera {
    fn default() -> Self {
        StandardOsCamera::new().expect("Failed to initialize camera")
    }
}