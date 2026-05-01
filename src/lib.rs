pub mod hardware;

pub mod runtime;
use runtime::{Runtime, Services};

pub mod window;
use window::{Window, Renderer, Surface, Input};

#[cfg(target_os = "android")]
use winit::platform::android::activity::AndroidApp;

pub mod air;
use air::{Contracts, Air};

mod config;
pub use config::{IS_MOBILE, IS_WEB};

pub trait Application: 'static {
    type Renderer<'surface>: Renderer<'surface, Application=Self>;

    fn new(context: &mut Context) -> Self;
    fn on_input(&mut self, context: &mut Context, input: Input);
    //fn draw<'surface>(&self, context: &Context, renderer: &mut Self::Renderer<'surface>);

    fn contracts() -> Contracts {Contracts::default()}
    fn background_services() -> Services {Vec::new()}
    fn services() -> Services {Vec::new()}
}

pub struct Context {
    pub hardware: hardware::Context,
    pub window: window::Context,
    pub air: air::Context
}

pub struct MaverickOS<A: Application> {
    runtime: Runtime,
    context: Context,
    surface: Surface<A>,
    app: A,
}

impl<A: Application> MaverickOS<A> {
    pub fn start(#[cfg(target_os = "android")] app: AndroidApp) {Window::<A>::start()}
    fn new(window: window::Context, surface: Surface<A>) -> Self {
        let hardware = hardware::Context::new();
        let (air, air_ctx) = Air::start(&hardware, A::contracts()).unwrap();
        let runtime = Runtime::start(&air_ctx, air, A::services(), A::background_services());
        
        let mut context = Context{
            hardware,
            window,
            air: air_ctx,
        };
        let app = A::new(&mut context);
        MaverickOS{
            runtime,
            context,
            surface,
            app
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

pub mod __private {
    #[cfg(target_os = "android")]
    pub use winit::platform::android::activity::AndroidApp;
    pub use crate::MaverickOS;
}

#[macro_export]
macro_rules! start {
    ($app:ty) => {
        use $crate::__private::*;

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
        #[unsafe(no_mangle)]
        pub fn android_main(app: AndroidApp) {
            MaverickOS::<$app>::start(app)
        }

        #[cfg(not(any(target_os = "android", target_os="ios", target_arch = "wasm32")))]
        pub fn maverick_main() {
            MaverickOS::<$app>::start()
        }
    };
}
