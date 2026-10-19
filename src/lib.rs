mod hardware;
pub use hardware::{
    Hardware, Camera, PickResult, Pickable, Picked, Cancled, Media, Data, Image, Video, Player,
};

mod window;
pub use window::{Window, PhysicalWindow, LogicalWindow};

pub mod inputs;
use inputs::{Inputs, OSInput, Input};

mod runtime;
use runtime::{Runtime, RUNTIME};

pub mod shared;

#[cfg(target_os = "android")]
pub use winit::platform::android::activity::AndroidApp;

use winit::window::WindowId;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent, DeviceEvent, DeviceId};

#[cfg(target_os="android")]
use winit::platform::android::EventLoopBuilderExtAndroid;

use std::time::Duration;

const TICK: Duration = Duration::from_millis(16);//60 fps

pub trait Application: 'static {
    type Renderer: Renderer;

    fn name() -> String;

    fn new(hardware: Hardware, window: LogicalWindow) -> Self;

    fn on_input(&mut self, input: Input);

    fn on_tick(&mut self) -> <Self::Renderer as Renderer>::Instructions;
}

pub trait Renderer {
    type Instructions: 'static;

    fn new(window: Window) -> Self;
    fn resize(&mut self);
    fn draw(&mut self, instructions: Self::Instructions);
}

pub struct MaverickOS<A: Application>(Option<App<A>>);
impl<A: Application> MaverickOS<A> {
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

impl<A: Application> ApplicationHandler for MaverickOS<A> {
    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        if let Some(app) = self.0.as_mut() && let StartCause::ResumeTimeReached{..} = cause {
            app.window.request_redraw()
        }
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(app) = self.0.as_mut() {
            app.renderer = None;
        }
    }

    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _device_id: DeviceId, event: DeviceEvent) {
        log::debug!("Ignored Device Event: {:?}", event);
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {match &mut self.0 {
        Some(app) => {
            app.renderer = Some(A::Renderer::new(app.window.clone()));
            app.app.on_input(Input::Resized(app.window.logical()));
        },
        none => {
            let window = window::Window::new(event_loop, A::name());
            *none = Some(App::new(window))
        }
    }}

    fn memory_warning(&mut self, _event_loop: &ActiveEventLoop) {
        log::warn!("Memory Warning");
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(app) = self.0.take() {
            drop(app);
            RUNTIME.get().unwrap().clone().shutdown();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(app) = self.0.as_mut() else {return};
        let scale = app.window.physical().scale_factor;
        if id == app.window.id() && let Some(event) = app.inputs.process(event, scale) {match event {
            OSInput::Close => event_loop.exit(),
            OSInput::Draw => {
                event_loop.set_control_flow(ControlFlow::WaitUntil(std::time::Instant::now()+TICK));
                let instructions = app.app.on_tick();
                if let Some(renderer) = app.renderer.as_mut() {
                    renderer.draw(instructions);
                } else {log::warn!("Redraw Requested Without A Valid Surface");}
            }
            OSInput::Resize => {
                if let Some(renderer) = app.renderer.as_mut() {
                    renderer.resize();
                } else {log::warn!("Resize Requested Without A Valid Surface");}
                app.app.on_input(Input::Resized(app.window.logical()));
            }
            OSInput::Input(input) => {
                app.app.on_input(input);
            }
        }}
    }
}

#[macro_export]
macro_rules! start {
    ($app:ty) => {
        #[cfg(target_arch = "wasm32")]
        #[cfg_attr(target_arch = "wasm32", wasm_bindgen(start))]
        pub fn maverick_main() {
            $crate::MaverickOS::<$app>::start()
        }

        #[cfg(target_os = "ios")]
        #[unsafe(no_mangle)]
        pub extern "C" fn maverick_main() {
            $crate::MaverickOS::<$app>::start()
        }

        #[cfg(target_os = "android")]
        #[unsafe(no_mangle)]
        pub fn android_main(app: $crate::AndroidApp) {
            $crate::MaverickOS::<$app>::start(app)
        }

        #[cfg(not(any(target_os = "android", target_os="ios", target_arch = "wasm32")))]
        pub fn main() {
            $crate::MaverickOS::<$app>::start()
        }
    };
}

struct App<A: Application> {
    _hardware: Hardware,
    inputs: Inputs,
    window: window::Window,
    renderer: Option<A::Renderer>,
    app: A,
}

impl<A: Application> App<A> {
    fn new(window: window::Window) -> Self {
        Runtime::start();
        let _hardware = hardware::Hardware::new(A::name());

        let app = A::new(_hardware.clone(), window.logical());
        let renderer = Some(A::Renderer::new(window.clone()));
        App{_hardware, window, inputs: Inputs::default(), renderer, app}
    }
}
