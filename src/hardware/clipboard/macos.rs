
#[derive(Clone)]
pub struct OsClipboard;

impl OsClipboard {
    pub fn new() -> Self {
        Self
    }

    pub fn get_content(&self) -> String {
        panic!("Clipboard not supported on macos")
    }

    pub fn set_content(&self, text: String) {
        panic!("Clipboard not supported on macos")
    }
}