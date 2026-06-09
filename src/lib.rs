pub mod hardware;

pub mod window;
use window::{Window, Renderer, Surface, Input};

pub use air;

#[cfg(target_os = "android")]
use winit::platform::android::activity::AndroidApp;

use air::{Air, Secret, Services};

mod config;
pub use config::{IS_MOBILE, IS_WEB};

use rusqlite::OptionalExtension;

pub trait Application: 'static {
    type Renderer<'surface>: Renderer<'surface, Application=Self>;

    fn new(context: &mut Context) -> Self;
    fn on_input(&mut self, context: &mut Context, input: Input);

    fn background_services() -> Services {Services::default()}
    fn services() -> Services {Services::default()}
}

pub struct Context {
    pub hardware: hardware::Context,
    pub window: window::Context,
    pub air: air::Context
}

pub struct MaverickOS<A: Application> {
    context: Context,
    surface: Surface<A>,
    runtime: Air,
    app: A,
}

impl<A: Application> MaverickOS<A> {
    pub fn start(#[cfg(target_os = "android")] app: AndroidApp) {Window::<A>::start()}
    fn new(window: window::Context, surface: Surface<A>) -> Self {
        let hardware = hardware::Context::new();
        let conn = rusqlite::Connection::open("./SECRET.db").unwrap();
        conn.execute("CREATE TABLE if not exists Cache(
            key TEXT NOT NULL PRIMARY KEY,
            value BLOB NOT NULL
        );", []).unwrap();
        let secret = match conn.query_row(
            "SELECT value FROM Cache WHERE key='secret'",
            [], |r| Ok(serde_json::from_slice(&r.get::<_, Vec<u8>>(0)?).ok()),
        ).optional().unwrap().flatten() {
            Some(secret) => secret,
            None => {
                let secret = Secret::new();
                conn.execute(
                    "INSERT INTO Cache(key, value) VALUES ('secret', ?1) ON CONFLICT DO UPDATE SET value=excluded.value;",
                    [serde_json::to_vec(&secret).unwrap()],
                ).unwrap();
                secret
            }
        };
        let (air, runtime) = Air::start(secret);
        runtime.start_services(A::services());
        runtime.start_services(A::background_services());
        
        let mut context = Context{
            hardware,
            window,
            air
        };
        let app = A::new(&mut context);
        MaverickOS{
            context,
            surface,
            runtime,
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
