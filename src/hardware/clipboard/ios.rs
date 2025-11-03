use objc2_foundation::NSString;
use objc2_ui_kit::UIPasteboard;

#[derive(Clone)]
pub struct OsClipboard;

impl OsClipboard {
    pub fn new() -> Self {
        Self
    }

    pub fn get_content(&self) -> String {
        unsafe {
            let pasteboard = UIPasteboard::generalPasteboard();
            pasteboard.string().map(|s| s.to_string()).unwrap_or_default()
        }
    }

    pub fn set_content(&self, text: String) {
        unsafe {
            let pasteboard = UIPasteboard::generalPasteboard();
            let ns_string = NSString::from_str(&text);
            pasteboard.setString(Some(&ns_string));
        }
    }
}