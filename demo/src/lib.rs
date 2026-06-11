use maverick_os::{Application, Context, start};
use maverick_os::air::{self, Contract, Reactants, Reactant, Instance, Name, Service, Services, Listner};
use maverick_os::air::names::Id;
use maverick_os::window::{self, Input, KeyEvent, Renderer, Handle};

use std::time::Duration;

use serde::{Serialize, Deserialize};

#[derive(Default)]
pub struct ChatBot(Listner<Room>);
impl Service for ChatBot {
    async fn run(&mut self, ctx: &mut air::Context) -> Option<Duration> {
        if let (room, Some(update)) = self.0.listen(ctx).await
        && let Some(msg_idx) = update.as_reactant::<_, SendMessage>() {
            let message = room.confirmed().unwrap().messages.get(msg_idx).unwrap().clone();
            if !message.body.contains("ChatBot Quoting") {
                room.apply(SendMessage(format!("ChatBot Quoting {} Saying: \"{}\"", message.author, message.body)));
            }
        }
        Some(Duration::from_secs(0))
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Message {
    author: Name,
    timestamp: u64,
    body: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Room {
    author: Name,
    name: String,
    messages: Vec<Message>
}
impl Contract for Room {
    type Init = String;
    fn id() -> Id {Id::hash("Room")}

    fn init(init: Self::Init, signer: Name, _timestamp: u64) -> Self {
        Room {
            author: signer,
            name: init, 
            messages: Vec::new()
        }
    }

    fn reactants() -> Reactants<Room> {
        Reactants::default().add::<SendMessage>()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SendMessage(String);
impl Reactant<Room> for SendMessage {
    type Result = usize;

    fn id() -> Id {Id::hash("SendMessage")}

    fn apply(self, room: &mut Room, signer: Name, timestamp: u64) -> Self::Result {
        room.messages.push(Message{author: signer, timestamp, body: self.0});
        room.messages.len()-1
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

    fn new(ctx: &mut Context) -> Self {
      //ctx.air.register::<Room>();
      //std::thread::sleep(Duration::from_secs(1));
      //let room = ctx.air.list::<Room>().pop().unwrap();
        let room = ctx.air.create::<Room>("The Room".to_string());
        DemoApplication(room)
    }
    fn on_input(&mut self, _ctx: &mut Context, input: Input) {
        if let Input::Keyboard{event: KeyEvent{text: Some(text), ..}, ..} = input {
            self.0.apply(SendMessage(text.to_string()));
            log::info!("\n\n\n\n\n\n\n\n\n\n\n\n\nRoom: {:#?}", self.0.pending());
        }
    }
    
    fn services() -> Services {Services::default().add(ChatBot::default())}
}

start!(DemoApplication);
