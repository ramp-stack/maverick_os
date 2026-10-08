pub mod hardware;

pub mod window;
use window::{Window, Renderer, Surface, Input};

mod runtime;
use runtime::{Runtime, RUNTIME};

mod cache;
use cache::Cache;

mod shared;

pub mod air;

mod config;
pub use config::{IS_MOBILE, IS_WEB};

#[cfg(target_os = "android")]
use winit::platform::android::activity::AndroidApp;

pub trait Application: 'static {
    type Renderer<'surface>: Renderer<'surface, Application=Self>;

    fn new(context: &Context) -> Self;
    fn on_input(&mut self, context: &Context, input: Input);
}

#[derive(Clone)]
pub struct Context {
    pub hardware: hardware::Context,
    pub runtime: runtime::Runtime,
    pub window: window::Context,
    pub air: air::Context,
}

pub struct MaverickOS<A: Application> {
    context: Context,
    surface: Surface<A>,
    app: A,
}

impl<A: Application> MaverickOS<A> {
    #[cfg(target_os = "android")] 
    pub fn start(app: AndroidApp) {Window::<A>::start(app)}

    #[cfg(not(target_os = "android"))] 
    pub fn start() {Window::<A>::start()}

    fn new(window: window::Context, surface: Surface<A>) -> Self {
        let hardware = hardware::Context::new();
        let runtime = Runtime::new();

        let mut s = Cache::new("secret").unwrap();
        let secret = s.get("secret").unwrap().unwrap_or_else(air::Secret::new);
        s.insert("secret", &secret).unwrap();
        let air = air::Context::new(secret);
        
        let context = Context{
            hardware,
            runtime,
            window,
            air
        };
        let app = A::new(&context);
        MaverickOS{
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
