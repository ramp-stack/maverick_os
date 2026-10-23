use maverick_os::Window;
use softbuffer::{Context, Surface};
use std::num::NonZeroU32;

pub struct DemoSurface {
    surface: Surface<Window, Window>,
    _context: Context<Window>,
}
impl DemoSurface {
    pub fn new(window: Window) -> Self {
        let context = Context::new(window.clone()).unwrap();
        let surface = Surface::new(&context, window).unwrap();
        Self {surface, _context: context}
    }
    pub fn resize(&mut self) {
        let size = self.window().physical();
        let (Some(width), Some(height)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {return};
        self.surface.resize(width, height).unwrap();
    }
    pub fn window(&self) -> &Window {self.surface.window()}
    pub fn draw(&mut self, paint: impl FnOnce(&mut [u32], u32, u32)) -> bool {
        let mut buffer = match self.surface.buffer_mut() {
            Ok(buffer) => buffer,
            Err(error) => {log::error!("surface: {error}"); return false}
        };
        let (width, height) = (buffer.width().get(), buffer.height().get());
        paint(&mut buffer, width, height);
        buffer.present().unwrap();
        true
    }
}
