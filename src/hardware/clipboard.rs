#[cfg(any(target_os = "ios", target_os = "macos"))]
mod ios;
#[cfg(any(target_os = "ios", target_os = "macos"))]
use ios::OsClipboard;

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
use android::OsClipboard;

#[derive(Clone)]
pub struct Clipboard(
    #[cfg(any(target_os = "ios", target_os = "macos", target_os = "android"))]
    OsClipboard
);

impl Clipboard {
    pub(crate) fn new(
        #[cfg(target_os = "android")]
        vm: &jni::JavaVM
    ) -> Self {
        Self(
            #[cfg(target_os = "ios")]
            OsClipboard::new(),
            #[cfg(target_os = "macos")]
            OsClipboard::new(),
            #[cfg(target_os = "android")]
            OsClipboard::new(vm)
        )
    }

    pub fn get(&self) -> String {
        #[cfg(any(target_os = "ios", target_os = "macos", target_os = "android"))]
        {
            self.0.get_content()
        }
        #[cfg(not(any(target_os = "ios", target_os = "macos", target_os = "android")))]
        panic!("Not Supported for Linux/Windows");
    }

    pub fn set(&self, text: String) {
        #[cfg(any(target_os = "ios", target_os = "macos", target_os = "android"))]
        self.0.set_content(text);
        #[cfg(not(any(target_os = "ios", target_os = "macos", target_os = "android")))]
        panic!("Not Supported for Linux/Windows");
    }
}