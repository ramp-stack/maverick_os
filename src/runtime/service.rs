
use std::collections::BTreeMap;
use std::time::Duration;
use std::future::Future;
use std::any::TypeId;
use std::pin::Pin;

use super::{Channel, Error};
use crate::{hardware, State};
use crate::runtime::thread::{Thread, Context};

pub type ServiceList = BTreeMap<TypeId, Box<dyn for<'a> FnOnce(&'a mut hardware::Context) -> Pin<Box<dyn Future<Output = Box<dyn Thread>> + 'a>>>>;

pub trait Services {
    fn services() -> ServiceList {BTreeMap::new()}
}

#[async_trait::async_trait]
pub trait BackgroundTask {
    async fn run(&mut self, ctx: &mut hardware::Context) -> Result<Duration, Error>;
}


#[async_trait::async_trait]
pub trait Service: Send {
    type Send;
    type Receive;

    async fn new(ctx: &mut hardware::Context) -> Self where Self: Sized;

    async fn run(&mut self, ctx: &mut Context<Self::Send, Self::Receive>) -> Result<Duration, Error>;

    fn callback(state: &mut State, payload: String); //-> Box<Callback> {Box::new(|_state: &mut State, _response: String| {})}

  //fn background_tasks(&self) -> Vec<Box<dyn BackgroundTask>> {vec![]}
  //fn services(&self) -> ServiceList {BTreeMap::new()}
}
