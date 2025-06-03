#[cfg(not(any(target_os = "ios", target_os = "android")))]
use cli_clipboard;
#[cfg(target_os = "ios")]
use objc2_ui_kit::UIPasteboard;
#[cfg(target_os = "ios")]
use objc2_foundation::NSString;

pub struct Clipboard;
impl Clipboard {
    #[cfg(target_os = "ios")]
    pub fn get(&self) -> String {
        unsafe {
            let pasteboard = UIPasteboard::generalPasteboard();
            pasteboard.string().map(|s| s.to_string()).unwrap_or_default() 
        }
    }

    #[cfg(target_os = "ios")]
    pub fn set(&self, text: String) {
        unsafe {
            let pasteboard = UIPasteboard::generalPasteboard();
            let ns_string = NSString::from_str(&text);
            pasteboard.setString(Some(&ns_string));
        }
    }

    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    pub fn get(&self) -> String {
        cli_clipboard::get_contents().unwrap_or_default()
    }

    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    pub fn set(&self, text: String) {
        cli_clipboard::set_contents(text).unwrap();
    }
}
