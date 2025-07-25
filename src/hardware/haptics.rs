
#![allow(dead_code)]
#[cfg(target_os = "ios")]
use objc2_ui_kit::{UIImpactFeedbackGenerator, UIImpactFeedbackStyle};
#[cfg(target_os = "ios")]
use objc2::{MainThreadMarker, MainThreadOnly, msg_send};
#[cfg(target_os = "ios")]
use objc2::rc::{Retained, Allocated};

pub struct Haptics;

impl Haptics {
    #[cfg(target_os = "ios")]
    pub fn vibrate() {
        unsafe {
            if let Some(mtm) = MainThreadMarker::new() {
                let alloc: Allocated<UIImpactFeedbackGenerator> = UIImpactFeedbackGenerator::alloc(mtm);
                let generator: Retained<UIImpactFeedbackGenerator> = msg_send![alloc, initWithStyle: UIImpactFeedbackStyle::Rigid];
                generator.prepare();
                generator.impactOccurred();
            }
        }
    }

    #[cfg(target_os = "android")]
    pub fn vibrate() {}

    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    pub fn vibrate() {}
}


