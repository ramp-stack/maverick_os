use predator::{traits, Application, Context, Window, Contracts, Input, Key, Renderer, DeviceInput, KeyboardState, Id, Name, Contract, Metadata, Instance};

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

pub struct DemoRenderer();
impl Renderer for DemoRenderer {
    type Instructions = ();

    fn new(_window: Window) -> Self {DemoRenderer()}
    fn resize(&mut self) {}
    fn draw(&mut self, _instructions: Self::Instructions) {}
}

pub struct DemoApplication(Instance<Room>, bool);
impl Application for DemoApplication {
    type Renderer = DemoRenderer;

    fn name() -> String {"Demo Application Name".to_string()}

    fn new(ctx: &mut Context) -> Self {
        let instances = ctx.air.instances::<Room>();
        let room = instances.create("The Room".to_string());
        DemoApplication(room, false)
    }

    fn contracts<A: traits::Air>(contracts: Contracts<A>) -> Contracts<A> {
        contracts.add::<Room>()
    }

    fn on_tick(&mut self, _ctx: &mut Context) -> <Self::Renderer as Renderer>::Instructions {
        if self.1 || self.0.try_next().is_some() {
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
        self.1 = false;
    }

    fn on_input(&mut self, _ctx: &mut Context, input: Input) {
        if let Input::Device(_, DeviceInput::Keyboard(Key::Character(text), KeyboardState::Pressed, _)) = input {
            self.0.send(text.to_string());
            self.0.share(Name::orange_me());
            self.1 = true;
        }
    }
    
    //fn services() -> Services {Services::default().add::<Lock<ChatBot>>()}
}

maverick_os::start!(DemoApplication);
