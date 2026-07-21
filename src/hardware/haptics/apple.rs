use objc2::MainThreadOnly;
use objc2::rc::{Allocated, Retained};
use objc2::{MainThreadMarker, msg_send};
use objc2_ui_kit::{UIImpactFeedbackGenerator, UIImpactFeedbackStyle};

#[derive(Clone)]
pub struct OsHaptics;

impl OsHaptics {
    pub fn new() -> Self {
        Self
    }

    pub fn vibrate(&self) {
        unsafe {
            if let Some(mtm) = MainThreadMarker::new() {
                let alloc: Allocated<UIImpactFeedbackGenerator> =
                    UIImpactFeedbackGenerator::alloc(mtm);
                let generator: Retained<UIImpactFeedbackGenerator> =
                    msg_send![alloc, initWithStyle: UIImpactFeedbackStyle::Rigid];
                generator.prepare();
                generator.impactOccurred();
            }
        }
    }
}
