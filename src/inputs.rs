use std::path::PathBuf;
use crate::LogicalWindow;

#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    Notification(String, String),
    Media(Media),
    Resized(LogicalWindow),
    WindowFocused(bool),
    WindowMoved(i32, i32),
    DroppedFile(PathBuf),
    HoveredFile(PathBuf),
    HoveredFileCancelled,
    Device(u64, DeviceInput),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Media {Play, Pause, Toggle, VolumeUp, VolumeDown}

#[derive(Clone, Debug, PartialEq)]
pub enum DeviceInput {
    Keyboard(Key, KeyboardState, Modifiers),
    CursorEntered,
    CursorLeft,
    Mouse((f32, f32), MouseState),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub supermeta: bool,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Key {
    Escape, Enter, Tab, Space,
    Up, Down, Left, Right,
    Delete, Backspace, Home, End,
    Shift, Control, Alt, SuperMeta,
    CapsLock, NumLock, ScrollLock,
    Character(char)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyboardState{ Pressed, Repeated, Released }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton{ Left, Right, Middle }

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MouseState {
    Pressed(MouseButton),
    Released(MouseButton),
    Scroll(f32, f32),
    Moved
}
