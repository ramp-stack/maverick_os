// Cross platform logger to provide a simple and diverse way to start logging,
//      with a specifed level across different platforms Android and WASM and other std desktop targets.

//System:

//<Android>>>: uses android_logger to log messages to logcat.

//<WASM>>> uses console_log and sets a panic hook for detailed messages on browser console.

//<Linux, iOs and macOS>>> uses env_logger for std output logging.

// Level sys: we use log::Level to set the maxium logging level. Defaults to Warn if None is used.


pub struct Logger;


impl Logger {
    pub fn start (level: Option<log::Level>) {
        let level = level.unwrap_or(log::Level::Warn);
        #[cfg(target_os="android")]
        {
            android_logger::init_once(
                android_logger::Config::default().with_max_level(level.to_level_filter()),
            );
        }

        #[cfg(target_arch="wasm32")]
        {
            std::panic::set_hook(Box::new(console_error_panic_hook::hook));
            console_log::init_with_level(level).expect("Couldn't initialize logger");
        }

        #[cfg(not(any(target_os="android", target_arch="wasm32")))]
        {
            env_logger::builder().filter_level(level.to_level_filter()).init();
        }
    }
}
