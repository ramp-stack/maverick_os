use std::time::Duration;

use crate::runtime::{Callback, Channel, Service, ServiceContext};
use crate::hardware;
use crate::{Cache, State};

use air::orange_name::{DefaultOrangeResolver, OrangeSecret};
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
struct COrangeSecret(Option<OrangeSecret>);

pub struct AirService{
    resolver: DefaultOrangeResolver,
    secret: OrangeSecret,
    purser: DefaultPurser
}

impl AirService {
    pub async fn create_public(item: PublicItem) -> Result<Id, Error> {
    }
    pub async fn read_public(filter: Filter) -> Result<Vec<(Id, OrangeName, PublicItem, DateTime)>, Error> {
    }
}

#[async_trait::async_trait]
impl Service for AirService {
    pub async fn new(hardware: &mut hardware::Context) -> Self {
        //TODO: check cloud too
        let secret = hardware.cache.get::<COrangeSecret>().await.0.unwrap_or_else(|| {
            let sec = OrangeSecret::new();
            hardware.cache.set(&COrangeSecret(Some(sec.clone()))).await;
            sec
        });
        AirService{
            resolver: DefaultOrangeResolver,
            secret,
            purser: DefaultPurser::new(TcpClient, DefaultOrangeResolver)
        }
    }

    ///This service does not have any repeating tasks and accepts no messages from main
    async fn run(&mut self, ctx: &mut ServiceContext, channel: &mut Channel) -> Duration {
        Duration::from_secs(10000)
    }
}



//impl Service for OrangeResolver {}

//  pub struct Air {
//      pub i: u32
//  }

//  impl AirTask {
//      fn callback(state: &mut State, received: String) {println!("Received: {:?}", received)}
//  }
//      fn active_tasks(&self) -> Vec<Box<dyn Task>> {vec![]}

//  #[async_trait::async_trait]
//  impl Task for AirTask {
//      fn id(&self) -> [u8; 32] {[20; 32]}

//      async fn run(&mut self, cache: &mut Cache, channel: &mut Channel) -> Duration {
//          while let Some(data) = channel.receive() {
//              self.i += 1;
//              channel.send(format!("Processed: {data} {}", self.i));
//          }
//          println!("HELLO");
//          Duration::from_secs(1)
//      }
//      //Move to _Trait
//      fn callback(&self) -> Box<Callback> {Box::new(Self::callback)}
//  }
