use std::future::Future;
use std::sync::Arc;

use image::{RgbaImage, load_from_memory};

pub use include_dir::{Dir, include_dir};

pub mod hardware;
pub use hardware::Context as HardwareContext;

pub mod runtime;
pub use runtime::{Services, Context as RuntimeContext, ServiceList};

pub mod window;
use window::Event;

mod config;
pub use config::{IS_MOBILE, IS_WEB};

pub trait Application: Services {
    fn new(context: &mut Context, assets: Assets) -> impl Future<Output = Self>;
    fn on_event(&mut self, context: &mut Context, event: Event) -> impl Future<Output = ()>;
}

anyanymap::Map!(State: );

pub struct Context {
    state: Option<State>,
    pub window: window::Context,
    pub runtime: runtime::Context,
    pub hardware: hardware::Context,
}

#[derive(Clone, Debug)]
pub struct Assets {inner: Dir<'static>}

impl Assets {
    pub fn new(inner: Dir<'static>) -> Self { Self { inner } }

    pub fn all(&self) -> &Dir<'static> {&self.inner}

    pub fn get_image(&self, path: &str) -> Option<Arc<RgbaImage>> {
        let bytes = self.inner.get_file(path)?.contents().to_vec();
        Some(Arc::new(load_from_memory(&bytes).ok()?.to_rgba8()))
    }

    pub fn get_font(&self, path: &str) -> Option<Vec<u8>> {
        Some(self.inner.get_file(path)?.contents().to_vec())
    }

    pub fn get_svg(&self, path: &str) -> Option<Arc<RgbaImage>> {
        let svg = self.inner.get_file(path)?.contents().to_vec();
        let svg = std::str::from_utf8(&svg).unwrap();
        let svg = nsvg::parse_str(svg, nsvg::Units::Pixel, 96.0).unwrap();
        let rgba = svg.rasterize(8.0).unwrap();
        let size = rgba.dimensions();
        Some(Arc::new(RgbaImage::from_raw(size.0, size.1, rgba.into_raw()).unwrap()))
    }
}


pub mod __private {
    #[cfg(target_os = "android")]
    pub use winit::platform::android::activity::AndroidApp;

    pub use include_dir;

    use runtime::{Runtime, ThreadConstructor};
    use window::{WindowManager, EventHandler, Event, Lifetime};

    use crate::{Assets, Context, Application,  window, runtime, hardware, State};

    use std::collections::BTreeMap;
    use std::any::TypeId;
    // use crate::runtime::Service as AirService;
    //TODO: Need seperate cache for OS level
    //TODO: All cloud access needs to go through the OS
    pub struct MaverickOS<A: Application> {
        context: Context,
        services: BTreeMap<TypeId, ThreadConstructor>,
        app: Option<A>,
        assets: Assets,
        window_name: String,
    }

    impl<A: Application + 'static> MaverickOS<A> {
        pub fn start(
            #[cfg(target_os = "android")]
            app: AndroidApp,
            window_name: &str,
            assets: include_dir::Dir<'static>,
        ) {

            #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
            let mut hardware = hardware::Context::new();

            #[cfg(any(target_os = "ios", target_os = "android"))]
            let hardware = hardware::Context::new();
            
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
                MaverickService::<A>::new(runtime, services, hardware, Assets::new(assets), window_name.to_string()),
                window_name.to_string()
            )
        }

        fn new(services: BTreeMap<TypeId, ThreadConstructor>, context: Context, assets: Assets, window_name: String) -> Self {
            MaverickOS::<A>{context, services, app: None, assets, window_name}
        }

        async fn on_event(&mut self, event: Event) {
            if self.app.is_none() {
                for service in self.services.values() {
                    self.context.runtime.spawn(service(&mut self.context.hardware).await);
                }
                self.app = Some(A::new(&mut self.context, self.assets.clone()).await);
            }
            self.app.as_mut().unwrap().on_event(&mut self.context, event).await;
        }
    }

    pub struct MaverickService<A: Application> {
        runtime: Option<Runtime>,
        services: Option<BTreeMap<TypeId, ThreadConstructor>>,
        hardware: Option<hardware::Context>,
        os: Option<MaverickOS::<A>>,
        assets: Assets,
        window_name: String,
    }

    impl<A: Application> MaverickService<A> {
        fn new(runtime: Runtime, services: BTreeMap<TypeId, ThreadConstructor>, hardware: hardware::Context, assets: Assets, window_name: String) -> Self {
            MaverickService{runtime: Some(runtime), services: Some(services), hardware: Some(hardware), os: None, assets, window_name}
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
                        state: Some(State::default()),
                    }, self.assets.clone(), self.window_name.clone()))
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
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
#[link(name = "PhotosUI", kind = "framework")]
unsafe extern "C" {}

#[cfg(target_os = "macos")]
#[link(name = "Cocoa", kind = "framework")]
unsafe extern "C" {}

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {}

#[cfg(target_os = "macos")]
#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {}

#[cfg(target_os = "macos")]
#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {}

#[cfg(target_os = "ios")]
#[link(name = "UIKit", kind = "framework")]
unsafe extern "C" {}

#[cfg(any(target_os = "ios", target_os = "macos"))]
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {}

#[cfg(any(target_os = "ios", target_os = "macos"))]
#[link(name = "Metal", kind = "framework")]
unsafe extern "C" {}

#[cfg(any(target_os = "ios", target_os = "macos"))]
#[link(name = "CoreVideo", kind = "framework")]
unsafe extern "C" {}

#[cfg(any(target_os = "ios", target_os = "macos"))]
#[link(name = "CoreMedia", kind = "framework")]
unsafe extern "C" {}

#[cfg(any(target_os = "ios", target_os = "macos"))]
#[link(name = "AVKit", kind = "framework")]
unsafe extern "C" {}

#[cfg(any(target_os = "ios", target_os = "macos"))]
#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {}

#[cfg(any(target_os = "ios", target_os = "macos"))]#[link(name = "Security", kind = "framework")]
unsafe extern "C" {}

#[cfg(any(target_os = "ios", target_os = "macos"))]
#[link(name = "QuartzCore", kind = "framework")]
unsafe extern "C" {}

#[cfg(any(target_os = "ios", target_os = "macos"))]
#[link(name = "c++")]
unsafe extern "C" {}

#[cfg(any(target_os = "ios", target_os = "macos"))]
#[link(name = "AudioToolbox", kind = "framework")]
unsafe extern "C" {}

#[cfg(any(target_os = "ios", target_os = "macos"))]
#[link(name = "Foundation", kind = "framework")]
unsafe extern "C" {}

#[macro_export]
macro_rules! start {
    ($app:ty) => {
        use $crate::__private::*;

        #[cfg(target_arch = "wasm32")]
        #[cfg_attr(target_arch = "wasm32", wasm_bindgen(start))]
        pub fn maverick_main() {
            MaverickOS::<$app>::start(
                env!("CARGO_PKG_NAME"),
                include_dir::include_dir!("$CARGO_MANIFEST_DIR/resources"),
            )
        }

        #[cfg(target_os = "ios")]
        #[unsafe(no_mangle)]
        pub extern "C" fn maverick_main() {
            MaverickOS::<$app>::start(
                env!("CARGO_PKG_NAME"),
                include_dir::include_dir!("$CARGO_MANIFEST_DIR/resources"),
            )
        }

        #[cfg(target_os = "android")]
        #[unsafe(no_mangle)]
        pub fn android_main(app: AndroidApp) {
            MaverickOS::<$app>::start(
                app,
                env!("CARGO_PKG_NAME"),
                include_dir::include_dir!("$CARGO_MANIFEST_DIR/resources"),
            )
        }

        #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
        pub fn maverick_main() {
            MaverickOS::<$app>::start(
                env!("CARGO_PKG_NAME"),
                include_dir::include_dir!("$CARGO_MANIFEST_DIR/resources"),
            )
        }
    };
}