use std::collections::BTreeMap;
use std::future::Future;
use std::any::TypeId;

#[cfg(target_os = "android")]
use winit::platform::android::activity::AndroidApp;

mod state;
pub use state::State;

pub mod hardware;
use crate::hardware::ApplicationSupport;
pub use crate::hardware::Cache;
pub use crate::hardware::PhotoPicker;
pub use crate::hardware::ImageOrientation;
pub use crate::hardware::Camera;
pub use crate::hardware::CameraError;

pub mod runtime;
use runtime::{Runtime, Services, ThreadConstructor, Service};

pub mod window;
use window::{WindowManager, EventHandler, Event, Lifetime};

pub mod prelude {
    pub use crate::{MaverickOS, Application, start};
}

pub mod air;
pub use air::Id;

pub trait Application: Services {
    fn new(context: &mut Context) -> impl Future<Output = Self>;
    fn on_event(&mut self, context: &mut Context, event: Event) -> impl Future<Output = ()>;
}

pub struct Context {
    pub state: Option<State>,
    pub window: window::Context,
    pub runtime: runtime::Context,
    pub hardware: hardware::Context,
}

//TODO: Need seperate cache for OS level
//TODO: All cloud access needs to go through the OS
pub struct MaverickOS<A: Application> {
    context: Context,
    services: BTreeMap<TypeId, ThreadConstructor>,
    app: Option<A>
}

impl<A: Application + 'static> MaverickOS<A> {
    pub fn start(
        #[cfg(target_os = "android")]
        app: AndroidApp
    ) {
        let mut hardware = hardware::Context::new();
        let runtime = Runtime::start(hardware.clone());


        let mut services = BTreeMap::new();
        let mut background_tasks = BTreeMap::new();
        let mut pre = A::services().0;
        while let Some((id, (constructor, backgrounds, deps))) = pre.pop_first() {
            services.entry(id).or_insert_with(|| {
                for (id, background) in backgrounds.0 {
                    background_tasks.insert(id, background);
                }
                pre.extend(deps().0);
                constructor
            });
        }

        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
        if std::env::args().len() > 1 {
            runtime.background(&mut hardware, background_tasks.into_values().collect());
            return
        }

        WindowManager::start(
            #[cfg(target_os = "android")]
            app,
            MaverickService::<A>::new(runtime, services, hardware)
        )
    }

    fn new(services: BTreeMap<TypeId, ThreadConstructor>, context: Context) -> Self {
        MaverickOS::<A>{context, services, app: None}
    }

    async fn on_event(&mut self, event: Event) {
        if self.app.is_none() {
            self.context.runtime.spawn(air::Service::new(&mut self.context.hardware).await);
            for service in self.services.values() {
                self.context.runtime.spawn(service(&mut self.context.hardware).await);
            }
            self.app = Some(A::new(&mut self.context).await);
        }
        self.app.as_mut().unwrap().on_event(&mut self.context, event).await;
    }
}

struct MaverickService<A: Application> {
    runtime: Option<Runtime>,
    services: Option<BTreeMap<TypeId, ThreadConstructor>>,
    hardware: Option<hardware::Context>,
    os: Option<MaverickOS::<A>>
}
impl<A: Application> MaverickService<A> {
    fn new(runtime: Runtime, services: BTreeMap<TypeId, ThreadConstructor>, hardware: hardware::Context) -> Self {
        MaverickService{runtime: Some(runtime), services: Some(services), hardware: Some(hardware), os: None}
    }
}

impl<A: Application + 'static> EventHandler for MaverickService<A> {
    fn event(&mut self, window_ctx: &window::Context, event: Event) {
        if let Some(runtime) = self.runtime.as_mut() {
            if self.os.is_none() {
                self.os = Some(MaverickOS::new(self.services.take().unwrap(), Context{
                    hardware: self.hardware.take().unwrap(),
                    runtime: runtime.context().clone(),
                    window: window_ctx.clone(),
                    state: Some(State::default())
                }))
            }
            self.os.as_mut().map(|a| {
                runtime.tick(a.context.state.as_mut().unwrap())
            });

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
        pub fn maverick_main() {
            MaverickOS::<$app>::start()
        }

        #[cfg(target_os = "ios")]
        #[unsafe(no_mangle)]
        pub extern "C" fn maverick_main() {
            MaverickOS::<$app>::start()
        }

        #[cfg(target_os = "android")]
        #[no_mangle]
        pub fn maverick_main(app: AndroidApp) {
            MaverickOS::<$app>::start(app)
        }

        #[cfg(not(any(target_os = "android", target_os="ios", target_arch = "wasm32")))]
        pub fn maverick_main() {
            MaverickOS::<$app>::start()
        }
    };
}
