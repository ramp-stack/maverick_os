use arboard::Clipboard;

#[derive(Clone)]
pub struct OsClipboard;

impl OsClipboard {
    pub fn new() -> Self {
        Self
    }

    pub fn get_content(&self) -> Option<String> {
        let mut clipboard = Clipboard::new().ok()?;
        Some(clipboard.get_text().ok()?)
    }

    pub fn set_content(&self, text: String) {
        Clipboard::new().as_mut().map(|clipboard| clipboard.set_text(text.to_string()));
    }
}
