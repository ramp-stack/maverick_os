use predator::{Input, Modifiers, Key, DeviceInput, KeyboardState, MouseState, MouseButton};
use winit::event::{WindowEvent, DeviceId, TouchPhase, MouseButton as WinitMouseButton, MouseScrollDelta, ElementState};
use winit::keyboard::{Key as WinitKey, NamedKey as WinitNamedKey};
use std::hash::{DefaultHasher, Hasher, Hash};

fn hash(di: DeviceId) -> u64 {
    let mut hasher = DefaultHasher::default();
    di.hash(&mut hasher);
    hasher.finish()
}

pub enum Event {
    Input(Input),
    Resize,
    Close,
    Draw,
}

#[derive(Default)]
pub struct Events{
    modifiers: Modifiers,
    touching: bool,
    scroll: Option<(f32, f32)>,
    mouse: (f32, f32),
}
impl Events {
    pub fn process(&mut self, event: WindowEvent) -> Option<Event> {
        match event {
            WindowEvent::CloseRequested | WindowEvent::Destroyed => Some(Event::Close),
            WindowEvent::RedrawRequested => Some(Event::Draw),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged{..} => Some(Event::Resize),
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
            }.map(|key| Event::Input(Input::Device(hash(device_id), DeviceInput::Keyboard(key, match event.state {
                ElementState::Pressed if event.repeat => KeyboardState::Repeated,
                ElementState::Pressed => KeyboardState::Pressed,
                ElementState::Released => KeyboardState::Released,
            }, self.modifiers)))),
            WindowEvent::CursorMoved{device_id, position} if self.mouse != position.into() => {
                self.mouse = position.into();
                Some(Event::Input(Input::Device(hash(device_id), DeviceInput::Mouse(self.mouse, MouseState::Moved))))
            },
            WindowEvent::MouseWheel{device_id, delta, phase} => match delta {
                MouseScrollDelta::LineDelta(x, y) => {
                    let shift = self.modifiers.shift;
                    let (dx, dy) = if shift {(-y, -x)} else {(-x, -y)};
                    (dx.abs() > 0.001 || dy.abs() > 0.001).then_some(
                        Event::Input(Input::Device(hash(device_id), DeviceInput::Mouse(self.mouse, MouseState::Scroll(dx, dy))))
                    )
                }
                MouseScrollDelta::PixelDelta(p) => {
                    match phase {
                        TouchPhase::Started => {
                            self.scroll = Some((0.0, 0.0));
                            Some(Event::Input(Input::Device(hash(device_id), DeviceInput::Mouse(self.mouse, MouseState::Scroll(0.0, 0.0)))))
                        }
                        TouchPhase::Moved => {
                            let dx = -(p.x as f32);
                            let dy = -(p.y as f32);
                            (dx.abs() > 0.001 || dy.abs() > 0.001).then_some(
                                Event::Input(Input::Device(hash(device_id), DeviceInput::Mouse(self.mouse, MouseState::Scroll(dx, dy))))
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
                Some(Event::Input(Input::Device(hash(device_id), DeviceInput::Mouse(self.mouse, state))))
            },
            WindowEvent::Touch(touch) => {
                let position = (touch.location.x as f32, touch.location.y as f32);
                self.mouse = position;
                match touch.phase {
                    TouchPhase::Started => {
                        self.scroll  = Some(position);
                        self.touching = true;
                        Some(Event::Input(Input::Device(u64::MAX, DeviceInput::Mouse(self.mouse, MouseState::Pressed(MouseButton::Left)))))
                    }
                    TouchPhase::Ended | TouchPhase::Cancelled => {
                        self.touching = false;
                        Some(Event::Input(Input::Device(u64::MAX, DeviceInput::Mouse(self.mouse, MouseState::Released(MouseButton::Left)))))
                    }
                    TouchPhase::Moved => {
                        self.scroll.and_then(|(prev_x, prev_y)| {
                            self.scroll = Some(position);
                            let dx = position.0 - prev_x;
                            let dy = position.1 - prev_y;
                            let scroll_x = -dx;
                            let scroll_y = -dy;
                            (scroll_x.abs() > 0.01 || scroll_y.abs() > 0.01).then_some(
                                Event::Input(Input::Device(u64::MAX, DeviceInput::Mouse(self.mouse, MouseState::Scroll(scroll_x, scroll_y))))
                            )
                        })
                    }
                }
            },
            WindowEvent::DroppedFile(path) => Some(Event::Input(Input::DroppedFile(path))),
            WindowEvent::HoveredFile(path) => Some(Event::Input(Input::HoveredFile(path))),
            WindowEvent::HoveredFileCancelled => Some(Event::Input(Input::HoveredFileCancelled)),
            WindowEvent::ModifiersChanged(mods) => {
                self.modifiers = Modifiers{
                    shift: mods.state().shift_key(),
                    control: mods.state().control_key(),
                    alt: mods.state().alt_key(),
                    supermeta: mods.state().super_key(),
                };
                None
            },
            WindowEvent::CursorEntered{device_id} => Some(Event::Input(Input::Device(hash(device_id), DeviceInput::CursorEntered))),
            WindowEvent::CursorLeft{device_id} => Some(Event::Input(Input::Device(hash(device_id), DeviceInput::CursorLeft))),
            WindowEvent::Moved(position) => Some(Event::Input(Input::WindowMoved(position.x, position.y))),
            e => {log::debug!("Ignored Event: {:?}", e); None}
        }
    }
}
