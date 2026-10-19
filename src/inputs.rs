use std::hash::{DefaultHasher, Hasher, Hash};
use std::path::PathBuf;

use winit::event::{WindowEvent, DeviceId, TouchPhase, MouseButton as WinitMouseButton, MouseScrollDelta, ElementState};
use winit::keyboard::{Key as WinitKey, NamedKey as WinitNamedKey};
use crate::LogicalWindow;

fn hash(di: DeviceId) -> u64 {
    let mut hasher = DefaultHasher::default();
    di.hash(&mut hasher);
    hasher.finish()
}

pub(crate) enum OSInput {
    Input(Input),
    Resize,
    Close,
    Draw,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    Resized(LogicalWindow),
    WindowFocused(bool),
    WindowMoved(i32, i32),
    DroppedFile(PathBuf),
    HoveredFile(PathBuf),
    HoveredFileCancelled,
    Device(u64, DeviceInput),
}

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

#[derive(Default)]
pub(crate) struct Inputs{
    modifiers: Modifiers,
    touching: bool,
    scroll: Option<(f32, f32)>,
    mouse: (f64, f64),
}
impl Inputs {
    fn logical(x: f64, y: f64, scale: f64) -> (f32, f32) {
        ((x / scale) as f32, (y / scale) as f32)
    }

    pub fn process(&mut self, event: WindowEvent, scale: f64) -> Option<OSInput> {
        let mouse = Self::logical(self.mouse.0, self.mouse.1, scale);
        match event {
            WindowEvent::CloseRequested | WindowEvent::Destroyed => Some(OSInput::Close),
            WindowEvent::RedrawRequested => Some(OSInput::Draw),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged{..} => Some(OSInput::Resize),
            WindowEvent::KeyboardInput{device_id, event, ..} => match event.logical_key {
                WinitKey::Named(named) => match named {
                    WinitNamedKey::Enter => Some(Key::Enter),
                    WinitNamedKey::Tab => Some(Key::Tab),
                    WinitNamedKey::Space => Some(Key::Character(' ')),
                    WinitNamedKey::Escape => Some(Key::Escape),
                    WinitNamedKey::ArrowUp => Some(Key::Up),
                    WinitNamedKey::ArrowDown => Some(Key::Down),
                    WinitNamedKey::ArrowLeft => Some(Key::Left),
                    WinitNamedKey::ArrowRight => Some(Key::Right),
                    WinitNamedKey::Delete => Some(Key::Delete),
                    WinitNamedKey::Backspace => Some(Key::Backspace),
                    WinitNamedKey::Home => Some(Key::Home),
                    WinitNamedKey::End => Some(Key::End),
                    WinitNamedKey::Shift => Some(Key::Shift),
                    WinitNamedKey::Control => Some(Key::Control),
                    WinitNamedKey::Alt => Some(Key::Alt),
                    WinitNamedKey::Super => Some(Key::SuperMeta),
                    WinitNamedKey::Meta => Some(Key::SuperMeta),
                    WinitNamedKey::CapsLock => Some(Key::CapsLock),
                    WinitNamedKey::NumLock => Some(Key::NumLock),
                    WinitNamedKey::ScrollLock => Some(Key::ScrollLock),
                    _ => None,
                },
                WinitKey::Character(c) => Some(Key::Character(c.chars().next().unwrap())),
                _ => None,
            }.map(|key| OSInput::Input(Input::Device(hash(device_id), DeviceInput::Keyboard(key, match event.state {
                ElementState::Pressed if event.repeat => KeyboardState::Repeated,
                ElementState::Pressed => KeyboardState::Pressed,
                ElementState::Released => KeyboardState::Released,
            }, self.modifiers)))),
            WindowEvent::CursorMoved{device_id, position} if self.mouse != (position.x, position.y) => {
                self.mouse = (position.x, position.y);
                let mouse = Self::logical(position.x, position.y, scale);
                Some(OSInput::Input(Input::Device(hash(device_id), DeviceInput::Mouse(mouse, MouseState::Moved))))
            },
            WindowEvent::MouseWheel{device_id, delta, phase} => match delta {
                MouseScrollDelta::LineDelta(x, y) => {
                    let shift = self.modifiers.shift;
                    let (dx, dy) = if shift {(-y, -x)} else {(-x, -y)};
                    (dx.abs() > 0.001 || dy.abs() > 0.001).then_some(
                        OSInput::Input(Input::Device(hash(device_id), DeviceInput::Mouse(mouse, MouseState::Scroll(dx, dy))))
                    )
                }
                MouseScrollDelta::PixelDelta(p) => {
                    match phase {
                        TouchPhase::Started => {
                            self.scroll = Some((0.0, 0.0));
                            Some(OSInput::Input(Input::Device(hash(device_id), DeviceInput::Mouse(mouse, MouseState::Scroll(0.0, 0.0)))))
                        }
                        TouchPhase::Moved => {
                            let (dx, dy) = Self::logical(-p.x, -p.y, scale);
                            (dx.abs() > 0.001 || dy.abs() > 0.001).then_some(
                                OSInput::Input(Input::Device(hash(device_id), DeviceInput::Mouse(mouse, MouseState::Scroll(dx, dy))))
                            )
                        }
                        TouchPhase::Ended | TouchPhase::Cancelled => {
                            self.scroll = None;
                            None
                        }
                    }
                }
            },
            WindowEvent::MouseInput{device_id, state, button} => {
                let button = match button {
                    WinitMouseButton::Right => MouseButton::Right,
                    WinitMouseButton::Middle => MouseButton::Middle,
                    _ => MouseButton::Left,
                };
                let state = match state {
                    ElementState::Pressed => MouseState::Pressed(button),
                    ElementState::Released => MouseState::Released(button),
                };
                Some(OSInput::Input(Input::Device(hash(device_id), DeviceInput::Mouse(mouse, state))))
            },
            WindowEvent::Touch(touch) => {
                self.mouse = (touch.location.x, touch.location.y);
                let position = Self::logical(touch.location.x, touch.location.y, scale);
                match touch.phase {
                    TouchPhase::Started => {
                        self.scroll  = Some(position);
                        self.touching = true;
                        Some(OSInput::Input(Input::Device(u64::MAX, DeviceInput::Mouse(position, MouseState::Pressed(MouseButton::Left)))))
                    }
                    TouchPhase::Ended | TouchPhase::Cancelled => {
                        self.touching = false;
                        Some(OSInput::Input(Input::Device(u64::MAX, DeviceInput::Mouse(position, MouseState::Released(MouseButton::Left)))))
                    }
                    TouchPhase::Moved => {
                        self.scroll.and_then(|(prev_x, prev_y)| {
                            self.scroll = Some(position);
                            let dx = position.0 - prev_x;
                            let dy = position.1 - prev_y;
                            let scroll_x = -dx;
                            let scroll_y = -dy;
                            (scroll_x.abs() > 0.01 || scroll_y.abs() > 0.01).then_some(
                                OSInput::Input(Input::Device(u64::MAX, DeviceInput::Mouse(position, MouseState::Scroll(scroll_x, scroll_y))))
                            )
                        })
                    }
                }
            },
            WindowEvent::DroppedFile(path) => Some(OSInput::Input(Input::DroppedFile(path))),
            WindowEvent::HoveredFile(path) => Some(OSInput::Input(Input::HoveredFile(path))),
            WindowEvent::HoveredFileCancelled => Some(OSInput::Input(Input::HoveredFileCancelled)),
            WindowEvent::ModifiersChanged(mods) => {
                self.modifiers = Modifiers{
                    shift: mods.state().shift_key(),
                    control: mods.state().control_key(),
                    alt: mods.state().alt_key(),
                    supermeta: mods.state().super_key(),
                };
                None
            },
            WindowEvent::CursorEntered{device_id} => Some(OSInput::Input(Input::Device(hash(device_id), DeviceInput::CursorEntered))),
            WindowEvent::CursorLeft{device_id} => Some(OSInput::Input(Input::Device(hash(device_id), DeviceInput::CursorLeft))),
            WindowEvent::Moved(position) => {
                let (x, y) = Self::logical(position.x as f64, position.y as f64, scale);
                Some(OSInput::Input(Input::WindowMoved(x as i32, y as i32)))
            },
            e => {log::debug!("Ignored Event: {:?}", e); None}
        }
    }
}
