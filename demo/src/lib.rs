use maverick_os::{Application, Context, Event, start, Dir};
use maverick_os::air::{Contracts, Contract, Substance, Id, Reactants, Reactant, Beaker, Name};
use maverick_os::window::{Input, KeyEvent};

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::convert::Infallible;

use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Hash)]
pub struct ChatRoom(String);
impl ChatRoom {
    pub fn new(name: &str) -> Self {ChatRoom(name.to_string())}
}
impl Contract for ChatRoom {
    fn id() -> Id {Id::hash("ChatRoom")}

    fn init(self, signer: &Name, _timestamp: u64) -> Substance {Substance::Map(BTreeMap::from([
        ("name".to_string(), Substance::String(self.0)),
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

pub struct DemoApplication(Id);
impl Application for DemoApplication {
    fn new(ctx: &mut Context, _dir: Dir<'static>) -> Self {
        let id = ctx.air.create(ChatRoom("Goodbye".to_string())).unwrap();
        //ctx.air.send(id, "/name", ChangeName("INIT".to_string())).unwrap();
        DemoApplication(id)
    }
    fn on_event(&mut self, ctx: &mut Context, event: Event) {
        if let Event::Input(Input::Keyboard{event: KeyEvent{text: Some(text), ..}, ..}) = event {
            ctx.air.send(self.0, "/name", ChangeName(text.to_string())).unwrap();
        }
        if let Some(r) = ctx.air.get::<ChatRoom>(&self.0).and_then(|t| t.query("/name").ok()) {
            log::info!("Room Name: {:?}", r)
        }
    }

    fn contracts() -> Contracts {Contracts::new().add::<ChatRoom>()}
}

start!(DemoApplication);
