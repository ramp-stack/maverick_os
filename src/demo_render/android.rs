use maverick_os::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

pub struct DemoSurface {
    window: Window,
    pixels: Vec<u32>,
}
impl DemoSurface {
    pub fn new(window: Window) -> Self {
        let RawWindowHandle::AndroidNdk(handle) = window.window_handle().unwrap().as_raw() else {
            unreachable!()
        };
        assert_eq!(unsafe {ndk_sys::ANativeWindow_setBuffersGeometry(
            handle.a_native_window.as_ptr().cast(), 0, 0, 1,
        )}, 0);
        Self {window, pixels: Vec::new()}
    }
    pub fn resize(&mut self) {}
    pub fn window(&self) -> &Window {&self.window}
    pub fn draw(&mut self, paint: impl FnOnce(&mut [u32], u32, u32)) -> bool {
        let RawWindowHandle::AndroidNdk(handle) = self.window.window_handle().unwrap().as_raw() else {
            unreachable!()
        };
        let native = handle.a_native_window.as_ptr().cast();
        unsafe {
            let mut buffer = std::mem::zeroed();
            if ndk_sys::ANativeWindow_lock(native, &mut buffer, std::ptr::null_mut()) != 0 {
                return false
            }
            self.pixels.resize(buffer.width as usize * buffer.height as usize, 0);
            paint(&mut self.pixels, buffer.width as u32, buffer.height as u32);
            let target = std::slice::from_raw_parts_mut(buffer.bits.cast::<u32>(),
                buffer.stride as usize * buffer.height as usize);
            for (source, row) in self.pixels.chunks(buffer.width as usize)
                .zip(target.chunks_mut(buffer.stride as usize)) {
                for (source, target) in source.iter().zip(row) {
                    *target = 0xff000000 | (source & 0xff) << 16 | (source & 0xff00) | (source >> 16 & 0xff);
                }
            }
            assert_eq!(ndk_sys::ANativeWindow_unlockAndPost(native), 0);
        }
        true
    }
}
