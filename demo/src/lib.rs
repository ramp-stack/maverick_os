use maverick_os::{Application, Context, start};
use maverick_os::air::{self, Contract, Reactants, Reactant, Instance, Name, Service, Services, Metadata, Secret, Lock, Instances, Update};
use maverick_os::air::names::Id;
use maverick_os::window::{self, Input, Key, Renderer, Handle, DeviceInput, KeyboardState};

use serde::{Serialize, Deserialize};

pub struct ChatBot(Instances<Room>);
impl Service for ChatBot {
    fn id() -> Id {Id::hash("CHATBOT")}
    async fn new(ctx: &mut air::Context, _secret: Secret) -> Self {
        let mut instances = ctx.instances();
        for room in instances.values_mut() {
            room.apply(SendMessage("ChatBot has Joined".to_string()));
        }
        ChatBot(instances)
    }
    async fn run(&mut self, ctx: &mut air::Context) {
        match self.0.listen().await {
            (instance, Update::New) => {
                instance.apply(SendMessage("ChatBot has Joined".to_string()));
            },
            (instance, Update::Confirmed(output)) => {
                if let Some(index) = output.downcast::<SendMessage>()
                && let Some(message) = instance.load_confirmed().messages.get(index)
                && message.author == ctx.me()
                && !message.body.contains("ChatBot") {
                    instance.apply(SendMessage(format!("ChatBot Replying to \"{:.10}...\": I totally agree", message.body)));
                }
            },
            _ => {}
        }
    }

    async fn shutdown(mut self, ctx: &mut air::Context) {
        for room in self.0.values_mut() {
            room.apply(SendMessage("ChatBot Shutting Down".to_string())).confirmed().await;
        }
    }
}

//  pub struct ChatBot(Instances);
//  impl Service for ChatBot {
//      fn id() -> Id {Id::hash("CHATBOT")}
//      async fn new(ctx: &mut air::Context, _secret: Secret) -> Self {ChatBot(ctx.instances())}
//      async fn run(&mut self, ctx: &mut air::Context) {
//          let mut join_set = tokio::task::JoinSet::new();
//          for mut room in self.0.list::<Room>() {
//              join_set.spawn(async {loop {
//                  if let Some(message) = room.listen_confirmed().await.downcast::<SendMessage>() {
//                      break (message, room);
//                  }
//              }});
//          }

//          loop { tokio::select!{
//              instance = self.0.listen() => {
//                  if let Some(mut room) = instance.downcast::<Room>() {
//                      join_set.spawn(async {loop {
//                          if let Some(message) = room.listen_confirmed().await.downcast::<SendMessage>() {
//                              break (message, room);
//                          }
//                      }});
//                  }
//              },
//              Some(Ok((index, mut room))) = join_set.join_next() => {
//                  let message = room.confirmed().messages.get(index).unwrap().clone();
//                  if message.author == ctx.me() && !message.body.contains("ChatBot") {
//                      room.apply(SendMessage(format!("ChatBot Replying to \"{:.10}...\": I totally agree", message.body)));
//                  }
//                  join_set.spawn(async {loop {
//                      if let Some(message) = room.listen_confirmed().await.downcast::<SendMessage>() {
//                          break (message, room);
//                      }
//                  }});
//              }
//          }}
//      }
//      async fn shutdown(self, ctx: &mut air::Context) {
//          for mut room in ctx.list::<Room>() {
//              room.apply(SendMessage("ChatBot Shutting Down".to_string())).confirmed().await;
//          }
//          println!("CHATBOT SHUTDOWN");
//      }
//  }

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

    fn init(init: Self::Init, metadata: Metadata) -> Self {
        Room {
            author: metadata.signer,
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
    type Output = usize;

    fn id() -> Id {Id::hash("SendMessage")}

    fn apply(self, room: &mut Room, metadata: Metadata) -> Self::Output {
        room.messages.push(Message{author: metadata.signer, timestamp: metadata.timestamp, body: self.0});
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

    fn new(ctx: &Context) -> Self {
        println!("Room CID: {:?}", Room::id());
        println!("SendMessage RID: {:?}", SendMessage::id());
        let room = ctx.air.create::<Room>("The Room".to_string());
        DemoApplication(room)
    }
    fn on_input(&mut self, _ctx: &Context, input: Input) {
        if let Input::Device(_, DeviceInput::Keyboard(Key::Character(text), KeyboardState::Pressed, _)) = input {
            self.0.apply(SendMessage(text.to_string()));
            let vec = self.0.load_pending().messages.iter().map(|m| m.body.clone()).collect::<Vec<_>>();
            log::info!(
                "\n\n\nRoom: {:?}, {:#?}",
                self.0.id(),
                &vec[vec.len().saturating_sub(20)..]
            );
        }
    }
    
    fn services() -> Services {Services::default().add::<Lock<ChatBot>>()}
}

start!(DemoApplication);
