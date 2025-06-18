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

pub use winit::keyboard::{NamedKey, SmolStr, Key};
pub use winit::window::Window;

const TICK: Duration = Duration::from_millis(16);//60 fps

///Window Context contains window information and its handle, The context is cheaply clonable but
///does not get remotely updated each resume/resize event creates a new window Context
#[derive(Clone)]
pub struct Context {
    pub scale_factor: f64,
    pub size: (u32, u32),
    pub handle: Arc<Window>,
}
impl Context {fn from_window(window: Arc<Window>) -> Self {Context{
    size: window.inner_size().into(),
    scale_factor: window.scale_factor(),
    handle: window
}}}

#[derive(Clone, Debug)]
pub enum Event {
    Lifetime(Lifetime),
    Input(Input),
}

#[derive(Clone, Debug)]
pub enum Lifetime {
    Resized,
    ///Window was created and is ready for the first frame
    Resumed,
    ///App was paused, Create one last frame and render it before the event ends and destroy window
    Paused,
    ///App is being closed and the window is or has been destroyed render no more
    Close,
    ///Equivelent to a tick draw a frame
    Draw,
    ///On mobile this app will be terminiated if it does not free some memory
    MemoryWarning,
}

#[derive(Clone, Debug)]
pub enum Input {
    Focused(bool),
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

pub trait EventHandler {
    fn event(&mut self, ctx: &Context, event: Event);
}

pub struct WindowManager<E: EventHandler + 'static> {
    context: Option<Context>,
    event_handler: E,
    pause: bool
}

impl<E: EventHandler> WindowManager<E> {
    pub fn start(
        #[cfg(target_os = "android")]
        app: AndroidApp,
        event_handler: E
    ) {
        WindowManager{context: None, event_handler, pause: false}.start_loop(
            #[cfg(target_os = "android")]
            app
        )
    }

    #[cfg(target_os = "android")]
    fn start_loop(mut self, app: AndroidApp) {
        let event_loop = EventLoop::builder().with_android_app(app).build().unwrap();
        event_loop.run_app(&mut self).unwrap();
    }

    #[cfg(target_arch = "wasm32")]
    fn start_loop(mut self) {
        let event_loop = EventLoop::new().unwrap();
        //event_loop.set_control_flow(ControlFlow::Poll);
        event_loop.run_app(self).unwrap();
    }

    #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
    fn start_loop(mut self) {
        let event_loop = EventLoop::new().unwrap();
        event_loop.run_app(&mut self).unwrap();
    }
}

impl<E: EventHandler> ApplicationHandler for WindowManager<E> {
    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        if let StartCause::ResumeTimeReached{..} = cause {
            if let Some(context) = &self.context { context.handle.request_redraw(); }
        }
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        if !self.pause {
            if let Some(context) = &self.context {
                self.pause = true;
                self.event_handler.event(context, Event::Lifetime(Lifetime::Paused));
            }
        }
    }

    fn device_event(&mut self, _event_loop: &ActiveEventLoop, device_id: DeviceId, event: DeviceEvent) {
        if !self.pause {
            if let Some(context) = &self.context {
                self.event_handler.event(context, Event::Input(Input::Device{device_id, event}));
            }
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = Arc::new(event_loop.create_window(
            Window::default_attributes().with_title("orange")
        ).unwrap());
        let context = Context::from_window(window);
        self.event_handler.event(&context, Event::Lifetime(Lifetime::Resumed));
        self.context = Some(context);
        self.pause = false;
    }

    fn memory_warning(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(context) = &mut self.context {
            self.event_handler.event(context, Event::Lifetime(Lifetime::MemoryWarning));
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, i: WindowId, event: WindowEvent) {
        if let Some(context) = &mut self.context {
            if i == context.handle.id() && (!self.pause || matches!(event, WindowEvent::Occluded(false))) {
                let event = match event {
                    WindowEvent::CloseRequested | WindowEvent::Destroyed => {
                        event_loop.exit();
                        Event::Lifetime(Lifetime::Close)
                    },
                    WindowEvent::RedrawRequested => {
                        println!("drawing---------------------");
                        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now()+TICK));
                        Event::Lifetime(Lifetime::Draw)
                    },
                    WindowEvent::Occluded(occluded) => {
                        if occluded {
                            self.pause = true;
                            Event::Lifetime(Lifetime::Paused)
                        } else {
                            self.pause = false;
                            //Only on IOS is this called and it is prior to an actual Resume event
                            Event::Lifetime(Lifetime::Resumed)
                        }
                    },
                    WindowEvent::Resized(size) => {
                        context.size = size.into();
                        Event::Lifetime(Lifetime::Resized)
                    },
                    WindowEvent::ScaleFactorChanged{scale_factor, ..} => {
                        context.scale_factor = scale_factor;
                        Event::Lifetime(Lifetime::Resized)
                    },
                    WindowEvent::Focused(focused) => Event::Input(Input::Focused(focused)),
                    WindowEvent::KeyboardInput{device_id, event, is_synthetic} => Event::Input(Input::Keyboard{device_id, event, is_synthetic}),
                    WindowEvent::CursorMoved{device_id, position} => Event::Input(Input::CursorMoved{device_id, position: position.into()}),
                    WindowEvent::MouseWheel{device_id, delta, phase} => Event::Input(Input::MouseWheel{device_id, delta, phase}),
                    WindowEvent::MouseInput{device_id, state, button} => Event::Input(Input::Mouse{device_id, state, button}),
                    WindowEvent::Touch(touch) => Event::Input(Input::Touch(touch)),
                    WindowEvent::DroppedFile(path) => Event::Input(Input::DroppedFile(path)),
                    WindowEvent::HoveredFile(path) => Event::Input(Input::HoveredFile(path)),
                    WindowEvent::HoveredFileCancelled => Event::Input(Input::HoveredFileCancelled),
                    WindowEvent::ModifiersChanged(modifiers) => Event::Input(Input::ModifiersChanged(modifiers)),
                    WindowEvent::CursorEntered{device_id} => Event::Input(Input::CursorEntered{device_id}),
                    WindowEvent::CursorLeft{device_id} => Event::Input(Input::CursorLeft{device_id}),
                    WindowEvent::PinchGesture{device_id, delta, phase} => Event::Input(Input::PinchGesture{device_id, delta, phase}),
                    WindowEvent::PanGesture{device_id, delta, phase} => Event::Input(Input::PanGesture{device_id, delta: delta.into(), phase}),
                    WindowEvent::DoubleTapGesture{device_id} => Event::Input(Input::DoubleTapGesture{device_id}),
                    WindowEvent::RotationGesture{device_id, delta, phase} => Event::Input(Input::RotationGesture{device_id, delta, phase}),
                    WindowEvent::TouchpadPressure{device_id, pressure, stage} => Event::Input(Input::TouchpadPressure{device_id, pressure, stage}),
                    WindowEvent::AxisMotion{device_id, axis, value} => Event::Input(Input::AxisMotion{device_id, axis, value}),
                    WindowEvent::Moved(position) => Event::Input(Input::Moved(position.into())),
                    _ => {return;}
                };
                self.event_handler.event(context, event);
            }
        }
    }
}
