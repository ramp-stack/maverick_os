use winit::event::{
    WindowEvent, DeviceId, TouchPhase, MouseButton as WinitMouseButton, MouseScrollDelta, ElementState,
};
use winit::event_loop::EventLoopProxy;
use winit::keyboard::{Key as WinitKey, NamedKey as WinitNamedKey};

use std::hash::{DefaultHasher, Hasher, Hash};

use crate::hardware;
use crate::inputs::{Input, Media, DeviceInput, Modifiers, Key, KeyboardState, MouseButton, MouseState};

pub enum Output {Input(Input), GeometryChanged, Wake}

pub struct Events {
    modifiers: Modifiers,
    touching: bool,
    scroll: Option<(f32, f32)>,
    mouse: (f64, f64),
    hardware: hardware::Events,
    proxy: EventLoopProxy<Output>,
}
impl Events {
    pub fn new(window: &crate::Window, proxy: EventLoopProxy<Output>) -> Self {
        Self {
            modifiers: Modifiers::default(), touching: false, scroll: None,
            mouse: (0.0, 0.0), hardware: hardware::Events::new(window, proxy.clone()), proxy,
        }
    }

    pub fn key(proxy: &EventLoopProxy<Output>, key: Key) {
        for state in [KeyboardState::Pressed, KeyboardState::Released] {
            let _ = proxy.send_event(Output::Input(Input::Device(u64::MAX - 1, DeviceInput::Keyboard(
                key, state, Modifiers::default(),
            ))));
        }
    }

    pub fn text(proxy: &EventLoopProxy<Output>, text: &str) {
        let mut characters = text.chars().peekable();
        while let Some(character) = characters.next() {
            Self::key(proxy, match character {
                '\r' => {
                    if characters.peek() == Some(&'\n') {characters.next();}
                    Key::Enter
                }
                '\n' => Key::Enter,
                '\t' => Key::Tab,
                _ => Key::Character(character),
            });
        }
    }

    fn hash(device: DeviceId) -> u64 {
        let mut hasher = DefaultHasher::default();
        device.hash(&mut hasher);
        hasher.finish()
    }

    fn logical(x: f64, y: f64, scale: f64) -> (f32, f32) {
        ((x / scale) as f32, (y / scale) as f32)
    }

    pub async fn next(&mut self) -> Input {self.hardware.next().await}

    pub fn suspend(&mut self) {
        self.scroll = None;
        self.touching = false;
        self.hardware.suspend();
    }

    pub fn process(&mut self, event: WindowEvent, scale: f64) -> Option<Input> {
        let event = self.hardware.process(event, scale)?;
        let mouse = Self::logical(self.mouse.0, self.mouse.1, scale);
        match event {
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged{..} => {
                self.scroll = None;
                self.touching = false;
                None
            },
            WindowEvent::Ime(winit::event::Ime::Commit(text)) => {Self::text(&self.proxy, &text); None}
            WindowEvent::KeyboardInput{device_id, event, ..} => match event.logical_key {
                WinitKey::Named(named) => match named {
                    WinitNamedKey::MediaPlay | WinitNamedKey::MediaPause
                    | WinitNamedKey::MediaPlayPause => {
                        return (event.state == ElementState::Pressed && !event.repeat).then(|| {
                            let input = match named {
                                WinitNamedKey::MediaPlay => Media::Play,
                                WinitNamedKey::MediaPause => Media::Pause,
                                _ => if hardware::Clock::playing().unwrap_or(false) {
                                    Media::Pause
                                } else {Media::Play},
                            };
                            Input::Media(input)
                        });
                    }
                    WinitNamedKey::AudioVolumeUp | WinitNamedKey::AudioVolumeDown => {
                        return (event.state == ElementState::Pressed).then(|| {
                            Input::Media(if named == WinitNamedKey::AudioVolumeUp {
                                Media::VolumeUp
                            } else {Media::VolumeDown})
                        });
                    }
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
                WinitKey::Character(c) => c.chars().next().map(Key::Character),
                _ => None,
            }.map(|key| {
                Input::Device(Self::hash(device_id), DeviceInput::Keyboard(
                    key, match event.state {
                        ElementState::Pressed if event.repeat => KeyboardState::Repeated,
                        ElementState::Pressed => KeyboardState::Pressed,
                        ElementState::Released => KeyboardState::Released,
                    }, self.modifiers,
                ))
            }),
            WindowEvent::CursorMoved{device_id, position} if self.mouse != (position.x, position.y) => {
                self.mouse = (position.x, position.y);
                let mouse = Self::logical(position.x, position.y, scale);
                Some(Input::Device(
                    Self::hash(device_id), DeviceInput::Mouse(mouse, MouseState::Moved),
                ))
            },
            WindowEvent::MouseWheel{device_id, delta, phase} => match delta {
                MouseScrollDelta::LineDelta(x, y) => {
                    let shift = self.modifiers.shift;
                    let (dx, dy) = if shift {(-y, -x)} else {(-x, -y)};
                    (dx.abs() > 0.001 || dy.abs() > 0.001).then_some(
                        Input::Device(
                            Self::hash(device_id), DeviceInput::Mouse(mouse, MouseState::Scroll(dx, dy)),
                        )
                    )
                }
                MouseScrollDelta::PixelDelta(p) => {
                    match phase {
                        TouchPhase::Started => {
                            self.scroll = Some((0.0, 0.0));
                            Some(Input::Device(Self::hash(device_id), DeviceInput::Mouse(
                                mouse, MouseState::Scroll(0.0, 0.0),
                            )))
                        }
                        TouchPhase::Moved => {
                            let (dx, dy) = Self::logical(-p.x, -p.y, scale);
                            (dx.abs() > 0.001 || dy.abs() > 0.001).then_some(
                                Input::Device(Self::hash(device_id), DeviceInput::Mouse(
                                    mouse, MouseState::Scroll(dx, dy),
                                ))
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
                Some(Input::Device(
                    Self::hash(device_id), DeviceInput::Mouse(mouse, state),
                ))
            },
            WindowEvent::Touch(touch) => {
                self.mouse = (touch.location.x, touch.location.y);
                let position = Self::logical(touch.location.x, touch.location.y, scale);
                match touch.phase {
                    TouchPhase::Started => {
                        self.scroll  = Some(position);
                        self.touching = true;
                        Some(Input::Device(u64::MAX, DeviceInput::Mouse(
                            position, MouseState::Pressed(MouseButton::Left),
                        )))
                    }
                    TouchPhase::Ended | TouchPhase::Cancelled => {
                        self.touching = false;
                        self.scroll = None;
                        Some(Input::Device(u64::MAX, DeviceInput::Mouse(
                            position, MouseState::Released(MouseButton::Left),
                        )))
                    }
                    TouchPhase::Moved => {
                        self.scroll.and_then(|(prev_x, prev_y)| {
                            self.scroll = Some(position);
                            let dx = position.0 - prev_x;
                            let dy = position.1 - prev_y;
                            let scroll_x = -dx;
                            let scroll_y = -dy;
                            (scroll_x.abs() > 0.01 || scroll_y.abs() > 0.01).then_some(
                                Input::Device(u64::MAX, DeviceInput::Mouse(
                                    position, MouseState::Scroll(scroll_x, scroll_y),
                                ))
                            )
                        })
                    }
                }
            },
            WindowEvent::DroppedFile(path) => Some(Input::DroppedFile(path)),
            WindowEvent::HoveredFile(path) => Some(Input::HoveredFile(path)),
            WindowEvent::HoveredFileCancelled => Some(Input::HoveredFileCancelled),
            WindowEvent::Focused(focused) => {
                if !focused {self.scroll = None; self.touching = false;}
                Some(Input::WindowFocused(focused))
            }
            WindowEvent::ModifiersChanged(mods) => {
                self.modifiers = Modifiers{
                    shift: mods.state().shift_key(),
                    control: mods.state().control_key(),
                    alt: mods.state().alt_key(),
                    supermeta: mods.state().super_key(),
                };
                None
            },
            WindowEvent::CursorEntered{device_id} => Some(Input::Device(
                Self::hash(device_id), DeviceInput::CursorEntered,
            )),
            WindowEvent::CursorLeft{device_id} => Some(Input::Device(
                Self::hash(device_id), DeviceInput::CursorLeft,
            )),
            WindowEvent::Moved(position) => {
                let (x, y) = Self::logical(position.x as f64, position.y as f64, scale);
                Some(Input::WindowMoved(x as i32, y as i32))
            },
            e => {log::debug!("Ignored Event: {:?}", e); None}
        }
    }
}
