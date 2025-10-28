#[cfg(any(target_os = "ios", target_os = "macos"))]
mod apple;
#[cfg(any(target_os = "ios", target_os = "macos"))]
use apple::OsNotifications;

#[derive(Clone)]
pub struct Notifications(
    #[cfg(any(target_os = "ios", target_os = "macos"))]
    OsNotifications
);

impl Notifications {
    pub(crate) fn new() -> Self {
        Self(
            #[cfg(any(target_os = "ios", target_os = "macos"))]
            OsNotifications::new()
        )
    }

    pub fn register(&self) {
        #[cfg(any(target_os = "ios", target_os = "macos"))]
        self.0.register();
        
        #[cfg(not(any(target_os = "ios", target_os = "macos")))]
        panic!("not supported os");
    }

    pub fn push(&self, title: &str, body: &str) {
        #[cfg(any(target_os = "ios", target_os = "macos"))]
        self.0.push(title, body);
        
        #[cfg(not(any(target_os = "ios", target_os = "macos")))]
        panic!("not supported os");
    }
}