use std::num::NonZeroU32;

use font8x8::UnicodeFonts;
use image::RgbaImage;
use softbuffer::{Context, Surface};

use maverick_os::{Data, Renderer, Window};

pub struct Quad {
    pub label: &'static str,
    pub data: Option<Data>,
}

pub struct Screen {
    pub top_left: Quad,
    pub top_right: Quad,
    pub bottom_left: Quad,
    pub bottom_right: Quad,
    pub buttons: Vec<Button>,
}

#[derive(Clone, Copy)]
pub struct Button {
    pub label: &'static str,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

pub struct DemoRenderer {
    surface: Surface<Window, Window>,
    _context: Context<Window>,
}

impl Renderer for DemoRenderer {
    type Instructions = Screen;

    fn new(window: Window) -> Self {
        let context = Context::new(window.clone()).unwrap();
        let surface = Surface::new(&context, window).unwrap();
        let mut renderer = DemoRenderer{surface, _context: context};
        renderer.resize();
        renderer
    }

    fn resize(&mut self) {
        let size = self.surface.window().physical();
        let (Some(width), Some(height)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {return};
        self.surface.resize(width, height).unwrap();
    }

    fn draw(&mut self, screen: Self::Instructions) {
        let scale = self.surface.window().physical().scale_factor as f32;
        let mut buffer = match self.surface.buffer_mut() {
            Ok(buffer) => buffer,
            Err(err) => {log::error!("surface: {err}"); return}
        };
        let width = buffer.width().get();
        let height = buffer.height().get();
        Canvas{
            pixels: &mut buffer,
            width, height, scale,
            glyph: scale.round() as u32,
        }.paint(&screen);
        buffer.present().unwrap();
    }
}

struct Canvas<'a> {
    pixels: &'a mut [u32],
    width: u32,
    height: u32,
    scale: f32,
    glyph: u32,
}

impl Canvas<'_> {
    fn paint(&mut self, screen: &Screen) {
        let content = self.content_height(screen);
        let mid_x = self.width / 2;
        let mid_y = content / 2;
        let camera_on = [
            &screen.top_left, &screen.top_right,
            &screen.bottom_left, &screen.bottom_right,
        ].into_iter().any(|quad|
            quad.label == "camera" && matches!(quad.data, Some(Data::Image(_))));
        self.paint_quad(0, 0, mid_x, mid_y, 0x00161616, &screen.top_left);
        self.paint_quad(mid_x, 0, self.width - mid_x, mid_y, 0x001c1c1c, &screen.top_right);
        self.paint_quad(0, mid_y, mid_x, content - mid_y, 0x00141820, &screen.bottom_left);
        self.paint_quad(mid_x, mid_y, self.width - mid_x, content - mid_y, 0x00201814, &screen.bottom_right);
        self.fill(0, content, self.width, self.height.saturating_sub(content), 0x00101010);
        for button in &screen.buttons {self.paint_button(button, camera_on);}
    }

    fn content_height(&self, screen: &Screen) -> u32 {
        let Some(top) = screen.buttons.iter().map(|button| button.y).reduce(f32::min) else {return self.height};
        ((top * self.scale).round().max(0.0) as u32).min(self.height)
    }

    fn paint_button(&mut self, button: &Button, camera_on: bool) {
        let (x, y, w, h) = self.rect(button.x, button.y, button.w, button.h);
        let fill = if camera_on && button.label == "camera" {0x00245c3a} else {0x00282828};
        self.fill(x, y, w, h, 0x005a5a5a);
        let edge = self.glyph;
        self.fill(x + edge, y + edge, w.saturating_sub(edge * 2), h.saturating_sub(edge * 2), fill);
        let text_w = (button.label.chars().count() as u32 * self.cell()).min(w.saturating_sub(edge * 2));
        self.draw_text(x + w.saturating_sub(text_w) / 2, y + h.saturating_sub(self.cell()) / 2, text_w, self.cell(), button.label, 0x00f2f2f2);
    }

    fn rect(&self, x: f32, y: f32, w: f32, h: f32) -> (u32, u32, u32, u32) {
        (
            (x * self.scale).round().max(0.0) as u32,
            (y * self.scale).round().max(0.0) as u32,
            (w * self.scale).round().max(0.0) as u32,
            (h * self.scale).round().max(0.0) as u32,
        )
    }

    fn cell(&self) -> u32 {8 * self.glyph}
    fn pad(&self) -> u32 {4 * self.glyph}

    fn paint_quad(&mut self, x: u32, y: u32, w: u32, h: u32, bg: u32, quad: &Quad) {
        self.fill(x, y, w, h, bg);
        self.draw_text(x + self.pad(), y + self.pad(), w.saturating_sub(self.pad() * 2), self.cell(), quad.label, 0x00a0a0a0);
        let (cx, cy, cw, ch) = self.content(x, y, w, h);
        match quad.data.as_ref() {
            Some(Data::Image(image)) => self.blit(cx, cy, cw, ch, image),
            Some(Data::Video(video)) => self.blit(cx, cy, cw, ch, &video.thumbnail()),
            Some(Data::Text(text)) => self.draw_text(cx, cy, cw, ch, text, 0x00f2f2f2),
            Some(Data::File(path)) => self.draw_text(
                cx, cy, cw, ch, &path.to_string_lossy(), 0x00f2f2f2,
            ),
            None => self.draw_text(cx, cy, cw, ch, "empty", 0x00f2f2f2),
        }
    }

    fn content(&self, x: u32, y: u32, w: u32, h: u32) -> (u32, u32, u32, u32) {
        let pad = self.pad();
        let label = self.cell();
        (x + pad, y + pad + label + pad, w.saturating_sub(pad * 2), h.saturating_sub(pad * 3 + label))
    }

    fn fill(&mut self, x: u32, y: u32, w: u32, h: u32, color: u32) {
        let x1 = (x + w).min(self.width);
        let y1 = (y + h).min(self.height);
        if x >= self.width || y >= self.height || x >= x1 || y >= y1 {return}
        for row in y..y1 {
            let start = row as usize * self.width as usize + x as usize;
            let end = row as usize * self.width as usize + x1 as usize;
            self.pixels[start..end].fill(color);
        }
    }

    fn blit(&mut self, x: u32, y: u32, w: u32, h: u32, image: &RgbaImage) {
        let iw = image.width();
        let ih = image.height();
        if iw == 0 || ih == 0 || w == 0 || h == 0 {return}
        for row in 0..h {
            let sy = (row as u64 * ih as u64 / h as u64) as u32;
            let dst = (y + row) as usize * self.width as usize + x as usize;
            for col in 0..w {
                let sx = (col as u64 * iw as u64 / w as u64) as u32;
                let pixel = image.get_pixel(sx, sy).0;
                self.pixels[dst + col as usize] = (pixel[0] as u32) << 16 | (pixel[1] as u32) << 8 | pixel[2] as u32;
            }
        }
    }

    fn draw_text(&mut self, x: u32, y: u32, w: u32, h: u32, text: &str, color: u32) {
        let cell = self.cell();
        let cols = w / cell;
        let rows = h / cell;
        if cols == 0 || rows == 0 {return}
        let mut col = 0u32;
        let mut row = 0u32;
        for ch in text.chars() {
            if ch == '\n' || col >= cols {
                col = 0;
                row += 1;
                if row >= rows {break}
                if ch == '\n' {continue}
            }
            self.draw_glyph(x + col * cell, y + row * cell, ch, color);
            col += 1;
        }
    }

    fn draw_glyph(&mut self, x: u32, y: u32, ch: char, color: u32) {
        let Some(glyph) = font8x8::BASIC_FONTS.get(ch).or_else(|| font8x8::BASIC_FONTS.get('?')) else {return};
        let scale = self.glyph;
        for (row, bits) in glyph.into_iter().enumerate() {
            for col in 0..8u32 {
                if bits & (1 << col) == 0 {continue}
                for dy in 0..scale {
                    for dx in 0..scale {
                        let px = x + col * scale + dx;
                        let py = y + row as u32 * scale + dy;
                        if px >= self.width {continue}
                        let index = py as usize * self.width as usize + px as usize;
                        if index < self.pixels.len() {self.pixels[index] = color}
                    }
                }
            }
        }
    }
}
