mod hardware;

mod window;

mod events;
use events::{Events, Event};

mod runtime;
use runtime::{Runtime, RUNTIME};

mod cache;
use cache::Cache;

mod shared;

pub mod air;

#[cfg(target_os = "android")]
use winit::platform::android::activity::AndroidApp;

use winit::window::WindowId;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent, DeviceEvent, DeviceId};

#[cfg(target_os="android")]
use winit::platform::android::EventLoopBuilderExtAndroid;

use predator::{Application, Hardware, Renderer, Window, Air, Context, Input, traits::Window as _};

use std::time::Duration;

const TICK: Duration = Duration::from_millis(16);//60 fps

struct App<A: Application> {
    context: Context,
    events: Events,
    window: window::Window,
    renderer: Option<A::Renderer>,
    app: A,
}

impl<A: Application> App<A> {
    fn new(window: window::Window) -> Self {
        Runtime::start();
        let hardware = Hardware::new(hardware::Hardware::new());

        let mut s = Cache::new("secret").unwrap();
        let secret = s.get("secret").unwrap().unwrap_or_else(air::Secret::new);
        s.insert("secret", &secret).unwrap();
        let air = Air::new::<A, _>(air::Air::new(secret));
        
        let mut context = Context{hardware, window: window.logical(), air};
        let app = A::new(&mut context);
        let renderer = Some(A::Renderer::new(Window::new(window.clone())));
        App{context, window, events: Events::default(), renderer, app}
    }
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
            app.renderer = Some(A::Renderer::new(Window::new(app.window.clone())));
        },
        none => {
            let window = window::Window::new(event_loop, A::name());
            *none = Some(App::new(window))
        }
    }}

    fn memory_warning(&mut self, _event_loop: &ActiveEventLoop) {
        log::warn!("Memory Warning");
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if let Some(app) = self.0.as_mut() && id == app.window.id() && let Some(event) = app.events.process(event) {match event {
            Event::Close => {
                RUNTIME.get().unwrap().clone().shutdown();
                event_loop.exit();
            }
            Event::Draw => {
                event_loop.set_control_flow(ControlFlow::WaitUntil(std::time::Instant::now()+TICK));
                let instructions = app.app.on_tick(&mut app.context);
                if let Some(renderer) = app.renderer.as_mut() {
                    renderer.draw(instructions);
                } else {log::warn!("Redraw Requested Without A Valid Surface");}
            }
            Event::Resize => {
                if let Some(renderer) = app.renderer.as_mut() {
                    renderer.resize();
                } else {log::warn!("Resize Requested Without A Valid Surface");}
                app.app.on_input(&mut app.context, Input::Resized);
            }
            Event::Input(input) => {
                app.app.on_input(&mut app.context, input);
            }
        }}
    }
}


pub mod __private {
    #[cfg(target_os = "android")]
    pub use winit::platform::android::activity::AndroidApp;
    pub use crate::MaverickOS;
}

#[macro_export]
macro_rules! start {
    ($app:ty) => {
        #[cfg(target_arch = "wasm32")]
        #[cfg_attr(target_arch = "wasm32", wasm_bindgen(start))]
        pub fn maverick_main() {
            $crate::__private::MaverickOS::<$app>::start()
        }

        #[cfg(target_os = "ios")]
        #[unsafe(no_mangle)]
        pub extern "C" fn maverick_main() {
            $crate::__private::MaverickOS::<$app>::start()
        }

        #[cfg(target_os = "android")]
        #[unsafe(no_mangle)]
        pub fn android_main(app: $crate::__private::AndroidApp) {
            $crate::__private::MaverickOS::<$app>::start(app)
        }

        #[cfg(not(any(target_os = "android", target_os="ios", target_arch = "wasm32")))]
        pub fn maverick_main() {
            $crate::__private::MaverickOS::<$app>::start()
        }
    };
}
