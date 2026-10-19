
#[cfg(target_os="android")]
use winit::platform::android::activity::AndroidApp;
#[cfg(target_os="android")]
use winit::platform::android::EventLoopBuilderExtAndroid;

use std::sync::Arc;

use winit::window::Window as WinitWindow;
use winit::window::WindowId;
use winit::event_loop::ActiveEventLoop;

use crate::hardware::SafeArea;

use raw_window_handle::{DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle};

#[derive(Clone)]
pub struct Window(Arc<WinitWindow>);
impl Window {
    pub fn new(event_loop: &ActiveEventLoop, name: String) -> Self {
        Self(Arc::new(event_loop.create_window(
            WinitWindow::default_attributes().with_title(&name)
        ).unwrap()))
    }

    pub fn id(&self) -> WindowId {self.0.id()}

    pub fn request_redraw(&self) {self.0.request_redraw()}

    pub fn logical(&self) -> LogicalWindow {
        let size = self.0.inner_size();
        let scale_factor = self.0.scale_factor();
        LogicalWindow{
            size: (
                (size.width as f64 / scale_factor) as f32,
                (size.height as f64 / scale_factor) as f32
            ),
            insets: SafeArea::get(),
        }
    }
    pub fn physical(&self) -> PhysicalWindow {
        let size = self.0.inner_size();
        let scale_factor = self.0.scale_factor();
        PhysicalWindow{width: size.width, height: size.height, scale_factor}
    }
}

impl HasWindowHandle for Window {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {self.0.window_handle()}
}

impl HasDisplayHandle for Window {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {self.0.display_handle()}
}

pub struct PhysicalWindow{
    pub width: u32,
    pub height: u32,
    pub scale_factor: f64
}

#[derive(Clone, Debug, PartialEq)]
pub struct LogicalWindow {
    pub size: (f32, f32),
    pub insets: (f32, f32, f32, f32),
}

//  pub(crate) struct Window<A: Application>{
//      winit: Arc<WinitWindow>,
//      handle: &'static dyn Handle,
//      renderer: Option<A::Renderer<'static>>
//  }
//  impl<A: Application> Window<A> {
//      pub fn id(&self) -> WindowId {self.0.id()}
//      pub fn new(window: WinitWindow) -> (Self, PhysicalWindow) {
//          let window = Arc::new(window);
//          let handle: &'static dyn Handle = unsafe {
//              std::mem::transmute::<&dyn Handle, &'static dyn Handle>(&*window)
//          };
//          let renderer = A::Renderer::new(physical, handle);
//          Surface(window, handle, Some(renderer))
//      }
//      pub fn suspend(&mut self) {self.2 = None;}
//      pub fn resurface(&mut self, physical: &PhysicalWindow) {self.2 = Some(A::Renderer::new(physical, self.1));}
//      pub fn resize(&mut self, physical: &PhysicalWindow) {self.as_mut().map(|r| r.resize(physical))}
//      pub fn request_redraw(&mut self) {self.0.request_redraw()}
//      pub fn as_mut(&mut self) -> Option<&mut A::Renderer<'static>> {self.2.as_mut()}
//  }

//  pub(crate) struct App<A: Application>{
//      maverick: Option<MaverickOS<A>>,
//    //modifiers: Modifiers,
//    //physical_window: PhysicalWindow,
//    //touching: bool,
//    //scroll: Option<(f32, f32)>,
//    //mouse: (f32, f32),
//  }
//  impl<A: Application> Default for App<A> {fn default() -> Self {Self{
//      maverick: None,
//    //modifiers: Modifiers::default(),
//    //physical_window: PhysicalWindow::default(),
//    //touching: false,
//    //scroll: None,
//    //mouse: (0.0, 0.0)
//  }}}

//  //  impl<A: Application> App<A> {
//  //      #[cfg(target_os = "android")]
//  //      pub fn start(app: AndroidApp) {
//  //          EventLoop::builder().with_android_app(app).build().unwrap().run_app(&mut Self::default()).unwrap();
//  //      }

//  //      #[cfg(target_arch = "wasm32")]
//  //      pub fn start() {
//  //          EventLoop::new().unwrap().spawn_app(Self::default()).unwrap();
//  //      }

//  //      #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
//  //      pub fn start() {
//  //          EventLoop::new().unwrap().run_app(&mut Self::default()).unwrap();
//  //      }
//  //  }
//  //  impl<A: Application> ApplicationHandler for App<A> {
//  //      fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
//  //          if let Some(maverick) = self.maverick.as_mut() && let StartCause::ResumeTimeReached{..} = cause {
//  //              maverick.surface.request_redraw()
//  //          }
//  //      }

//  //      fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
//  //          if let Some(maverick) = self.maverick.as_mut() {
//  //              //maverick.runtime.pause();
//  //              maverick.surface.suspend();
//  //          }
//  //      }

//  //      fn device_event(&mut self, _event_loop: &ActiveEventLoop, _device_id: DeviceId, event: DeviceEvent) {
//  //          log::debug!("Ignored Device Event: {:?}", event);
//  //      }

//  //      fn resumed(&mut self, event_loop: &ActiveEventLoop) {match &mut self.maverick {
//  //          Some(maverick) => {
//  //              //maverick.runtime.resume();
//  //              maverick.surface.resurface(&maverick.context.window);
//  //          },
//  //          none => {
//  //              let winit_window = event_loop.create_window(WinitWindow::default_attributes().with_title("orange")).unwrap();
//  //              let scale_factor = winit_window.scale_factor();
//  //              let window = Window{
//  //                  size: (
//  //                      (size.width as f64 / scale_factor) as f32,
//  //                      (size.height as f64 / scale_factor) as f32
//  //                  ),
//  //                  safe_area_insets: SafeArea::get()
//  //              };
//  //              self.physical_window = PhysicalWindow(size.width, size.height, scale_factor);
//  //              let surface = Surface::new(winit_window, &self.physical_window);
//  //              *none = Some(MaverickOS::new(window, surface));
//  //          }
//  //      }}

//  //      fn memory_warning(&mut self, _event_loop: &ActiveEventLoop) {
//  //          log::warn!("Memory Warning");
//  //      }

//  //      fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
//  //          if let Some(maverick) = self.maverick.as_mut() && id == maverick.surface.id() && let Some(event) = match event {
//  //              WindowEvent::CloseRequested | WindowEvent::Destroyed => {
//  //                  let maverick = self.maverick.take().unwrap();
//  //                  maverick.context.runtime.shutdown();
//  //                  event_loop.exit();
//  //                  return;
//  //              },
//  //              WindowEvent::RedrawRequested => {
//  //                  event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now()+TICK));
//  //                  maverick.app.on_tick(&maverick.context, &mut maverick.renderer);
//  //
//  //                  for event in maverick.context.hardware.tick() {
//  //                      maverick.app.on_input(&maverick.context, event);
//  //                  }
//  //                  if let Some(surface) = maverick.surface.as_mut() {
//  //                      surface.draw(&maverick.context.window, &maverick.app);
//  //                  } else {log::warn!("Redraw Requested Without A Valid Surface");}
//  //                  None
//  //              },
//  //              WindowEvent::Resized(size) => {
//  //                  maverick.context.window.width = size.width;
//  //                  maverick.context.window.height = size.height;
//  //                  if let Some(surface) = maverick.surface.as_mut() {
//  //                      surface.resize(&maverick.context.window);
//  //                  } else {log::warn!("Resize Requested Without A Valid Surface");}
//  //                  Some(Input::Resized)
//  //              },
//  //              WindowEvent::ScaleFactorChanged{scale_factor, ..} => {
//  //                  maverick.context.window.scale_factor = scale_factor;
//  //                  if let Some(surface) = maverick.surface.as_mut() {
//  //                      surface.resize(&maverick.context.window);
//  //                  } else {log::warn!("Resize Requested Without A Valid Surface");}
//  //                  Some(Input::Resized)
//  //              },
//  //              WindowEvent::Focused(focused) => Some(Input::WindowFocused(focused)),
//  //              WindowEvent::KeyboardInput{device_id, event, ..} => match event.logical_key {
//  //                  WinitKey::Named(named) => match named {
//  //                      WinitNamedKey::Enter => Some(Key::Enter),
//  //                      WinitNamedKey::Tab => Some(Key::Tab),
//  //                      WinitNamedKey::Space => Some(Key::Character(' ')),
//  //                      WinitNamedKey::Escape => Some(Key::Escape),
//  //                      WinitNamedKey::ArrowUp => Some(Key::Up),
//  //                      WinitNamedKey::ArrowDown => Some(Key::Down),
//  //                      WinitNamedKey::ArrowLeft => Some(Key::Left),
//  //                      WinitNamedKey::ArrowRight => Some(Key::Right),
//  //                      WinitNamedKey::Delete => Some(Key::Delete),
//  //                      WinitNamedKey::Backspace => Some(Key::Backspace),
//  //                      WinitNamedKey::Home => Some(Key::Home),
//  //                      WinitNamedKey::End => Some(Key::End),
//  //                      WinitNamedKey::Shift => Some(Key::Shift),
//  //                      WinitNamedKey::Control => Some(Key::Control),
//  //                      WinitNamedKey::Alt => Some(Key::Alt),
//  //                      WinitNamedKey::Super => Some(Key::SuperMeta),
//  //                      WinitNamedKey::Meta => Some(Key::SuperMeta),
//  //                      WinitNamedKey::CapsLock => Some(Key::CapsLock),
//  //                      WinitNamedKey::NumLock => Some(Key::NumLock),
//  //                      WinitNamedKey::ScrollLock => Some(Key::ScrollLock),
//  //                      _ => None,
//  //                  },
//  //                  WinitKey::Character(c) => Some(Key::Character(c.chars().next().unwrap())),
//  //                  _ => None,
//  //              }.map(|key| Input::Device(device_id, DeviceInput::Keyboard(key, match event.state {
//  //                  ElementState::Pressed if event.repeat => KeyboardState::Repeated,
//  //                  ElementState::Pressed => KeyboardState::Pressed,
//  //                  ElementState::Released => KeyboardState::Released,
//  //              }, self.modifiers))),
//  //              WindowEvent::CursorMoved{device_id, position} if self.mouse != position.into() => {
//  //                  self.mouse = position.into();
//  //                  Some(Input::Device(device_id, DeviceInput::Mouse(self.mouse, MouseState::Moved)))
//  //              },
//  //              WindowEvent::MouseWheel{device_id, delta, phase} => match delta {
//  //                  MouseScrollDelta::LineDelta(x, y) => {
//  //                      let shift = self.modifiers.shift;
//  //                      let (dx, dy) = if shift {(-y, -x)} else {(-x, -y)};
//  //                      (dx.abs() > 0.001 || dy.abs() > 0.001).then_some(
//  //                          Input::Device(device_id, DeviceInput::Mouse(self.mouse, MouseState::Scroll(dx, dy)))
//  //                      )
//  //                  }
//  //                  MouseScrollDelta::PixelDelta(p) => {
//  //                      match phase {
//  //                          TouchPhase::Started => {
//  //                              self.scroll = Some((0.0, 0.0));
//  //                              Some(Input::Device(device_id, DeviceInput::Mouse(self.mouse, MouseState::Scroll(0.0, 0.0))))
//  //                          }
//  //                          TouchPhase::Moved => {
//  //                              let dx = -(p.x as f32);
//  //                              let dy = -(p.y as f32);
//  //                              (dx.abs() > 0.001 || dy.abs() > 0.001).then_some(
//  //                                  Input::Device(device_id, DeviceInput::Mouse(self.mouse, MouseState::Scroll(dx, dy)))
//  //                              )
//  //                          }
//  //                          TouchPhase::Ended | TouchPhase::Cancelled => {
//  //                              self.scroll = None;
//  //                              None
//  //                          }
//  //                      }
//  //                  }
//  //              },
//  //              WindowEvent::MouseInput{device_id, state, button} => {
//  //                  let button = match button {
//  //                      WinitMouseButton::Right => MouseButton::Right,
//  //                      WinitMouseButton::Middle => MouseButton::Middle,
//  //                      _ => MouseButton::Left,
//  //                  };
//  //                  let state = match state {
//  //                      ElementState::Pressed => MouseState::Pressed(button),
//  //                      ElementState::Released => MouseState::Released(button),
//  //                  };
//  //                  Some(Input::Device(device_id, DeviceInput::Mouse(self.mouse, state)))
//  //              },
//  //              WindowEvent::Touch(touch) => {
//  //                  let position = (touch.location.x as f32, touch.location.y as f32);
//  //                  self.mouse = position;
//  //                  match touch.phase {
//  //                      TouchPhase::Started => {
//  //                          self.scroll  = Some(position);
//  //                          self.touching = true;
//  //                          Some(Input::Device(DeviceId::dummy(), DeviceInput::Mouse(self.mouse, MouseState::Pressed(MouseButton::Left))))
//  //                      }
//  //                      TouchPhase::Ended | TouchPhase::Cancelled => {
//  //                          self.touching = false;
//  //                          Some(Input::Device(DeviceId::dummy(), DeviceInput::Mouse(self.mouse, MouseState::Released(MouseButton::Left))))
//  //                      }
//  //                      TouchPhase::Moved => {
//  //                          self.scroll.and_then(|(prev_x, prev_y)| {
//  //                              self.scroll = Some(position);
//  //                              let dx = position.0 - prev_x;
//  //                              let dy = position.1 - prev_y;
//  //                              let scroll_x = -dx;
//  //                              let scroll_y = -dy;
//  //                              (scroll_x.abs() > 0.01 || scroll_y.abs() > 0.01).then_some(
//  //                                  Input::Device(DeviceId::dummy(), DeviceInput::Mouse(self.mouse, MouseState::Scroll(scroll_x, scroll_y)))
//  //                              )
//  //                          })
//  //                      }
//  //                  }
//  //              },
//  //              WindowEvent::DroppedFile(path) => Some(Input::DroppedFile(path)),
//  //              WindowEvent::HoveredFile(path) => Some(Input::HoveredFile(path)),
//  //              WindowEvent::HoveredFileCancelled => Some(Input::HoveredFileCancelled),
//  //              WindowEvent::ModifiersChanged(mods) => {
//  //                  self.modifiers = Modifiers{
//  //                      shift: mods.state().shift_key(),
//  //                      control: mods.state().control_key(),
//  //                      alt: mods.state().alt_key(),
//  //                      supermeta: mods.state().super_key(),
//  //                  };
//  //                  None
//  //              },
//  //              WindowEvent::CursorEntered{device_id} => Some(Input::Device(device_id, DeviceInput::CursorEntered)),
//  //              WindowEvent::CursorLeft{device_id} => Some(Input::Device(device_id, DeviceInput::CursorLeft)),
//  //              WindowEvent::Moved(position) => Some(Input::WindowMoved(position.x, position.y)),
//  //              e => {log::debug!("Ignored Event: {:?}", e); return;}
//  //          } {
//  //              maverick.app.on_input(&maverick.context, event);
//  //          }
//  //      }
//  //  }
