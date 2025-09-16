#![allow(non_snake_case, non_upper_case_globals)]

use std::{sync::Mutex, slice::from_raw_parts};
use image::RgbaImage;
use image::Rgba;

#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2::__framework_prelude::NSObject;
#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2::rc::Retained;
#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2::runtime::{NSObjectProtocol};
#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2::{define_class, AllocAnyThread, DeclaredClass};
#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2_core_media::CMSampleBuffer;
#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2_av_foundation::*;
#[cfg(any(target_os = "ios", target_os = "macos"))]
use objc2_core_video::*;


#[derive(Debug)]
pub struct ProcessorClass {
    pub last_frame: Mutex<Option<RgbaImage>>,
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
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

#[cfg(any(target_os = "ios", target_os = "macos"))]
impl StandardProcessor {
    pub fn new() -> Retained<Self> {
        let this = Self::alloc();
        let this = this.set_ivars(ProcessorClass {
            last_frame: Mutex::new(None),
        });
        unsafe { objc2::msg_send![super(this), init] }
    }
}

