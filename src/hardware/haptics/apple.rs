
use std::cell::RefCell;

use objc2::MainThreadMarker;
use objc2::rc::Retained;

use objc2_ui_kit::{
    UIApplication,
    UIImpactFeedbackGenerator,
    UIImpactFeedbackStyle,
    UIWindow,
};

// Cached generator, associated with the current window.
thread_local! {
    static GENERATOR: RefCell<Option<(
        Retained<UIWindow>,
        Retained<UIImpactFeedbackGenerator>
    )>> = RefCell::new(None);
}

#[derive(Clone, Default)]
pub struct OsHaptics;

impl OsHaptics {
    pub fn new() -> Self {
        Self
    }

    pub fn vibrate(&self) {
        let Some(mtm) = MainThreadMarker::new() else {
            eprintln!("Haptics: must be called on main thread");
            return;
        };

        let app = UIApplication::sharedApplication(mtm);

        #[allow(deprecated)]
        let Some(window) = app.keyWindow() else {
            eprintln!("Haptics: no active window");
            return;
        };

        GENERATOR.with(|cache| {
            let mut cache = cache.borrow_mut();

            let needs_new = match cache.as_ref() {
                Some((old_window, _)) => {
                    !std::ptr::eq(&**old_window, &*window)
                }
                None => true,
            };

            if needs_new {
                let generator =
                    UIImpactFeedbackGenerator::feedbackGeneratorWithStyle_forView(
                        UIImpactFeedbackStyle::Rigid,
                        &window,
                    );

                generator.prepare();
                *cache = Some((window, generator));
            }

            if let Some((_, generator)) = cache.as_ref() {
                generator.impactOccurred();
                generator.prepare();
            }
        });
    }
}
