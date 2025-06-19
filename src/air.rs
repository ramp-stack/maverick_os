use std::time::Duration;

use crate::runtime::{self, Service as ThreadService, ThreadContext, Services, ServiceList};
use crate::hardware;

use air::orange_name::{OrangeSecret, OrangeResolver};
use air::server::{Purser, Error, Request as AirRequest};
use air::storage::{PublicItem, Filter, Client, Processed};
use air::Id;
use serde::{Serialize, Deserialize};

pub extern crate air;

pub struct Service{
    pub resolver: OrangeResolver,
    secret: OrangeSecret,
    pub purser: Purser
}

impl Service {
  //pub fn my_name(&self) -> OrangeName {self.secret.name()}
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Request {
    CreatePublic(PublicItem),
    ReadPublic(Filter),
    UpdatePublic(Id, PublicItem),
}

//In order to aggrigate api calls Turn the Air Service into a Compiler(again) and accept cmds

#[async_trait::async_trait]
impl ThreadService for Service {
    type Send = Result<Processed, Error>;
    type Receive = Request;

    async fn new(hardware: &mut hardware::Context) -> Self {
        //TODO: check cloud too
        let secret = if let Some(s) = hardware.cache.get("OrangeSecret").await {s} else {
            let sec = OrangeSecret::new();
            hardware.cache.set("OrangeSecret", sec.clone()).await;
            sec
        };
        hardware.cache.set("OrangeName", Some(secret.name())).await;
        Service{
            resolver: OrangeResolver,
            secret,
            purser: Purser::new()
        }
    }

    ///This service does not have any repeating tasks and accepts no messages from main
    async fn run(&mut self, ctx: &mut ThreadContext<Self::Send, Self::Receive>) -> Result<Option<Duration>, runtime::Error> {
        let mut clients = Vec::new();
        let mut requests = Vec::new();

        while let Some((id, request)) = ctx.get_request() {
            let client = match request {
                Request::CreatePublic(item) => Client::create_public(&mut self.resolver, &self.secret, item).await?,
                Request::ReadPublic(filter) => Client::read_public(filter),
                Request::UpdatePublic(id, item) => Client::update_public(&mut self.resolver, &self.secret, id, item).await?,
            };
            requests.push(client.build_request());
            clients.push((client, id));
        }
        let batch = AirRequest::batch(requests);
        let endpoint = self.resolver.endpoint(&self.secret.name(), None, None).await?;
        let res = self.purser.send(&mut self.resolver, &endpoint, batch).await?;
        for (response, (client, id)) in res.batch()?.into_iter().zip(clients) {
            ctx.respond(id, client.process_response(&mut self.resolver, response).await)
        }
        Ok(Some(Duration::from_millis(100)))
    }
}

impl Services for Service {
    fn services() -> ServiceList {ServiceList::default()}
}
