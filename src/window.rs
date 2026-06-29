use winit::window::WindowId;
pub use winit::event::{WindowEvent, DeviceEvent, DeviceId, TouchPhase, Touch, AxisId, MouseButton as WinitMouseButton, MouseScrollDelta, ElementState, KeyEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::application::ApplicationHandler;
use winit::event::StartCause;

#[cfg(target_os="android")]
use winit::platform::android::activity::AndroidApp;
#[cfg(target_os="android")]
use winit::platform::android::EventLoopBuilderExtAndroid;

use std::path::PathBuf;
use std::time::Instant;
use std::time::Duration;
use std::sync::Arc;
use image::RgbaImage;

use winit::keyboard::{NamedKey as WinitNamedKey, Key as WinitKey};
use winit::window::Window as WinitWindow;

use crate::{MaverickOS, Application};

use raw_window_handle::{HasWindowHandle, HasDisplayHandle};

const TICK: Duration = Duration::from_millis(0);//60 fps

pub trait Handle: HasWindowHandle + HasDisplayHandle + Send + Sync {}
impl<T: HasWindowHandle + HasDisplayHandle + Send + Sync> Handle for T {}

pub trait Renderer<'surface> {
    type Application: Application;

    fn new(context: &Context, window: &'surface dyn Handle) -> Self;
    fn resize(&mut self, context: &Context);
    fn draw(&mut self, context: &Context, app: &Self::Application);
}

#[derive(Debug, Clone)]
pub struct Context {
    pub width: u32,
    pub height: u32,
    pub scale_factor: f64
}
impl Context {
    pub fn new(window: &WinitWindow) -> Self {
        let size = window.inner_size();
        Context{width: size.width, height: size.height, scale_factor: window.scale_factor()}
    }
}

pub(crate) struct Surface<A: Application>(Arc<WinitWindow>, &'static dyn Handle, Option<A::Renderer<'static>>);
impl<A: Application> Surface<A> {
    pub fn id(&self) -> WindowId {self.0.id()}
    pub fn new(window: WinitWindow, context: &Context) -> Self {
        let window = Arc::new(window);
        let handle: &'static dyn Handle = unsafe {
            std::mem::transmute::<&dyn Handle, &'static dyn Handle>(&*window)
        };
        let renderer = A::Renderer::new(context, handle);
        Surface(window, handle, Some(renderer))
    }
    pub fn suspend(&mut self) {self.2 = None;}
    pub fn resurface(&mut self, context: &Context) {self.2 = Some(A::Renderer::new(context, self.1));}
    pub fn request_redraw(&mut self) {self.0.request_redraw()}
    pub fn as_mut(&mut self) -> Option<&mut A::Renderer<'static>> {self.2.as_mut()}
}

#[derive(Clone, Debug)]
pub enum Input {
    Tick,
    Resized,
    WindowFocused(bool),
    WindowMoved(i32, i32),
    CameraFrame(RgbaImage),
    SelectedPhoto(RgbaImage),
    DroppedFile(PathBuf),
    HoveredFile(PathBuf),
    HoveredFileCancelled,
    Device(DeviceId, DeviceInput),
}

#[derive(Clone, Debug)]
pub enum DeviceInput {
    Keyboard(Key, KeyboardState, Modifiers),
    CursorEntered,
    CursorLeft,
    Mouse((f32, f32), MouseState),
}
impl DeviceInput {
    pub fn scale(self, scale: f32) -> Self {match self {
        Self::Mouse(pos, state) => Self::Mouse((pos.0 * scale, pos.1 * scale), match state{
            MouseState::Scroll(dx, dy) => MouseState::Scroll(dx * scale, dy * scale),
            b => b
        }),
        e => e
    }}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub shift:     bool,
    pub control:   bool,
    pub alt:       bool,
    pub supermeta: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Key {
    Escape, Enter, Tab, Space,
    Up, Down, Left, Right,
    Delete, Backspace, Home, End,
    Shift, Control, Alt, SuperMeta,
    CapsLock, NumLock, ScrollLock,
    Character(char)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyboardState{ Pressed, Repeated, Released }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton{ Left, Right, Middle }

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MouseState {
    Pressed(MouseButton),
    Released(MouseButton),
    Scroll(f32, f32),
    Moved
}

pub(crate) struct Window<A: Application>{
    maverick: Option<MaverickOS<A>>,
    modifiers: Modifiers,
    touching: bool,
    scroll: Option<(f32, f32)>,
    mouse: (f32, f32),
}
impl<A: Application> Default for Window<A> {fn default() -> Self {Window{
    maverick: None,
    modifiers: Modifiers::default(),
    touching: false,
    scroll: None,
    mouse: (0.0, 0.0)
}}}

impl<A: Application> Window<A> {
    #[cfg(target_os = "android")]
    pub fn start(app: AndroidApp) {
        EventLoop::builder().with_android_app(app).build().unwrap().run_app(&mut Self::default()).unwrap();
    }

    #[cfg(target_arch = "wasm32")]
    pub fn start() {
        EventLoop::new().unwrap().spawn_app(Self::default()).unwrap();
    }

    #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
    pub fn start() {
        EventLoop::new().unwrap().run_app(&mut Self::default()).unwrap();
    }
}
impl<A: Application> ApplicationHandler for Window<A> {
    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        if let Some(maverick) = self.maverick.as_mut() && let StartCause::ResumeTimeReached{..} = cause {
            maverick.surface.request_redraw()
        }
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(maverick) = self.maverick.as_mut() {
            //maverick.runtime.pause();
            maverick.surface.suspend();
        }
    }

    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _device_id: DeviceId, event: DeviceEvent) {
        log::debug!("Ignored Device Event: {:?}", event);
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {match &mut self.maverick {
        Some(maverick) => {
            //maverick.runtime.resume();
            maverick.surface.resurface(&maverick.context.window);
        },
        none => {
            let window = event_loop.create_window(WinitWindow::default_attributes().with_title("orange")).unwrap();
            let context = Context::new(&window);
            let surface = Surface::new(window, &context);
            *none = Some(MaverickOS::new(context, surface));
        }
    }}

    fn memory_warning(&mut self, _event_loop: &ActiveEventLoop) {
        log::warn!("Memory Warning");
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if let Some(maverick) = self.maverick.as_mut() && id == maverick.surface.id() && let Some(event) = match event {
            WindowEvent::CloseRequested | WindowEvent::Destroyed => {
                let maverick = self.maverick.take().unwrap();
                maverick.runtime.shutdown();
                event_loop.exit();
                return;
            },
            WindowEvent::RedrawRequested => {
                event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now()+TICK));
                maverick.app.on_input(&maverick.context, Input::Tick);
                
                for event in maverick.context.hardware.tick() {
                    maverick.app.on_input(&maverick.context, event);
                }
                if let Some(surface) = maverick.surface.as_mut() {
                    surface.draw(&maverick.context.window, &maverick.app);
                } else {log::warn!("Redraw Requested Without A Valid Surface");}
                None
            },
            WindowEvent::Resized(size) => {
                maverick.context.window.width = size.width;
                maverick.context.window.height = size.height;
                if let Some(surface) = maverick.surface.as_mut() {
                    surface.resize(&maverick.context.window);
                } else {log::warn!("Resize Requested Without A Valid Surface");}
                Some(Input::Resized)
            },
            WindowEvent::ScaleFactorChanged{scale_factor, ..} => {
                maverick.context.window.scale_factor = scale_factor;
                if let Some(surface) = maverick.surface.as_mut() {
                    surface.resize(&maverick.context.window);
                } else {log::warn!("Resize Requested Without A Valid Surface");}
                Some(Input::Resized)
            },
            WindowEvent::Focused(focused) => Some(Input::WindowFocused(focused)),
            WindowEvent::KeyboardInput{device_id, event, ..} => match event.logical_key {
                WinitKey::Named(named) => match named {
                    WinitNamedKey::Enter => Some(Key::Enter),
                    WinitNamedKey::Tab => Some(Key::Tab),
                    WinitNamedKey::Space => Some(Key::Space),
                    WinitNamedKey::Escape => Some(Key::Escape),
                    WinitNamedKey::ArrowUp => Some(Key::Up),
                    WinitNamedKey::ArrowDown => Some(Key::Down),
                    WinitNamedKey::ArrowLeft => Some(Key::Left),
                    WinitNamedKey::ArrowRight => Some(Key::Right),
                    WinitNamedKey::Delete => Some(Key::Delete),
                    WinitNamedKey::Backspace => Some(Key::Backspace),
                    WinitNamedKey::Home => Some(Key::Home),
                    WinitNamedKey::End => Some(Key::End),
                    WinitNamedKey::Shift => Some(Key::Shift),
                    WinitNamedKey::Control => Some(Key::Control),
                    WinitNamedKey::Alt => Some(Key::Alt),
                    WinitNamedKey::Super => Some(Key::SuperMeta),
                    WinitNamedKey::Meta => Some(Key::SuperMeta),
                    WinitNamedKey::CapsLock => Some(Key::CapsLock),
                    WinitNamedKey::NumLock => Some(Key::NumLock),
                    WinitNamedKey::ScrollLock => Some(Key::ScrollLock),
                    _ => None,
                },
                WinitKey::Character(c) => Some(Key::Character(c.chars().next().unwrap())),
                _ => None,
            }.map(|key| Input::Device(device_id, DeviceInput::Keyboard(key, match event.state {
                ElementState::Pressed if event.repeat => KeyboardState::Repeated,
                ElementState::Pressed => KeyboardState::Pressed,
                ElementState::Released => KeyboardState::Released,
            }, self.modifiers))),
            WindowEvent::CursorMoved{device_id, position} if self.mouse != position.into() => {
                self.mouse = position.into();
                Some(Input::Device(device_id, DeviceInput::Mouse(self.mouse, MouseState::Moved)))
            },
            WindowEvent::MouseWheel{device_id, delta, phase} => match delta {
                MouseScrollDelta::LineDelta(x, y) => {
                    let shift = self.modifiers.shift;
                    let (dx, dy) = if shift {(-y, -x)} else {(-x, -y)};
                    (dx.abs() > 0.001 || dy.abs() > 0.001).then_some(
                        Input::Device(device_id, DeviceInput::Mouse(self.mouse, MouseState::Scroll(dx, dy)))
                    )
                }
                MouseScrollDelta::PixelDelta(p) => {
                    match phase {
                        TouchPhase::Started => {
                            self.scroll = Some((0.0, 0.0));
                            Some(Input::Device(device_id, DeviceInput::Mouse(self.mouse, MouseState::Scroll(0.0, 0.0))))
                        }
                        TouchPhase::Moved => {
                            let dx = -(p.x as f32);
                            let dy = -(p.y as f32);
                            (dx.abs() > 0.001 || dy.abs() > 0.001).then_some(
                                Input::Device(device_id, DeviceInput::Mouse(self.mouse, MouseState::Scroll(dx, dy)))
                            )
                        }
                        TouchPhase::Ended | TouchPhase::Cancelled => {
                            self.scroll = None;
                            None
                        }
                    }
                }
            },
            WindowEvent::MouseInput{device_id, state, button} => {
                let button = match button {
                    WinitMouseButton::Right => MouseButton::Right,
                    WinitMouseButton::Middle => MouseButton::Middle,
                    _ => MouseButton::Left,
                };
                let state = match state {
                    ElementState::Pressed => MouseState::Pressed(button),
                    ElementState::Released => MouseState::Released(button),
                };
                Some(Input::Device(device_id, DeviceInput::Mouse(self.mouse, state)))
            },
            WindowEvent::Touch(touch) => {
                let position = (touch.location.x as f32, touch.location.y as f32);
                self.mouse = position;
                match touch.phase {
                    TouchPhase::Started => {
                        self.scroll  = Some(position);
                        self.touching = true;
                        Some(Input::Device(DeviceId::dummy(), DeviceInput::Mouse(self.mouse, MouseState::Pressed(MouseButton::Left))))
                    }
                    TouchPhase::Ended | TouchPhase::Cancelled => {
                        self.touching = false;
                        Some(Input::Device(DeviceId::dummy(), DeviceInput::Mouse(self.mouse, MouseState::Released(MouseButton::Left))))
                    }
                    TouchPhase::Moved => {
                        self.scroll.and_then(|(prev_x, prev_y)| {
                            self.scroll = Some(position);
                            let dx = position.0 - prev_x;
                            let dy = position.1 - prev_y;
                            let scroll_x = -dx;
                            let scroll_y = -dy;
                            (scroll_x.abs() > 0.01 || scroll_y.abs() > 0.01).then_some(
                                Input::Device(DeviceId::dummy(), DeviceInput::Mouse(self.mouse, MouseState::Scroll(scroll_x, scroll_y)))
                            )
                        })
                    }
                }
            },
            WindowEvent::DroppedFile(path) => Some(Input::DroppedFile(path)),
            WindowEvent::HoveredFile(path) => Some(Input::HoveredFile(path)),
            WindowEvent::HoveredFileCancelled => Some(Input::HoveredFileCancelled),
            WindowEvent::ModifiersChanged(mods) => {
                self.modifiers = Modifiers{
                    shift: mods.state().shift_key(),
                    control: mods.state().control_key(),
                    alt: mods.state().alt_key(),
                    supermeta: mods.state().super_key(),
                };
                None
            },
            WindowEvent::CursorEntered{device_id} => Some(Input::Device(device_id, DeviceInput::CursorEntered)),
            WindowEvent::CursorLeft{device_id} => Some(Input::Device(device_id, DeviceInput::CursorLeft)),
            WindowEvent::Moved(position) => Some(Input::WindowMoved(position.x, position.y)),
            e => {log::debug!("Ignored Event: {:?}", e); return;}
        } {
            maverick.app.on_input(&maverick.context, event);
        }
    }
}
