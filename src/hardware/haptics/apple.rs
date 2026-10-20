
use std::cell::RefCell;

use objc2::{ClassType, MainThreadMarker};
use objc2::rc::Retained;
use objc2::AnyThread;

use objc2_core_haptics::{
    CHHapticEngine,
    CHHapticDeviceCapability,
};

use objc2_foundation::NSData;

const HAPTIC_PATTERN: &str = r#"
{
    "Version": 1.0,
    "Pattern": [
        {
            "Event": {
                "Time": 0.0,
                "EventType": "HapticTransient",
                "EventParameters": [
                    {
                        "ParameterID": "HapticIntensity",
                        "ParameterValue": 0.8
                    },
                    {
                        "ParameterID": "HapticSharpness",
                        "ParameterValue": 0.7
                    }
                ]
            }
        }
    ]
}
"#;

struct HapticState {
    engine: Retained<CHHapticEngine>,
    pattern: Retained<NSData>,
}

impl HapticState {
    fn new() -> Option<Self> {
        unsafe {
            let capabilities =
                CHHapticEngine::capabilitiesForHardware();

            if !capabilities.supportsHaptics() {
                println!("Haptics: hardware not supported");
                return None;
            }

            let engine =
                match CHHapticEngine::initAndReturnError(
                    CHHapticEngine::alloc()
                ) {
                    Ok(engine) => engine,
                    Err(error) => {
                        println!(
                            "Haptics: engine creation failed: {:?}",
                            error
                        );
                        return None;
                    }
                };

            engine.setPlaysHapticsOnly(true);
            engine.setAutoShutdownEnabled(true);

            let pattern = NSData::with_bytes(
                HAPTIC_PATTERN.as_bytes()
            );

            Some(Self { engine, pattern })
        }
    }

    fn vibrate(&self) -> Result<(), String> {
        unsafe {
            self.engine
                .startAndReturnError()
                .map_err(|e| format!("Engine start: {:?}", e))?;

            self.engine
                .playPatternFromData_error(&self.pattern)
                .map_err(|e| format!("Pattern playback: {:?}", e))?;

            Ok(())
        }
    }
}

thread_local! {
    static HAPTICS: RefCell<Option<HapticState>> =
        RefCell::new(None);
}

#[derive(Clone, Default)]
pub struct OsHaptics;

impl OsHaptics {
    pub fn new() -> Self {
        Self
    }

    pub fn vibrate(&self) {
        if MainThreadMarker::new().is_none() {
            println!("Haptics: not on main thread");
            return;
        }

        HAPTICS.with(|cache| {
            let mut cache = cache.borrow_mut();

            if cache.is_none() {
                *cache = HapticState::new();
            }

            if let Some(state) = cache.as_ref() {
                if let Err(error) = state.vibrate() {
                    println!("Haptics: {}", error);

                    // Recreate the engine on the next call.
                    *cache = None;
                }
            }
        });
    }
}
