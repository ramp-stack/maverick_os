#[cfg(target_os = "ios")]
use objc2::MainThreadMarker;
#[cfg(target_os = "ios")]
use objc2_ui_kit::UIApplication;
#[cfg(target_os = "ios")]
use std::sync::Once;
#[cfg(target_os = "ios")]
use objc2::rc::Retained;

#[cfg(target_os = "ios")]
static mut INSETS: [f32; 4] = [0.0; 4];
#[cfg(target_os = "ios")]
static INIT: Once = Once::new();

#[cfg(target_os = "ios")]
#[allow(dead_code)]
pub fn get_safe_area_insets() -> (f32, f32, f32, f32) {
    unsafe {
        INIT.call_once(|| {
            let mtm = MainThreadMarker::new().expect("must be on the main thread");
            let window: Retained<UIApplication> = UIApplication::sharedApplication(mtm);

            if let Some(key_window) = window.keyWindow() {
                let insets = key_window.safeAreaInsets();

                INSETS[0] = insets.top as f32;
                INSETS[1] = insets.bottom as f32;
                INSETS[2] = insets.left as f32;
                INSETS[3] = insets.right as f32;
            }
        });

        (INSETS[0], INSETS[1], INSETS[2], INSETS[3])
    } 
}

#[cfg(not(target_os = "ios"))]
#[allow(dead_code)]
pub fn get_safe_area_insets() -> (f32, f32, f32, f32) {
    (0.0, 0.0, 0.0, 0.0)
}