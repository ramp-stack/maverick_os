#[cfg(target_os = "ios")]
use objc2::MainThreadMarker;
#[cfg(target_os = "ios")]
use objc2_ui_kit::UIApplication;
#[cfg(target_os = "ios")]
use objc2::rc::Retained;

/// Provides the safe area insets of the device screen.
pub struct SafeAreaInsets;

impl SafeAreaInsets {
    #[cfg(target_os = "ios")]
    pub fn get() -> (f32, f32, f32, f32) {
        unsafe {
            let mtm = MainThreadMarker::new().expect("must be on the main thread");
            let window: Retained<UIApplication> = UIApplication::sharedApplication(mtm);

            #[allow(deprecated)]
            if let Some(key_window) = window.keyWindow() {
                let insets = key_window.safeAreaInsets();

                return (insets.top as f32, insets.bottom as f32, insets.left as f32, insets.right as f32);
            }
        } 

        (0.0, 0.0, 0.0, 0.0)
    }

    #[cfg(not(target_os = "ios"))]
    pub fn get() -> (f32, f32, f32, f32) {
        (0.0, 0.0, 0.0, 0.0)
    }
}