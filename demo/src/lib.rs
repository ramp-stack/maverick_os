use maverick_os::inputs::{DeviceInput, Key, KeyboardState};
use serde::{Serialize, Deserialize};

pub struct DemoRenderer();
impl Renderer for DemoRenderer {
    type Instructions = ();

    fn new(_window: Window) -> Self {DemoRenderer()}
    fn resize(&mut self) {}
    fn draw(&mut self, _instructions: Self::Instructions) {}
}

pub struct DemoApplication(Hardware);
impl Application for DemoApplication {
    type Renderer = DemoRenderer;

    fn name() -> String {"Demo Application Name".to_string()}

    fn new(hardware: Hardware) -> Self {
      //let instances = ctx.air.instances::<Room>();
      //let room = instances.create("The Room".to_string());
        DemoApplication(hardware)
    }

    fn on_tick(&mut self) -> <Self::Renderer as Renderer>::Instructions {
      //if self.1 || self.0.try_next().is_some() {
      //    let vec = self.0.pending().messages.iter().map(|m| m.body.clone()).collect::<Vec<_>>();
      //    let vec2 = self.0.confirmed().unwrap().messages.iter().map(|m| m.body.clone()).collect::<Vec<_>>();
      //    log::info!(
      //        "\n\n\nPending Room: {:?} : {:#?} \nConfirm Room: {:?} : {:#?}",
      //        self.0.pending().name,
      //        &vec[vec.len().saturating_sub(20)..].concat(),
      //        self.0.confirmed().unwrap().name,
      //        &vec2[vec2.len().saturating_sub(20)..].concat()
      //    );
      //}
      //self.1 = false;
    }

    fn on_input(&mut self, input: Input) {
        if let Input::Device(_, DeviceInput::Keyboard(Key::Character(text), KeyboardState::Pressed, _)) = input {
          //self.0.send(text.to_string());
          //self.0.share(Name::orange_me());
          //self.1 = true;
        }
    }
    
    //fn services() -> Services {Services::default().add::<Lock<ChatBot>>()}
}

maverick_os::start!(DemoApplication);
