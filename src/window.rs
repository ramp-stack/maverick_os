use winit::window::WindowId;
pub use winit::event::{WindowEvent, DeviceEvent, DeviceId, TouchPhase, Touch, AxisId, MouseButton, MouseScrollDelta, Modifiers, ElementState, KeyEvent};
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

pub use winit::keyboard::{NamedKey, SmolStr, Key};
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
    Focused(bool),
    CameraFrame(RgbaImage),
    PickedPhoto(RgbaImage),
    DroppedFile(PathBuf),
    HoveredFile(PathBuf),
    HoveredFileCancelled,
    Keyboard{device_id: DeviceId, event: KeyEvent, is_synthetic: bool},
    ModifiersChanged(Modifiers),
    CursorMoved{device_id: DeviceId, position: (f64, f64)},
    CursorEntered{device_id: DeviceId},
    CursorLeft{device_id: DeviceId},
    MouseWheel{device_id: DeviceId, delta: MouseScrollDelta, phase: TouchPhase},
    Mouse{device_id: DeviceId, state: ElementState, button: MouseButton},
    PinchGesture{device_id: DeviceId, delta: f64, phase: TouchPhase},
    PanGesture{device_id: DeviceId, delta: (f32, f32), phase: TouchPhase},
    DoubleTapGesture{device_id: DeviceId},
    RotationGesture{device_id: DeviceId, delta: f32, phase: TouchPhase},
    TouchpadPressure{device_id: DeviceId, pressure: f32, stage: i64},
    AxisMotion{device_id: DeviceId, axis: AxisId, value: f64},
    Moved((i32, i32)),
    Touch(Touch),
    Device{device_id: DeviceId, event: DeviceEvent},
}

pub(crate) struct Window<A: Application>(Option<MaverickOS<A>>);
impl<A: Application> Window<A> {
    #[cfg(target_os = "android")]
    pub fn start(app: AndroidApp) {
        EventLoop::builder().with_android_app(app).build().unwrap().run_app(&mut Self(None)).unwrap();
    }

    #[cfg(target_arch = "wasm32")]
    pub fn start() {
        EventLoop::new().unwrap().spawn_app(Self(None)).unwrap();
    }

    #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
    pub fn start() {
        EventLoop::new().unwrap().run_app(&mut Self(None)).unwrap();
    }
}
impl<A: Application> ApplicationHandler for Window<A> {
    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        if let Some(maverick) = self.0.as_mut() && let StartCause::ResumeTimeReached{..} = cause {
            maverick.surface.request_redraw()
        }
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(maverick) = self.0.as_mut() {
            maverick.runtime.pause();
            maverick.surface.suspend();
        }
    }

    fn device_event(&mut self, _event_loop: &ActiveEventLoop, device_id: DeviceId, event: DeviceEvent) {
        if let Some(maverick) = self.0.as_mut() {
            maverick.app.on_input(&mut maverick.context, Input::Device{device_id, event});
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {match &mut self.0 {
        Some(maverick) => {
            maverick.runtime.resume();
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
        if let Some(maverick) = self.0.as_mut() && id == maverick.surface.id() {
            let event = match event {
                WindowEvent::CloseRequested | WindowEvent::Destroyed => {
                    let maverick = self.0.take().unwrap();
                    maverick.runtime.shutdown();
                    event_loop.exit();
                    return;
                },
                WindowEvent::RedrawRequested => {
                    event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now()+TICK));
                    maverick.app.on_input(&mut maverick.context, Input::Tick);
                    
                    for event in maverick.context.hardware.tick() {
                        maverick.app.on_input(&mut maverick.context, event);
                    }
                    if let Some(surface) = maverick.surface.as_mut() {
                        surface.draw(&maverick.context.window, &maverick.app);
                    } else {log::warn!("Redraw Requested Without A Valid Surface");}
                    return;
                },
                WindowEvent::Occluded(true) => {
                    #[cfg(target_os = "ios")]
                    maverick.runtime.pause();
                    return;
                },
                WindowEvent::Resized(size) => {
                    maverick.context.window.width = size.width;
                    maverick.context.window.height = size.height;
                    if let Some(surface) = maverick.surface.as_mut() {
                        surface.resize(&maverick.context.window);
                    } else {log::warn!("Resize Requested Without A Valid Surface");}
                    Input::Resized
                },
                WindowEvent::ScaleFactorChanged{scale_factor, ..} => {
                    maverick.context.window.scale_factor = scale_factor;
                    if let Some(surface) = maverick.surface.as_mut() {
                        surface.resize(&maverick.context.window);
                    } else {log::warn!("Resize Requested Without A Valid Surface");}
                    Input::Resized
                },
                WindowEvent::Focused(focused) => Input::Focused(focused),
                WindowEvent::KeyboardInput{device_id, event, is_synthetic} => Input::Keyboard{device_id, event, is_synthetic},
                WindowEvent::CursorMoved{device_id, position} => Input::CursorMoved{device_id, position: position.into()},
                WindowEvent::MouseWheel{device_id, delta, phase} => Input::MouseWheel{device_id, delta, phase},
                WindowEvent::MouseInput{device_id, state, button} => Input::Mouse{device_id, state, button},
                WindowEvent::Touch(touch) => Input::Touch(touch),
                WindowEvent::DroppedFile(path) => Input::DroppedFile(path),
                WindowEvent::HoveredFile(path) => Input::HoveredFile(path),
                WindowEvent::HoveredFileCancelled => Input::HoveredFileCancelled,
                WindowEvent::ModifiersChanged(modifiers) => Input::ModifiersChanged(modifiers),
                WindowEvent::CursorEntered{device_id} => Input::CursorEntered{device_id},
                WindowEvent::CursorLeft{device_id} => Input::CursorLeft{device_id},
                WindowEvent::PinchGesture{device_id, delta, phase} => Input::PinchGesture{device_id, delta, phase},
                WindowEvent::PanGesture{device_id, delta, phase} => Input::PanGesture{device_id, delta: delta.into(), phase},
                WindowEvent::DoubleTapGesture{device_id} => Input::DoubleTapGesture{device_id},
                WindowEvent::RotationGesture{device_id, delta, phase} => Input::RotationGesture{device_id, delta, phase},
                WindowEvent::TouchpadPressure{device_id, pressure, stage} => Input::TouchpadPressure{device_id, pressure, stage},
                WindowEvent::AxisMotion{device_id, axis, value} => Input::AxisMotion{device_id, axis, value},
                WindowEvent::Moved(position) => Input::Moved(position.into()),
                e => {log::info!("Ignored Event: {:?}", e); return;}
            };
            maverick.app.on_input(&mut maverick.context, event);
        }
    }
}
