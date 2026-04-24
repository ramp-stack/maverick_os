pub mod hardware;

pub mod runtime;
use runtime::Services;

pub mod window;
pub use window::Event;

pub mod air;
use air::Contracts;

mod config;
pub use config::{IS_MOBILE, IS_WEB};

pub use include_dir::Dir;

pub trait Application {
    fn new(context: &mut Context, assets: Dir<'static>) -> Self;
    fn on_event(&mut self, context: &mut Context, event: Event);
    fn contracts() -> Contracts {Contracts::default()}

    fn background_services() -> Services {Vec::new()}
    fn services() -> Services {Vec::new()}
}

pub struct Context {
    pub hardware: hardware::Context,
    pub window: window::Context,
    pub air: air::Context
}

pub mod __private {
    #[cfg(target_os = "android")]
    pub use winit::platform::android::activity::AndroidApp;
    pub use include_dir;

    use crate::{Context, Application, window, hardware, runtime, air};

    use runtime::Runtime;
    use window::{WindowManager, EventHandler, Event, Lifetime};
    use air::Air;

    pub struct MaverickOS<A: Application> {
        runtime: Runtime,
        context: Context,
        app: A,
    }

    use include_dir::Dir;

    struct S<A: Application>(Option<Dir<'static>>, Option<MaverickOS<A>>);

    impl<A: Application + 'static> MaverickOS<A> {
        pub fn start(
            #[cfg(target_os = "android")]
            app: AndroidApp,
            dir: Dir<'static>
        ) {
            WindowManager::start(
                #[cfg(target_os = "android")]
                app,
                S::<A>(Some(dir), None)
            )
        }
    }

    impl<A: Application + 'static> EventHandler for S<A> {
        fn event(&mut self, window: &window::Context, event: Event) {
            match &mut self.1 {
                Some(maverick) => {
                    maverick.context.window = window.clone();
                    match &event {
                        Event::Lifetime(Lifetime::Paused) => maverick.runtime.pause(),
                        Event::Lifetime(Lifetime::Resumed) => maverick.runtime.resume(),
                        Event::Lifetime(Lifetime::Close) => maverick.runtime.shutdown(),
                        _ => {}
                    }
                    maverick.app.on_event(&mut maverick.context, event);
                },
                none => {
                    let hardware = hardware::Context::new();
                    let (air, service) = Air::start(&hardware, A::contracts()).unwrap();
                    let runtime = Runtime::start(&air, service, A::services(), A::background_services());
                    let mut context = Context{
                        hardware,
                        window: window.clone(),
                        air,
                    };
                    let app = A::new(&mut context, self.0.take().unwrap());
                    *none = Some(MaverickOS{
                        runtime,
                        context,
                        app
                    });
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
            MaverickOS::<$app>::start(include_dir::include_dir!("$CARGO_MANIFEST_DIR/resources"))
        }

        #[cfg(target_os = "ios")]
        #[unsafe(no_mangle)]
        pub extern "C" fn maverick_main() {
            MaverickOS::<$app>::start(include_dir::include_dir!("$CARGO_MANIFEST_DIR/resources"))
        }

        #[cfg(target_os = "android")]
        #[unsafe(no_mangle)]
        pub fn android_main(app: AndroidApp) {
            MaverickOS::<$app>::start(app, include_dir::include_dir!("$CARGO_MANIFEST_DIR/resources"))
        }

        #[cfg(not(any(target_os = "android", target_os="ios", target_arch = "wasm32")))]
        pub fn maverick_main() {
            MaverickOS::<$app>::start(include_dir::include_dir!("$CARGO_MANIFEST_DIR/resources"))
        }
    };
}
