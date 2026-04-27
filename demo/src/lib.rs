use maverick_os::{Application, Context, start};
use maverick_os::air::{Contracts, Contract, Substance, Id, Reactants, Reactant, Beaker, Name};
use maverick_os::window::{self, Input, KeyEvent, Renderer, Handle};

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::convert::Infallible;

use serde::{Serialize, Deserialize};

//  pub struct ChatBot;
//  impl Service for ChatBot {
//      async fn run(&mut self, ctx: &mut air::Context) -> Option<Duration> {
//          ctx.query.
//          Some(Duration::from_millis(1))
//      }
//  }

#[derive(Serialize, Deserialize, Hash)]
pub struct ChatRoom;
impl ChatRoom {
    pub fn new(_name: &str) -> Self {ChatRoom}
}
impl Contract for ChatRoom {
    fn id() -> Id {Id::hash("ChatRoom2.4")}

    fn init(self, signer: &Name, _timestamp: u64) -> Substance {Substance::Map(BTreeMap::from([
        ("name".to_string(), Substance::String("myroom".to_string())),
        ("author".to_string(), Substance::String(signer.to_string())),
        ("messages".to_string(), Substance::map())
    ]))}

    fn routes() -> BTreeMap<PathBuf, Reactants> {
        BTreeMap::from([
            (PathBuf::from("/name"), Reactants::new().add::<ChangeName>()),
            (PathBuf::from("/messages"), Reactants::new().add::<SendMessage>())
        ])
    }
}

#[derive(Serialize, Deserialize, Hash)]
pub struct ChangeName(String);
impl Reactant for ChangeName {
    type Error = Infallible;
    type Contract = ChatRoom;

    fn apply<B: Beaker>(self, _path: &Path, signer: &Name, _timestamp: u64, substance: &mut B) -> Result<(), Self::Error> {
        if substance.query("/author") == Ok(Substance::String(signer.to_string())) {
            let _ = substance.insert("/name", Substance::String(self.0));
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Hash)]
pub struct SendMessage(String);
impl Reactant for SendMessage {
    type Error = Infallible;
    type Contract = ChatRoom;

    fn apply<B: Beaker>(self, _path: &Path, signer: &Name, timestamp: u64, substance: &mut B) -> Result<(), Self::Error> {
        let _ = substance.insert("/messages/-", Substance::Map(BTreeMap::from([
            ("author".to_string(), Substance::String(signer.to_string())),
            ("timestamp".to_string(), Substance::Integer(timestamp as i64)),
            ("body".to_string(), Substance::String(self.0)),
        ])));
        Ok(())
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

pub struct DemoApplication(Id);
impl Application for DemoApplication {
    type Renderer<'surface> = DemoRenderer<'surface>;

    fn new(ctx: &Context) -> Self {
        let id = ctx.air.create(ChatRoom).unwrap();
        ctx.air.send(id, "/name", ChangeName("The Room".to_string())).unwrap();
        DemoApplication(id)
    }
    fn on_input(&mut self, ctx: &Context, input: Input) {
        if let Input::Keyboard{event: KeyEvent{text: Some(text), ..}, ..} = input {
            ctx.air.send(self.0, "/name", SendMessage(text.to_string())).unwrap();
        }
        if let Some(r) = ctx.air.get::<ChatRoom>(&self.0).and_then(|t| t.query("/").ok()) {
            log::info!("Room: {:?}", r)
        }
    }
    
    fn contracts() -> Contracts {Contracts::new().add::<ChatRoom>()}
}

start!(DemoApplication);
