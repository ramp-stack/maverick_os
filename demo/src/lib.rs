use maverick_os::{Application, Context, start};
use maverick_os::air::{Contract, Instance, Name, Metadata, Id};
use maverick_os::window::{self, Input, Key, Renderer, Handle, DeviceInput, KeyboardState};

use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Clone, Debug, Hash, PartialEq, Eq)]
pub struct Message {
    author: Name,
    timestamp: u64,
    body: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Hash, PartialEq, Eq)]
pub struct Room {
    author: Name,
    name: String,
    messages: Vec<Message>
}
impl Contract for Room {
    type Init = String;
    type Message = String;
    type Result = usize;

    fn id() -> Id {Id::hash("Room")}

    fn init(init: Self::Init, metadata: Metadata) -> Self {
        Room {
            author: metadata.signer,
            name: init, 
            messages: Vec::new()
        }
    }

    fn apply(&mut self, message: Self::Message, metadata: Metadata) -> Self::Result {
        self.messages.push(Message{
            author: metadata.signer,
            timestamp: metadata.timestamp,
            body: message
        });
        self.messages.len()
    }
}

pub struct DemoRenderer<'surface>(&'surface dyn Handle);
impl<'surface> Renderer<'surface> for DemoRenderer<'surface> {
    type Application = DemoApplication;
    fn new(_context: &window::Context, handle: &'surface dyn Handle) -> Self {DemoRenderer(handle)}
    fn resize(&mut self, _context: &window::Context) {}
    fn draw(&mut self, _context: &window::Context, _app: &Self::Application) {
        self.0.display_handle().unwrap();
    }
}

pub struct DemoApplication(Instance<Room>);
impl Application for DemoApplication {
    type Renderer<'surface> = DemoRenderer<'surface>;

    fn new(ctx: &Context) -> Self {
        let instances = ctx.air.instances::<Room>();
        let room = instances.create("The Room".to_string());
        DemoApplication(room)
    }

    fn on_input(&mut self, _ctx: &Context, input: Input) {
        let s = if let Input::Device(_, DeviceInput::Keyboard(Key::Character(text), KeyboardState::Pressed, _)) = input {
            self.0.send(text.to_string());
            self.0.share(Name::orange_me());
            true
        } else {false};
        if input == Input::Tick && self.0.try_next().is_some() || s {
            let vec = self.0.pending().messages.iter().map(|m| m.body.clone()).collect::<Vec<_>>();
            let vec2 = self.0.confirmed().unwrap().messages.iter().map(|m| m.body.clone()).collect::<Vec<_>>();
            log::info!(
                "\n\n\nPending Room: {:?} : {:#?} \nConfirm Room: {:?} : {:#?}",
                self.0.pending().name,
                &vec[vec.len().saturating_sub(20)..].concat(),
                self.0.confirmed().unwrap().name,
                &vec2[vec2.len().saturating_sub(20)..].concat()
            );
        }
    }
    
    //fn services() -> Services {Services::default().add::<Lock<ChatBot>>()}
}

start!(DemoApplication);
