#[cfg(any(target_os = "ios", target_os = "macos"))]
mod apple;
#[cfg(any(target_os = "ios", target_os = "macos"))]
use apple::OsContextMenu;

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
use android::OsContextMenu;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextMenuAction {
    Copy,
    Paste,
    Cut,
    SelectAll,
    Delete,
    Custom(String),
}

#[derive(Clone)]
pub struct ContextMenu(
    #[cfg(any(target_os = "ios", target_os = "macos", target_os = "android"))]
    OsContextMenu
);

impl ContextMenu {
    pub(crate) fn new() -> Self {
        Self(
            #[cfg(any(target_os = "ios", target_os = "macos", target_os = "android"))]
            OsContextMenu::new()
        )
    }

    pub fn show(&self, x: f32, y: f32, actions: &[ContextMenuAction]) -> Option<ContextMenuAction> {
        #[cfg(any(target_os = "ios", target_os = "macos", target_os = "android"))]
        return self.0.show(x, y, actions);

        #[cfg(not(any(target_os = "ios", target_os = "macos", target_os = "android")))]
        {
            let _ = (x, y, actions);
            None
        }
    }
}