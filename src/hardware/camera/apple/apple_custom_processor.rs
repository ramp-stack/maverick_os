#![allow(non_snake_case, non_upper_case_globals)]

use std::{sync::{Arc, Mutex}, slice::from_raw_parts};
use image::RgbaImage;


#[cfg(target_os = "macos")]
use crate::hardware::WhiteBalanceMode;

#[cfg(any(target_os = "ios", target_os = "macos"))]
use crate::hardware::CameraSettings;
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
#[cfg(any(target_os = "ios", target_os = "macos"))]
use imageproc::filter;

#[derive(Debug)]
pub struct ProcessorClass {
    pub last_raw_frame: Mutex<Option<RgbaImage>>,
    pub settings: Arc<Mutex<CameraSettings>>,
    pub bayer_format_verified: Mutex<bool>,
    pub ready: Mutex<bool>,
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
define_class!(
    #[unsafe(super = NSObject)]
    #[ivars = ProcessorClass]
    #[derive(Debug)]
    pub struct CustomProcessor;

    unsafe impl NSObjectProtocol for CustomProcessor {}

    unsafe impl AVCaptureVideoDataOutputSampleBufferDelegate for CustomProcessor {
        #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
        fn captureOutput_didOutputSampleBuffer_fromConnection(
            &self,
            _output: &AVCaptureOutput,
            sample_buffer: &CMSampleBuffer,
            _connection: &AVCaptureConnection,
        ) {
            *self.ivars().ready.lock().unwrap() = true;
            
            if let Some(raw_image) = self.process_sample_buffer(sample_buffer) {
                let settings = self.settings().lock().unwrap().clone();

                #[cfg(not(target_os = "ios"))]
                let processed_image = ImageProcessor::apply_image_settings(raw_image, &settings);
                
                #[cfg(target_os = "ios")]
                let mut processed_image = ImageProcessor::apply_image_settings(raw_image, &settings);

                #[cfg(target_os = "ios")]
                { processed_image = self.rotate_90_cw(&processed_image); }

                *self.ivars().last_raw_frame.lock().unwrap() = Some(processed_image);
            }
        }
    }
);

#[cfg(any(target_os = "ios", target_os = "macos"))]
impl CustomProcessor {
    pub fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(ProcessorClass {
            last_raw_frame: Mutex::new(None),
            settings: Arc::new(Mutex::new(CameraSettings::default())),
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
        unsafe { CVPixelBufferLockBaseAddress(pixel_buffer, CVPixelBufferLockFlags(0)) };

        let format = unsafe { CVPixelBufferGetPixelFormatType(pixel_buffer) };
        let (h, w, row_stride) = unsafe {
            (CVPixelBufferGetHeight(pixel_buffer), CVPixelBufferGetWidth(pixel_buffer), CVPixelBufferGetBytesPerRow(pixel_buffer))
        };

        let result = match format {
            kCVPixelFormatType_14Bayer_RGGB => self.process_bayer(pixel_buffer, w, h, row_stride, BayerPattern::RGGB),
            kCVPixelFormatType_14Bayer_BGGR => self.process_bayer(pixel_buffer, w, h, row_stride, BayerPattern::BGGR),
            kCVPixelFormatType_14Bayer_GRBG => self.process_bayer(pixel_buffer, w, h, row_stride, BayerPattern::GRBG),
            kCVPixelFormatType_14Bayer_GBRG => self.process_bayer(pixel_buffer, w, h, row_stride, BayerPattern::GBRG),
            kCVPixelFormatType_32BGRA => self.process_bgra(pixel_buffer, w, h, row_stride),
            kCVPixelFormatType_420YpCbCr8BiPlanarFullRange | kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange => self.process_yuv(pixel_buffer, w, h),
            _ => None,
        };

        unsafe { CVPixelBufferUnlockBaseAddress(pixel_buffer, CVPixelBufferLockFlags(0)) };
        result
    }

    fn process_bayer(&self, pixel_buffer: &CVPixelBuffer, width: usize, height: usize, row_bytes: usize, pattern: BayerPattern) -> Option<RgbaImage> {
        *self.ivars().bayer_format_verified.lock().unwrap() = true;
        let addr = unsafe { CVPixelBufferGetBaseAddress(pixel_buffer) } as *const u8;
        if addr.is_null() { None } else { ImageProcessor::process_bayer_data(addr, width, height, row_bytes, pattern) }
    }

    fn process_bgra(&self, pixel_buffer: &CVPixelBuffer, width: usize, height: usize, row_bytes: usize) -> Option<RgbaImage> {
        let addr = unsafe { CVPixelBufferGetBaseAddress(pixel_buffer) } as *const u8;
        if addr.is_null() { return None; }

        let data = unsafe { from_raw_parts(addr, height * row_bytes) };
        let mut rgba = Vec::with_capacity(width * height * 4);

        for y in 0..height {
            let row = &data[y * row_bytes..][..width * 4];
            for px in row.chunks_exact(4) {
                // Assume input is RGBA (common for some buffers); adjust if confirmed as BGRA
                rgba.extend_from_slice(&[px[2], px[1], px[0], px[3]]); // R, G, B, A // R, G, B, A
            }
        }
        RgbaImage::from_raw(width as u32, height as u32, rgba)
    }
    fn process_yuv(&self, pb: &CVPixelBuffer, width: usize, height: usize) -> Option<RgbaImage> {
        let y_base = unsafe { CVPixelBufferGetBaseAddressOfPlane(pb, 0) } as *const u8;
        let uv_base = unsafe { CVPixelBufferGetBaseAddressOfPlane(pb, 1) } as *const u8;
        if y_base.is_null() || uv_base.is_null() { return None; }

        let y_stride = unsafe { CVPixelBufferGetBytesPerRowOfPlane(pb, 0) };
        let uv_stride = unsafe { CVPixelBufferGetBytesPerRowOfPlane(pb, 1) };

        let y = unsafe { from_raw_parts(y_base, y_stride * height) };
        let uv = unsafe { from_raw_parts(uv_base, uv_stride * height / 2) };
        let mut out = Vec::with_capacity(width * height * 4);

        for j in 0..height {
            for i in 0..width {
                let yv = y[j * y_stride + i] as f32;
                let uv_idx = (j / 2) * uv_stride + (i & !1);
                let u = uv[uv_idx] as f32;     // Cb
                let v = uv[uv_idx + 1] as f32; // Cr

                // BT.709 full-range YUV to RGB conversion
                let r = (yv + 1.5748 * (v - 128.0)).clamp(0.0, 255.0) as u8;
                let g = (yv - 0.1873 * (u - 128.0) - 0.4681 * (v - 128.0)).clamp(0.0, 255.0) as u8;
                let b = (yv + 1.8556 * (u - 128.0)).clamp(0.0, 255.0) as u8;

                out.extend_from_slice(&[r, g, b, 255]);
            }
        }
        RgbaImage::from_raw(width as u32, height as u32, out)
    }
    // pub fn update_settings<F>(&self, f: F) where F: FnOnce(&mut CameraSettings) {
    //     let mut s = self.ivars().settings.lock().unwrap();
    //     f(&mut s);
    //     s.clamp_values();
    // }


    pub fn settings(&self) -> Arc<Mutex<CameraSettings>> {
        self.ivars().settings.clone()
    }

    pub fn is_ready(&self) -> bool { 
        *self.ivars().ready.lock().unwrap() 
    }
    
    #[cfg(target_os = "ios")]
    fn rotate_90_cw(&self, img: &RgbaImage) -> RgbaImage {
        let (width, height) = img.dimensions();
        let mut rotated = RgbaImage::new(height, width);
        for y in 0..height {
            for x in 0..width {
                rotated.put_pixel(height - 1 - y, x, *img.get_pixel(x, y));
            }
        }
        rotated
    }
}


#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(clippy::upper_case_acronyms)]
pub enum BayerPattern { RGGB, BGGR, GRBG, GBRG }

#[derive(Debug, Clone, Copy)]
pub enum PixelType { Red, Green, Blue }

impl BayerPattern {
    pub fn pixel_type(&self, x: usize, y: usize) -> PixelType {
        let (even_row, even_col) = (y % 2 == 0, x % 2 == 0);
        match (self, even_row, even_col) {
            (BayerPattern::RGGB, true, true) | (BayerPattern::BGGR, false, false) => PixelType::Red,
            (BayerPattern::RGGB, false, false) | (BayerPattern::BGGR, true, true) => PixelType::Blue,
            (BayerPattern::GRBG, true, false) | (BayerPattern::GBRG, false, true) => PixelType::Red,
            (BayerPattern::GRBG, false, true) | (BayerPattern::GBRG, true, false) => PixelType::Blue,
            _ => PixelType::Green,
        }
    }
}

pub struct ImageProcessor;

impl ImageProcessor {
    pub fn process_bayer_data(base_address: *const u8, width: usize, height: usize, bytes_per_row: usize, pattern: BayerPattern) -> Option<RgbaImage> {
        let mut bayer_16bit = vec![0u16; width * height];
        
        for y in 0..height {
            let row_start = y * bytes_per_row;
            for x in 0..width {
                let byte_index = row_start + x * 2;
                let pixel_index = y * width + x;
                
                if byte_index + 1 < bytes_per_row * height && pixel_index < bayer_16bit.len() {
                    let raw_slice = unsafe { from_raw_parts(base_address.add(byte_index), 2) };
                    let pixel_14bit = u16::from_le_bytes([raw_slice[0], raw_slice[1]]) & 0x3FFF;
                    bayer_16bit[pixel_index] = pixel_14bit << 2;
                }
            }
        }

        let rgba_data = Self::demosaic_bilinear(&bayer_16bit, width, height, pattern);
        RgbaImage::from_raw(width as u32, height as u32, rgba_data)
    }

    pub fn _process_bgra_data(base_address: *const u8, width: usize, height: usize, bytes_per_row: usize) -> Option<RgbaImage> {
        let slice = unsafe { from_raw_parts(base_address, bytes_per_row * height) };
        let mut rgba_data = Vec::with_capacity(width * height * 4);

        for y in 0..height {
            let row_start = y * bytes_per_row;
            for x in 0..width {
                let src_index = row_start + x * 4;
                if src_index + 3 < slice.len() {
                    let [b, g, r, a] = [slice[src_index], slice[src_index + 1], slice[src_index + 2], slice[src_index + 3]];
                    rgba_data.extend_from_slice(&[r, g, b, a]);
                }
            }
        }
        RgbaImage::from_raw(width as u32, height as u32, rgba_data)
    }

    pub fn apply_image_settings(mut img: RgbaImage, settings: &CameraSettings) -> RgbaImage {
        let bval = settings.brightness.map(|b| (b - 0.5) * 2.0 * 255.0);
        let contrast = settings.contrast.map(|c| 1.0 + (c - 0.5) * 2.0).unwrap_or(1.0);
        let sat = settings.saturation.map(|s| 1.0 + (s - 0.5) * 2.0).unwrap_or(1.0);
        let hue_shift = settings.hue.map(|h| (h - 0.5) * 360.0).unwrap_or(0.0);

        let need_hsv = (sat - 1.0).abs() > f32::EPSILON || hue_shift.abs() > f32::EPSILON;

        for px in img.pixels_mut() {
            let (mut r, mut g, mut b) = (px[0] as f32, px[1] as f32, px[2] as f32);

            if let Some(v) = bval { r += v; g += v; b += v; }

            r = ((r - 128.0) * contrast + 128.0).clamp(0.0, 255.0);
            g = ((g - 128.0) * contrast + 128.0).clamp(0.0, 255.0);
            b = ((b - 128.0) * contrast + 128.0).clamp(0.0, 255.0);

            #[cfg(target_os = "macos")]
            if settings.white_balance_mode == WhiteBalanceMode::Custom {
                if let Some(gains) = &settings.white_balance_gains {
                    r *= gains.red; g *= gains.green; b *= gains.blue;
                }
            }

            if need_hsv {
                let (mut h, mut s, v) = Self::rgb_to_hsv(r, g, b);
                s *= sat; s = s.clamp(0.0, 1.0);
                h = (h + hue_shift) % 360.0; if h < 0.0 { h += 360.0; }
                let (r2, g2, b2) = Self::hsv_to_rgb(h, s, v);

                px[0] = r2.clamp(0.0, 255.0) as u8;
                px[1] = g2.clamp(0.0, 255.0) as u8;
                px[2] = b2.clamp(0.0, 255.0) as u8;
            } else {
                px[0] = r.clamp(0.0, 255.0) as u8;
                px[1] = g.clamp(0.0, 255.0) as u8;
                px[2] = b.clamp(0.0, 255.0) as u8;
            }
        }

        if let Some(amount) = settings.noise_reduction {
            if amount > 0.0 {
                let kernel = [
                    1.0/9.0, 1.0/9.0, 1.0/9.0,
                    1.0/9.0, 1.0/9.0, 1.0/9.0,
                    1.0/9.0, 1.0/9.0, 1.0/9.0,
                ];
                img = filter::filter3x3(&img, &kernel);
            }
        }

        if let Some(strength) = settings.sharpness {
            if strength > 0.0 {
                let kernel = [
                    0.0, -0.5, 0.0,
                    -0.5, 3.0, -0.5,
                    0.0, -0.5, 0.0,
                ];
                img = filter::filter3x3(&img, &kernel);
            }
        }

        img
    }


    fn rgb_to_hsv(r: f32, g: f32, b: f32) -> (f32,f32,f32) {
        let r = r/255.0; let g = g/255.0; let b = b/255.0;
        let max = r.max(g).max(b); let min = r.min(g).min(b); let d = max - min;
        let h = if d==0.0 {0.0} else if max==r {(60.0*((g-b)/d))%360.0} else if max==g {60.0*((b-r)/d+2.0)} else {60.0*((r-g)/d+4.0)};
        let s = if max==0.0 {0.0} else {d/max}; (h,s,max)
    }

    fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (f32,f32,f32) {
        let c = v*s; let x = c*(1.0 - ((h/60.0)%2.0 - 1.0).abs()); let m = v-c;
        let (r1,g1,b1) = match h {
            h if h<60.0 => (c,x,0.0),
            h if h<120.0 => (x,c,0.0),
            h if h<180.0 => (0.0,c,x),
            h if h<240.0 => (0.0,x,c),
            h if h<300.0 => (x,0.0,c),
            _ => (c,0.0,x)
        };
        ((r1+m)*255.0,(g1+m)*255.0,(b1+m)*255.0)
    }



    // fn rgb_to_hsl(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    //     let (r, g, b) = (r / 255.0, g / 255.0, b / 255.0);
    //     let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    //     let l = (max + min) / 2.0;
    //     if max == min { return (0.0, 0.0, l); }
    //     let d = max - min;
    //     let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    //     let h = if max == r {
    //         ((g - b) / d + if g < b { 6.0 } else { 0.0 }) / 6.0
    //     } else if max == g {
    //         ((b - r) / d + 2.0) / 6.0
    //     } else {
    //         ((r - g) / d + 4.0) / 6.0
    //     };
    //     (h * 360.0, s, l)
    // }

    // fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    //     let h = h / 360.0;
    //     if s == 0.0 {
    //         let v = (l * 255.0).clamp(0.0, 255.0);
    //         return (v, v, v);
    //     }
    //     let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    //     let p = 2.0 * l - q;
    //     fn hue_to_rgb(p: f32, q: f32, mut t: f32) -> f32 {
    //         if t < 0.0 { t += 1.0; }
    //         if t > 1.0 { t -= 1.0; }
    //         if t < 1.0/6.0 { p + (q - p) * 6.0 * t }
    //         else if t < 1.0/2.0 { q }
    //         else if t < 2.0/3.0 { p + (q - p) * (2.0/3.0 - t) * 6.0 }
    //         else { p }
    //     }
    //     (
    //         (hue_to_rgb(p, q, h + 1.0/3.0) * 255.0).clamp(0.0, 255.0),
    //         (hue_to_rgb(p, q, h) * 255.0).clamp(0.0, 255.0),
    //         (hue_to_rgb(p, q, h - 1.0/3.0) * 255.0).clamp(0.0, 255.0),
    //     )
    // }

    // // Helper: apply color filters
    // fn apply_color_filter(filter: ColorFilter, r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    //     match filter {
    //         ColorFilter::None => (r, g, b),
    //         ColorFilter::Mono => {
    //             let gray = 0.299 * r + 0.587 * g + 0.114 * b;
    //             (gray, gray, gray)
    //         }
    //         ColorFilter::Sepia => {
    //             let nr = (0.393*r + 0.769*g + 0.189*b).clamp(0.0, 255.0);
    //             let ng = (0.349*r + 0.686*g + 0.168*b).clamp(0.0, 255.0);
    //             let nb = (0.272*r + 0.534*g + 0.131*b).clamp(0.0, 255.0);
    //             (nr, ng, nb)
    //         }
    //         ColorFilter::Vibrant => {
    //             ((r*1.1).clamp(0.0,255.0), (g*1.1).clamp(0.0,255.0), (b*1.1).clamp(0.0,255.0))
    //         }
    //         ColorFilter::Cool => ((r*0.9).clamp(0.0,255.0), g, (b*1.1).clamp(0.0,255.0)),
    //         ColorFilter::Warm => ((r*1.1).clamp(0.0,255.0), g, (b*0.9).clamp(0.0,255.0)),
    //     }
    // }


    fn demosaic_bilinear(bayer_data: &[u16], width: usize, height: usize, pattern: BayerPattern) -> Vec<u8> {
        let mut rgb_data = vec![0u8; width * height * 4];
        
        for y in 1..height-1 {
            for x in 1..width-1 {
                let (idx, rgba_idx) = (y * width + x, (y * width + x) * 4);
                let pixel_val = (bayer_data[idx] >> 8) as u8;
                
                let (r, g, b) = match pattern.pixel_type(x, y) {
                    PixelType::Red => (pixel_val, Self::interpolate_orthogonal(bayer_data, x, y, width), Self::interpolate_diagonal(bayer_data, x, y, width)),
                    PixelType::Green => (Self::interpolate_diagonal(bayer_data, x, y, width), pixel_val, Self::interpolate_diagonal(bayer_data, x, y, width)),
                    PixelType::Blue => (Self::interpolate_diagonal(bayer_data, x, y, width), Self::interpolate_orthogonal(bayer_data, x, y, width), pixel_val),
                };
                
                [rgb_data[rgba_idx], rgb_data[rgba_idx + 1], rgb_data[rgba_idx + 2], rgb_data[rgba_idx + 3]] = [r, g, b, 255];
            }
        }
        rgb_data
    }

    fn interpolate_orthogonal(data: &[u16], x: usize, y: usize, width: usize) -> u8 {
        let indices = [(y-1, x), (y, x-1), (y, x+1), (y+1, x)];
        let avg = indices.iter().map(|&(row, col)| data.get(row * width + col).unwrap_or(&0)).map(|&v| v as u32).sum::<u32>() / 4;
        (avg >> 8) as u8
    }

    fn interpolate_diagonal(data: &[u16], x: usize, y: usize, width: usize) -> u8 {
        let indices = [(y-1, x-1), (y-1, x+1), (y+1, x-1), (y+1, x+1)];
        let avg = indices.iter().map(|&(row, col)| data.get(row * width + col).unwrap_or(&0)).map(|&v| v as u32).sum::<u32>() / 4;
        (avg >> 8) as u8
    }
}