use std::slice::from_raw_parts;
use image::RgbaImage;
use crate::hardware::ImageSettings;

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

    pub fn apply_image_settings(rgba_image: RgbaImage, settings: &ImageSettings) -> RgbaImage {
        let (width, height) = (rgba_image.width(), rgba_image.height());
        let mut pixels = rgba_image.into_raw();
        
        let wb_multipliers = if settings.temperature != 6500.0 { 
            settings.temperature_to_rgb_multipliers() 
        } else { 
            [settings.white_balance_r, settings.white_balance_g, settings.white_balance_b] 
        };
        
        let has_wb = wb_multipliers.iter().any(|&m| m != 1.0);
        
        pixels.chunks_exact_mut(4).for_each(|pixel| {
            let [mut r, mut g, mut b] = [pixel[0] as f32, pixel[1] as f32, pixel[2] as f32];
            
            if has_wb {
                r = (r * wb_multipliers[0]).clamp(0.0, 255.0);
                g = (g * wb_multipliers[1]).clamp(0.0, 255.0);
                b = (b * wb_multipliers[2]).clamp(0.0, 255.0);
            }
            
            if settings.exposure != 0.0 {
                let multiplier = 2.0_f32.powf(settings.exposure);
                [r, g, b] = [r * multiplier, g * multiplier, b * multiplier].map(|v| v.clamp(0.0, 255.0));
            }
            
            if settings.brightness != 0 {
                let brightness = settings.brightness as f32;
                [r, g, b] = [r + brightness, g + brightness, b + brightness].map(|v| v.clamp(0.0, 255.0));
            }
            
            if settings.contrast != 0.0 {
                let factor = 1.0 + settings.contrast;
                [r, g, b] = [(r - 128.0) * factor + 128.0, (g - 128.0) * factor + 128.0, (b - 128.0) * factor + 128.0].map(|v| v.clamp(0.0, 255.0));
            }
            
            if settings.saturation != 0.0 {
                let gray = 0.299 * r + 0.587 * g + 0.114 * b;
                let factor = 1.0 + settings.saturation;
                [r, g, b] = [gray + (r - gray) * factor, gray + (g - gray) * factor, gray + (b - gray) * factor].map(|v| v.clamp(0.0, 255.0));
            }
            
            if settings.gamma != 2.2 {
                let inv_gamma = 1.0 / settings.gamma;
                [r, g, b] = [255.0 * (r / 255.0).powf(inv_gamma), 255.0 * (g / 255.0).powf(inv_gamma), 255.0 * (b / 255.0).powf(inv_gamma)].map(|v| v.clamp(0.0, 255.0));
            }
            
            [pixel[0], pixel[1], pixel[2]] = [r as u8, g as u8, b as u8];
        });
        
        RgbaImage::from_raw(width, height, pixels).unwrap_or_else(|| RgbaImage::new(width, height))
    }

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