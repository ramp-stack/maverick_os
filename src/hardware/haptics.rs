
#![allow(dead_code)]
#[cfg(target_os = "ios")]
use objc2_ui_kit::{UIImpactFeedbackGenerator, UIImpactFeedbackStyle};
#[cfg(target_os = "ios")]
use objc2::{MainThreadMarker, MainThreadOnly, msg_send};
#[cfg(target_os = "ios")]
use objc2::rc::{Retained, Allocated};

//Cross platform<phone devices duhh> haptic feedback sys.
// System:

// <iOS>>>: Uses UIImpactFeedbackGenerator with a Rigid style to produce haptic feedback.

// <Android>>>: No operation method.

/// Trigger haptic feedback on the device.
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


