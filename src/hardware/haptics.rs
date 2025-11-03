#[cfg(target_os = "ios")]
mod apple;
#[cfg(target_os = "ios")]
use apple::OsHaptics;

#[derive(Clone)]
pub struct Haptics(
    #[cfg(target_os = "ios")]
    OsHaptics
);

impl Haptics {
    pub(crate) fn new() -> Self {
        Self(
            #[cfg(target_os = "ios")]
            OsHaptics::new()
        )
    }

    pub fn vibrate(&self) {
        #[cfg(target_os = "ios")]
        self.0.vibrate();
        
        #[cfg(not(target_os = "ios"))]
        panic!("Haptics not supported on this platform");
    }
}