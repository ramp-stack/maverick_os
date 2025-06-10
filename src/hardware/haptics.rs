
#![allow(dead_code)]
#[cfg(target_os = "ios")]
use objc2_ui_kit::UIImpactFeedbackGenerator;
#[cfg(target_os = "ios")]
use objc2::MainThreadMarker;

pub struct Haptics;

impl Haptics {
    #[cfg(target_os = "ios")]
    pub fn vibrate() {
        unsafe {
            let mtm = MainThreadMarker::new().expect("must be on the main thread");
            let generator = UIImpactFeedbackGenerator::new(mtm);
            let intensity = 0.75;
            generator.prepare();
            generator.impactOccurredWithIntensity(intensity);
        }
    }

    #[cfg(target_os = "android")]
    pub fn vibrate() {}

    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    pub fn vibrate() {}
}


