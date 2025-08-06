use std::slice::from_raw_parts;
use image::RgbaImage;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BayerPattern { RGGB, BGGR, GRBG, GBRG }

impl BayerPattern {
    pub fn pixel_type(&self, x: usize, y: usize) -> PixelType {
        let even_row = y % 2 == 0;
        let even_col = x % 2 == 0;
        
        match self {
            BayerPattern::RGGB => match (even_row, even_col) {
                (true, true) => PixelType::Red,
                (true, false) => PixelType::Green,
                (false, true) => PixelType::Green,
                (false, false) => PixelType::Blue,
            },
            BayerPattern::BGGR => match (even_row, even_col) {
                (true, true) => PixelType::Blue,
                (true, false) => PixelType::Green,
                (false, true) => PixelType::Green,
                (false, false) => PixelType::Red,
            },
            BayerPattern::GRBG => match (even_row, even_col) {
                (true, true) => PixelType::Green,
                (true, false) => PixelType::Red,
                (false, true) => PixelType::Blue,
                (false, false) => PixelType::Green,
            },
            BayerPattern::GBRG => match (even_row, even_col) {
                (true, true) => PixelType::Green,
                (true, false) => PixelType::Blue,
                (false, true) => PixelType::Red,
                (false, false) => PixelType::Green,
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum PixelType { Red, Green, Blue }

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
            let g = (0.39008157 * temp_scaled.ln() - 0.63184144).clamp(0.0, 1.0);
            let b = if temp < 2000.0 {
                0.0
            } else {
                (0.54320678 * (temp_scaled - 10.0).ln() - 1.19625408).clamp(0.0, 1.0)
            };
            [r, g, b]
        } else {
            let r = (1.29293618 * (temp_scaled - 60.0).powf(-0.1332047)).clamp(0.0, 1.0);
            let g = (1.12989086 * (temp_scaled - 60.0).powf(-0.0755148)).clamp(0.0, 1.0);
            let b = 1.0;
            [r, g, b]
        }
    }
}

pub struct ImageProcessor;

impl ImageProcessor {
    pub fn process_bayer_data(base_address: *const u8, width: usize, height: usize, bytes_per_row: usize, pattern: BayerPattern) -> Option<RgbaImage> {
        let start = Instant::now(); 
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
        println!("fps: {:?}", start.elapsed().as_millis()); 
        RgbaImage::from_raw(width as u32, height as u32, rgba_data)
    }

    pub fn process_bgra_data(base_address: *const u8, width: usize, height: usize, bytes_per_row: usize) -> Option<RgbaImage> {
        let slice = unsafe { from_raw_parts(base_address, bytes_per_row * height) };
        let mut rgba_data = Vec::with_capacity(width * height * 4);

        for y in 0..height {
            let row_start = y * bytes_per_row;
            for x in 0..width {
                let src_index = row_start + x * 4;
                
                if src_index + 3 < slice.len() {
                    let b = slice[src_index];
                    let g = slice[src_index + 1];
                    let r = slice[src_index + 2];
                    let a = slice[src_index + 3];
                    
                    rgba_data.extend_from_slice(&[r, g, b, a]);
                }
            }
        }

        RgbaImage::from_raw(width as u32, height as u32, rgba_data)
    }

    pub fn apply_image_settings(rgba_image: RgbaImage, settings: &ImageSettings) -> RgbaImage {
        let width = rgba_image.width();
        let height = rgba_image.height();
        let mut pixels = rgba_image.into_raw();
        
        let wb_multipliers = if settings.temperature != 6500.0 {
            settings.temperature_to_rgb_multipliers()
        } else {
            [settings.white_balance_r, settings.white_balance_g, settings.white_balance_b]
        };
        
        let has_wb = wb_multipliers[0] != 1.0 || wb_multipliers[1] != 1.0 || wb_multipliers[2] != 1.0;
        
        pixels.chunks_exact_mut(4).for_each(|pixel| {
            let mut r = pixel[0] as f32;
            let mut g = pixel[1] as f32;
            let mut b = pixel[2] as f32;
            
            if has_wb {
                r = (r * wb_multipliers[0]).clamp(0.0, 255.0);
                g = (g * wb_multipliers[1]).clamp(0.0, 255.0);
                b = (b * wb_multipliers[2]).clamp(0.0, 255.0);
            }
            
            if settings.exposure != 0.0 {
                let exposure_multiplier = 2.0_f32.powf(settings.exposure);
                r = (r * exposure_multiplier).clamp(0.0, 255.0);
                g = (g * exposure_multiplier).clamp(0.0, 255.0);
                b = (b * exposure_multiplier).clamp(0.0, 255.0);
            }
            
            if settings.brightness != 0 {
                let brightness_f = settings.brightness as f32;
                r = (r + brightness_f).clamp(0.0, 255.0);
                g = (g + brightness_f).clamp(0.0, 255.0);
                b = (b + brightness_f).clamp(0.0, 255.0);
            }
            
            if settings.contrast != 0.0 {
                let contrast_factor = 1.0 + settings.contrast;
                r = ((r - 128.0) * contrast_factor + 128.0).clamp(0.0, 255.0);
                g = ((g - 128.0) * contrast_factor + 128.0).clamp(0.0, 255.0);
                b = ((b - 128.0) * contrast_factor + 128.0).clamp(0.0, 255.0);
            }
            
            if settings.saturation != 0.0 {
                let gray = 0.299 * r + 0.587 * g + 0.114 * b;
                let saturation_factor = 1.0 + settings.saturation;
                r = (gray + (r - gray) * saturation_factor).clamp(0.0, 255.0);
                g = (gray + (g - gray) * saturation_factor).clamp(0.0, 255.0);
                b = (gray + (b - gray) * saturation_factor).clamp(0.0, 255.0);
            }
            
            if settings.gamma != 2.2 {
                let inv_gamma = 1.0 / settings.gamma;
                r = (255.0 * (r / 255.0).powf(inv_gamma)).clamp(0.0, 255.0);
                g = (255.0 * (g / 255.0).powf(inv_gamma)).clamp(0.0, 255.0);
                b = (255.0 * (b / 255.0).powf(inv_gamma)).clamp(0.0, 255.0);
            }
            
            pixel[0] = r as u8;
            pixel[1] = g as u8;
            pixel[2] = b as u8;
        });
        
        RgbaImage::from_raw(width, height, pixels).unwrap_or_else(|| {
            RgbaImage::new(width, height)
        })
    }

    fn demosaic_bilinear(bayer_data: &[u16], width: usize, height: usize, pattern: BayerPattern) -> Vec<u8> {
        let mut rgb_data = vec![0u8; width * height * 4]; 
        
        for y in 1..height-1 {
            for x in 1..width-1 {
                let idx = y * width + x;
                let rgba_idx = idx * 4;
                
                let pixel_val = (bayer_data[idx] >> 8) as u8;
                
                match pattern.pixel_type(x, y) {
                    PixelType::Red => {
                        rgb_data[rgba_idx] = pixel_val;
                        rgb_data[rgba_idx + 1] = Self::interpolate_green(bayer_data, x, y, width);
                        rgb_data[rgba_idx + 2] = Self::interpolate_blue(bayer_data, x, y, width);
                        rgb_data[rgba_idx + 3] = 255;
                    },
                    PixelType::Green => {
                        rgb_data[rgba_idx] = Self::interpolate_red(bayer_data, x, y, width);
                        rgb_data[rgba_idx + 1] = pixel_val;
                        rgb_data[rgba_idx + 2] = Self::interpolate_blue(bayer_data, x, y, width);
                        rgb_data[rgba_idx + 3] = 255;
                    },
                    PixelType::Blue => {
                        rgb_data[rgba_idx] = Self::interpolate_red(bayer_data, x, y, width);
                        rgb_data[rgba_idx + 1] = Self::interpolate_green(bayer_data, x, y, width);
                        rgb_data[rgba_idx + 2] = pixel_val;
                        rgb_data[rgba_idx + 3] = 255;
                    },
                }
            }
        }
        
        rgb_data
    }

    fn interpolate_green(data: &[u16], x: usize, y: usize, width: usize) -> u8 {
        let neighbors = [
            data.get((y-1) * width + x).unwrap_or(&0),
            data.get(y * width + x-1).unwrap_or(&0),
            data.get(y * width + x+1).unwrap_or(&0),
            data.get((y+1) * width + x).unwrap_or(&0),
        ];
        let avg = neighbors.iter().map(|&v| *v as u32).sum::<u32>() / 4;
        (avg >> 8) as u8
    }

    fn interpolate_red(data: &[u16], x: usize, y: usize, width: usize) -> u8 {
        let neighbors = [
            data.get((y-1) * width + x-1).unwrap_or(&0),
            data.get((y-1) * width + x+1).unwrap_or(&0),
            data.get((y+1) * width + x-1).unwrap_or(&0),
            data.get((y+1) * width + x+1).unwrap_or(&0),
        ];
        let avg = neighbors.iter().map(|&v| *v as u32).sum::<u32>() / 4;
        (avg >> 8) as u8
    }

    fn interpolate_blue(data: &[u16], x: usize, y: usize, width: usize) -> u8 {
        let neighbors = [
            data.get((y-1) * width + x-1).unwrap_or(&0),
            data.get((y-1) * width + x+1).unwrap_or(&0),
            data.get((y+1) * width + x-1).unwrap_or(&0),
            data.get((y+1) * width + x+1).unwrap_or(&0),
        ];
        let avg = neighbors.iter().map(|&v| *v as u32).sum::<u32>() / 4;
        (avg >> 8) as u8
    }
}