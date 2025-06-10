use std::time::Duration;

use crate::runtime::{self, Channel, Service, ServiceContext};
use crate::hardware;

use air::orange_name::{OrangeSecret, OrangeResolver, OrangeName};
use air::server::{Purser, Error};
use air::storage::{PublicItem, Filter, Client};
use air::{DateTime, Id};
use serde::{Serialize, Deserialize};

pub extern crate air;

pub struct AirService{
    pub resolver: OrangeResolver,
    secret: OrangeSecret,
    pub purser: Purser
}

impl AirService {
    pub fn my_name(&self) -> OrangeName {self.secret.name()}
    pub async fn create_public(&mut self, item: PublicItem) -> Result<Id, Error> {
        let endpoint = self.resolver.endpoint(&self.secret.name(), None, None).await?;
        let c = Client::create_public(&mut self.resolver, &self.secret, item).await?;
        let res = self.purser.send(&mut self.resolver, &endpoint, c.build_request()).await?;
        Ok(c.process_response(&mut self.resolver, res).await?.create_public())
    }
    pub async fn update_public(&mut self, id: Id, item: PublicItem) -> Result<(), Error> {
        let endpoint = self.resolver.endpoint(&self.secret.name(), None, None).await?;
        let c = Client::update_public(&mut self.resolver, &self.secret, id, item).await?;
        let res = self.purser.send(&mut self.resolver, &endpoint, c.build_request()).await?;
        c.process_response(&mut self.resolver, res).await?.assert_empty();
        Ok(())
    }
    pub async fn read_public(&mut self, filter: Filter) -> Result<Vec<(Id, OrangeName, PublicItem, DateTime)>, Error> {
        let endpoint = self.resolver.endpoint(&self.secret.name(), None, None).await?;
        let c = Client::read_public(filter);
        let res = self.purser.send(&mut self.resolver, &endpoint, c.build_request()).await?;
        Ok(c.process_response(&mut self.resolver, res).await?.read_public())
    }
}

//In order to aggrigate api calls Turn the Air Service into a Compiler(again) and accept cmds

#[async_trait::async_trait]
impl Service for AirService {
    async fn new(hardware: &mut hardware::Context) -> Self {
        //TODO: check cloud too
        let secret = if let COrangeSecret(Some(s)) = hardware.cache.get::<COrangeSecret>().await {s} else {
            let sec = OrangeSecret::new();
            hardware.cache.set(&COrangeSecret(Some(sec.clone()))).await;
            sec
        };
        AirService{
            resolver: OrangeResolver,
            secret,
            purser: Purser::new()
        }
    }

    ///This service does not have any repeating tasks and accepts no messages from main
    async fn run(&mut self, _ctx: &mut ServiceContext, _channel: &mut Channel) -> Result<Duration, runtime::Error> {
        Ok(Duration::from_secs(10000))
    }
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
struct COrangeSecret(Option<OrangeSecret>);