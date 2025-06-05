use std::sync::{Mutex, Arc};
use std::future::Future;

mod state;
pub use state::{Field, State};

pub mod hardware;
pub use crate::hardware::{
    Clipboard,
    Cache,
    Share,
    ApplicationSupport,
    CloudStorage,
    {PhotoPicker, ImageOrientation},
    {Camera, CameraError},
};
pub mod runtime;
use runtime::{Runtime, Services};

pub mod window;
use window::{WindowManager, EventHandler, Event, Lifetime};

pub mod prelude {
    pub use crate::{MaverickOS, Application, start};
}

pub mod air;
//  use air::AirTask;

pub trait Application: Services {
    fn new(context: &mut Context) -> impl Future<Output = Self>;
    fn on_event(&mut self, context: &mut Context, event: Event) -> impl Future<Output = ()>;
}

pub struct Context {
    pub state: Arc<Mutex<State>>,
    pub window: window::Context,
    pub runtime: runtime::Context,
    pub hardware: hardware::Context,
}

pub struct MaverickOS<A: Application> {
    context: Context,
    app: Option<A>
}

impl<A: Application + 'static> MaverickOS<A> {
    pub fn start(
        #[cfg(target_os = "android")]
        app: AndroidApp
    ) {
        let hardware = hardware::Context::new();
        let runtime = Runtime::start::<A>(hardware.clone());
        WindowManager::start(
            #[cfg(target_os = "android")]
            app,
            MaverickService::<A>::new(runtime, hardware)
        )
    }

    fn new(context: Context) -> Self {
        MaverickOS::<A>{context, app: None}
    }

    ///Receiving the first Resume should trigger the MaverickOS new function accepting zero args
    async fn on_event(&mut self, event: Event) {
        if self.app.is_none() {
            self.app = Some(A::new(&mut self.context).await);
        }
        self.app.as_mut().unwrap().on_event(&mut self.context, event).await;
    }
}

struct MaverickService<A: Application> {
    state: Arc<Mutex<State>>,
    runtime: Option<Runtime>,
    hardware: Option<hardware::Context>,
    os: Option<MaverickOS::<A>>
}
impl<A: Application> MaverickService<A> {
    fn new(runtime: Runtime, hardware: hardware::Context) -> Self {
        MaverickService{runtime: Some(runtime), hardware: Some(hardware), state: Arc::new(Mutex::new(State::default())), os: None}
    }
}

impl<A: Application + 'static> EventHandler for MaverickService<A> {
    fn event(&mut self, window_ctx: &window::Context, event: Event) {
        if let Some(runtime) = self.runtime.as_mut() {
            runtime.tick(&mut self.state.lock().unwrap());
            if self.os.is_none() {
                self.os = Some(MaverickOS::new(Context{
                    hardware: self.hardware.take().unwrap(),
                    runtime: runtime.context().clone(),
                    window: window_ctx.clone(),
                    state: self.state.clone(),
                }))
            }
            let os = self.os.as_mut().unwrap();
            os.context.window = window_ctx.clone();
            runtime.block_on(os.on_event(event.clone()));
            match &event {
                Event::Lifetime(Lifetime::Paused) => runtime.pause(),
                Event::Lifetime(Lifetime::Resumed) => runtime.resume(),
                Event::Lifetime(Lifetime::Close) => self.runtime.take().unwrap().close(),
                _ => {}
            }
        }
    }
}

#[macro_export]
macro_rules! start {
    ($app:ty) => {
        #[cfg(target_arch = "wasm32")]
        #[cfg_attr(target_arch = "wasm32", wasm_bindgen(start))]
        pub fn wasm_main() {
            MaverickOS::<$app>::start()
        }

        #[cfg(target_os = "ios")]
        #[no_mangle]
        pub extern "C" fn ios_main() {
            MaverickOS::<$app>::start()
        }

        #[cfg(target_os = "android")]
        #[no_mangle]
        pub fn android_main(app: AndroidApp) {
            MaverickOS::<$app>::start(app)
        }

        #[cfg(not(any(target_os = "android", target_os="ios", target_arch = "wasm32")))]
        pub fn main() {
            MaverickOS::<$app>::start()
        }
    };
}
