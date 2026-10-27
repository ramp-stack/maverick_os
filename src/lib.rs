use futures::{FutureExt, StreamExt};
use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use futures::future::LocalBoxFuture;
use winit::application::ApplicationHandler;
use winit::event::{WindowEvent, DeviceEvent, DeviceId};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::WindowId;

#[cfg(target_os = "android")]
pub use winit::platform::android::activity::AndroidApp;
#[cfg(target_os = "android")]
use winit::platform::android::EventLoopBuilderExtAndroid;

use std::sync::Arc;
use std::task::{Context as TaskContext, Wake, Waker};
use std::marker::PhantomData;

mod hardware;
pub use hardware::{
    Hardware, Camera, Microphone, Capture, Keyboard, PickResult, Pickable, Picked, Cancled, Media, Data,
    Image, ImageFormat, Pixel, Sample, Visual, Video, VideoFrame, Audio, AudioFrame, Sound,
    MediaFrame, Cursor, Encoder, Decoder, Tracks, Format,
    Frame, PlaybackMode, Player, Speaker, Playback, PushDestination, PushToken, Notify,
};

mod window;
pub use window::{Window, PhysicalWindow, LogicalWindow};

pub mod inputs;
use inputs::Input;

pub mod shared;

mod events;
use events::{Events, Output};

mod runtime;
use runtime::RUNTIME;

pub trait Application: 'static {
    type Renderer: Renderer;

    fn name() -> String;
    fn new(hardware: Hardware, window: LogicalWindow) -> Self;
    fn on_input(&mut self, input: Input);
    fn draw(&mut self) -> <Self::Renderer as Renderer>::Instructions;
    fn changed(&mut self) -> impl Future<Output = ()> {std::future::pending()}
}

pub trait Renderer: 'static {
    type Instructions: 'static;

    fn new(window: Window) -> Self;
    fn resize(&mut self);
    fn update(&mut self, instructions: Self::Instructions) -> bool;
    /// Returns true after submitting a displayable frame; false when rendering was skipped.
    fn draw(&mut self) -> bool;
}

pub struct MaverickOS<A: Application> {
    proxy: EventLoopProxy<Output>,
    driver: Option<Driver>,
    application: PhantomData<fn() -> A>,
}
impl<A: Application> MaverickOS<A> {
    #[cfg(target_os = "android")]
    pub fn start(app: AndroidApp) {
        let events = EventLoop::<Output>::with_user_event().with_android_app(app).build().unwrap();
        let proxy = events.create_proxy();
        events.run_app(&mut Self {proxy, driver: None, application: PhantomData}).unwrap();
    }

    #[cfg(target_arch = "wasm32")]
    pub fn start() {
        use winit::platform::web::EventLoopExtWebSys;
        let events = EventLoop::<Output>::with_user_event().build().unwrap();
        let proxy = events.create_proxy();
        events.spawn_app(Self {proxy, driver: None, application: PhantomData});
    }

    #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
    pub fn start() {
        let events = EventLoop::<Output>::with_user_event().build().unwrap();
        let proxy = events.create_proxy();
        events.run_app(&mut Self {proxy, driver: None, application: PhantomData}).unwrap();
    }

    fn send(&mut self, message: Message) {
        self.driver.as_mut().unwrap().messages.unbounded_send(message).unwrap();
        self.pump();
    }
    fn pump(&mut self) {
        let driver = self.driver.as_mut().unwrap();
        let waker = Waker::from(driver.wake.clone());
        assert!(driver.task.as_mut().poll(&mut TaskContext::from_waker(&waker)).is_pending());
    }
}
impl<A: Application> ApplicationHandler<Output> for MaverickOS<A> {
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: Output) {
        if self.driver.is_some() {
            match event {
                Output::Wake => self.pump(),
                Output::Input(input) => self.send(Message::Input(input)),
                Output::GeometryChanged => self.send(Message::GeometryChanged),
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::Wait);
    }

    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        if self.driver.is_some() {self.send(Message::Suspended);}
        event_loop.set_control_flow(ControlFlow::Wait);
    }

    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _device_id: DeviceId, event: DeviceEvent) {
        log::debug!("Ignored Device Event: {:?}", event);
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.driver.is_some() {self.send(Message::Resumed);} else {
            let (window, keyboard) = Window::new(event_loop, A::name(), self.proxy.clone());
            let (messages, receiver) = mpsc::unbounded();
            let task = Box::pin(App::<A>::new(window.clone(), self.proxy.clone(), keyboard).run(receiver));
            let wake = Arc::new(EventWake {proxy: self.proxy.clone()});
            self.driver = Some(Driver {task, messages, window, wake});
            self.pump();
        }
        event_loop.set_control_flow(ControlFlow::Wait);
    }

    fn memory_warning(&mut self, _event_loop: &ActiveEventLoop) {log::warn!("Memory Warning");}

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(driver) = self.driver.take() {
            drop(driver);
            RUNTIME.get().unwrap().clone().shutdown();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(driver) = &mut self.driver else {return};
        if id != driver.window.id() {return}
        if matches!(event, WindowEvent::CloseRequested | WindowEvent::Destroyed) {
            event_loop.exit();
            return
        }
        let scale = driver.window.physical().scale_factor;
        self.send(Message::Window(event, scale));
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

enum Message {Input(Input), Window(WindowEvent, f64), GeometryChanged, Suspended, Resumed}

struct Driver {
    task: LocalBoxFuture<'static, ()>,
    messages: UnboundedSender<Message>,
    window: Window,
    wake: Arc<EventWake>,
}

struct App<A: Application> {
    hardware: Hardware,
    events: Events,
    launch: hardware::LaunchScreen,
    window: Window,
    physical: PhysicalWindow,
    logical: LogicalWindow,
    keyboard_visible: bool,
    renderer: Option<A::Renderer>,
    redraw: bool,
    app: A,
}
impl<A: Application> App<A> {
    fn new(window: Window, proxy: EventLoopProxy<Output>, keyboard: hardware::Keyboard) -> Self {
        let launch = hardware::LaunchScreen::new(&window);
        let hardware = Hardware::new(A::name(), proxy.clone(), keyboard);
        let events = Events::new(&window, proxy);
        let physical = window.physical();
        let logical = window.logical();
        let keyboard_visible = hardware.keyboard().visible();
        let app = A::new(hardware.clone(), logical.clone());
        let renderer = Some(A::Renderer::new(window.clone()));
        Self {
            hardware, launch, window, physical, logical, keyboard_visible, events,
            renderer, redraw: false, app,
        }
    }
    async fn run(mut self, mut messages: UnboundedReceiver<Message>) {
        self.update();
        self.window.request_redraw();
        loop {
            if self.renderer.is_none() {
                let Some(message) = messages.next().await else {return};
                self.message(message);
                continue
            }
            futures::select_biased! {
                message = messages.next().fuse() => {
                    let Some(message) = message else {return};
                    self.message(message);
                },
                input = self.events.next().fuse() => self.input(input),
                _ = self.app.changed().fuse() => {
                    if self.update() {self.window.request_redraw();}
                },
            }
        }
    }
    fn message(&mut self, message: Message) {
        match message {
            Message::Suspended => {
                self.events.suspend();
                self.renderer = None;
                self.redraw = false;
            }
            Message::Resumed => {
                self.renderer = Some(A::Renderer::new(self.window.clone()));
                self.physical = self.window.physical();
                self.logical = self.window.logical();
                self.keyboard_visible = self.hardware.keyboard().visible();
                self.app.on_input(Input::Resized(self.logical.clone()));
                self.update();
                self.window.request_redraw();
            }
            Message::Window(event, scale) => match event {
                WindowEvent::RedrawRequested => {
                    if self.geometry_changed() {self.update();}
                    self.draw();
                }
                WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged{..} => {
                    let physical = self.window.physical();
                    if physical == self.physical && !self.geometry_changed() {return}
                    if physical != self.physical {
                        let _ = self.events.process(event, scale);
                        self.physical = physical;
                        if let Some(renderer) = &mut self.renderer {renderer.resize();}
                    }
                    self.update();
                    if self.renderer.is_some() {self.window.request_redraw();}
                }
                event => {
                    if let Some(input) = self.events.process(event, scale) {self.input(input);}
                }
            }
            Message::Input(input) => self.input(input),
            Message::GeometryChanged => {
                if self.geometry_changed() {self.update();}
                if self.redraw {self.draw();}
            }
        }
    }
    fn input(&mut self, input: Input) {
        let input = match input {
            Input::Resized(_) => {
                self.logical = self.window.logical();
                self.keyboard_visible = self.hardware.keyboard().visible();
                Input::Resized(self.logical.clone())
            }
            input => input,
        };
        self.app.on_input(input);
        if self.update() {self.window.request_redraw();}
    }
    fn draw(&mut self) {
        let Some(renderer) = &mut self.renderer else {return};
        if renderer.draw() {
            self.redraw = false;
            self.launch.dismiss();
        } else {self.redraw = true;}
    }
    fn update(&mut self) -> bool {
        let logical = self.window.logical();
        let keyboard_visible = self.hardware.keyboard().visible();
        if logical != self.logical || keyboard_visible != self.keyboard_visible {
            self.logical = logical;
            self.keyboard_visible = keyboard_visible;
            self.app.on_input(Input::Resized(self.logical.clone()));
        }
        self.redraw |= self.renderer.as_mut().is_some_and(|renderer| renderer.update(self.app.draw()));
        self.redraw
    }

    fn geometry_changed(&self) -> bool {
        self.window.logical() != self.logical
            || self.hardware.keyboard().visible() != self.keyboard_visible
    }
}

struct EventWake {
    proxy: EventLoopProxy<Output>,
}
impl Wake for EventWake {
    fn wake(self: Arc<Self>) {self.wake_by_ref();}
    fn wake_by_ref(self: &Arc<Self>) {let _ = self.proxy.send_event(Output::Wake);}
}
