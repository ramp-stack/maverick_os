use raw_window_handle::{DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle};
use winit::window::{Window as WinitWindow, WindowId};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};

use std::sync::Arc;

use crate::hardware::{SafeArea, KeyboardInset, Keyboard};
use crate::events::Output;

#[derive(PartialEq)]
pub struct PhysicalWindow {
    pub width: u32,
    pub height: u32,
    pub scale_factor: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LogicalWindow {
    /// Available size above native keyboard obstruction, including animation frames.
    pub size: (f32, f32),
    pub insets: (f32, f32, f32, f32),
    /// Selects mobile layout and touch-oriented interaction behavior.
    pub is_mobile: bool,
}

#[derive(Clone)]
pub struct Window(Arc<WinitWindow>, Arc<KeyboardInset>);
impl Window {
    pub(crate) fn new(event_loop: &ActiveEventLoop, name: String, proxy: EventLoopProxy<Output>) -> (Self, Keyboard) {
        let window = Arc::new(event_loop.create_window(
            WinitWindow::default_attributes().with_title(&name)
        ).unwrap());
        let inset = Arc::new(KeyboardInset::new(&window, proxy.clone()));
        let keyboard = Keyboard::new(proxy, inset.clone());
        (Self(window, inset), keyboard)
    }

    pub fn id(&self) -> WindowId {self.0.id()}

    pub fn request_redraw(&self) {self.0.request_redraw()}

    pub fn physical(&self) -> PhysicalWindow {SafeArea::physical(&self.0)}

    pub fn logical(&self) -> LogicalWindow {
        let physical = self.physical();
        let overlap = self.1.get().0;
        let mut window = LogicalWindow {
            size: (
                (physical.width as f64 / physical.scale_factor) as f32,
                (physical.height as f64 / physical.scale_factor) as f32
            ),
            insets: SafeArea::get(&self.0),
            is_mobile: SafeArea::is_mobile(),
        };
        window.size.1 = (window.size.1 - overlap).max(0.0);
        window.insets.2 = (window.insets.2 - overlap).max(0.0);
        window
    }
}

impl HasWindowHandle for Window {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {self.0.window_handle()}
}

impl HasDisplayHandle for Window {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {self.0.display_handle()}
}
