use std::sync::mpsc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextMenuAction {
    Copy,
    Paste,
    Cut,
    SelectAll,
    Delete,
    Custom(String),
}

impl ContextMenuAction {
    fn label(&self) -> &str {
        match self {
            ContextMenuAction::Copy => "Copy",
            ContextMenuAction::Paste => "Paste",
            ContextMenuAction::Cut => "Cut",
            ContextMenuAction::SelectAll => "Select All",
            ContextMenuAction::Delete => "Delete",
            ContextMenuAction::Custom(label) => label.as_str(),
        }
    }

    fn from_index(index: usize, actions: &[ContextMenuAction]) -> Option<ContextMenuAction> {
        actions.get(index).cloned()
    }
}

pub struct ContextMenu;

#[cfg(target_os = "windows")]
mod win32 {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "user32")]
    extern "system" {
        pub fn CreatePopupMenu() -> *mut std::ffi::c_void;
        pub fn AppendMenuW(
            hmenu: *mut std::ffi::c_void,
            uflags: u32,
            uid_new_item: usize,
            lpnew_item: *const u16,
        ) -> i32;
        pub fn TrackPopupMenu(
            hmenu: *mut std::ffi::c_void,
            uflags: u32,
            x: i32,
            y: i32,
            nreserved: i32,
            hwnd: *mut std::ffi::c_void,
            prcrect: *const std::ffi::c_void,
        ) -> i32;
        pub fn DestroyMenu(hmenu: *mut std::ffi::c_void) -> i32;
        pub fn GetForegroundWindow() -> *mut std::ffi::c_void;
    }

    pub const MF_STRING: u32 = 0x0000;
    pub const TPM_RETURNCMD: u32 = 0x0100;
    pub const TPM_LEFTALIGN: u32 = 0x0000;
    pub const TPM_TOPALIGN: u32 = 0x0000;

    pub fn to_wide(s: &str) -> Vec<u16> {
        OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }
}

impl ContextMenu {
    /// Show a native Win32 popup context menu at screen coordinates (x, y).
    /// Blocks until the user selects an item or dismisses the menu.
    pub fn show(x: f32, y: f32, actions: &[ContextMenuAction]) -> Option<ContextMenuAction> {
        #[cfg(target_os = "windows")]
        unsafe {
            let hmenu = win32::CreatePopupMenu();
            if hmenu.is_null() {
                return None;
            }

            for (i, action) in actions.iter().enumerate() {
                let wide_label = win32::to_wide(action.label());
                win32::AppendMenuW(
                    hmenu,
                    win32::MF_STRING,
                    (i + 1) as usize, // menu IDs are 1-based; 0 means dismissed
                    wide_label.as_ptr(),
                );
            }

            let hwnd = win32::GetForegroundWindow();

            let flags = win32::TPM_RETURNCMD | win32::TPM_LEFTALIGN | win32::TPM_TOPALIGN;

            let result = win32::TrackPopupMenu(
                hmenu,
                flags,
                x as i32,
                y as i32,
                0,
                hwnd,
                std::ptr::null(),
            );

            win32::DestroyMenu(hmenu);

            if result > 0 {
                ContextMenuAction::from_index((result - 1) as usize, actions)
            } else {
                None
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ = (x, y, actions);
            None
        }
    }
}