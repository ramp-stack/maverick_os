use maverick_os::{Application, Context, start};
use maverick_os::air::{self, Contract, Reactants, Reactant, Instance, Name, Service};
use maverick_os::air::names::Id;
use maverick_os::window::{self, Input, KeyEvent, Renderer, Handle};
//use maverick_os::runtime::{Services, Service, async_trait};

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::convert::Infallible;
use std::time::Duration;

use serde::{Serialize, Deserialize};

#[derive(Default)]
pub struct ChatBot(u32, BTreeMap<Id, Instance<Room>>);
impl Service for ChatBot {
    async fn run(&mut self, ctx: &mut air::Context) -> Option<Duration> {
      //match ctx.listen::<Room>() {
      //    Update::Instance(room) => self.1
      //}
      //ctx.list(&ChatRoom::id()).into_iter().for_each(|id| {
      //    ctx.send(id, "/messages", SendMessage("This is an automated message: 'Keep It Quiet!'".to_string())).unwrap();
      //});
        Some(Duration::from_secs(5))
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
        room.messages.len()
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
        let room = ctx.air.create::<Room>("The Room".to_string());
        DemoApplication(room)
    }
    fn on_input(&mut self, ctx: &mut Context, input: Input) {
        if let Input::Keyboard{event: KeyEvent{text: Some(text), ..}, ..} = input {
            self.0.apply(SendMessage(text.to_string()));
        }
        log::info!("\n\n\n\n\n\n\n\n\n\n\n\n\nRoom: {:#?}", self.0.pending());
        //log::info!("CRoom: {:#?}", self.0.confirmed());
    }
    
    //fn services() -> Services {vec![Box::new(ChatBot)]}
}

start!(DemoApplication);
