use std::sync::mpsc::{channel, Sender, Receiver};
use std::collections::hash_map::DefaultHasher;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::hash::{Hasher, Hash};
use std::future::Future;
use std::sync::Arc;
use std::any::TypeId;
use std::pin::Pin;
use std::any::Any;

use downcast_rs::{impl_downcast, Downcast};
use tokio::task::JoinHandle;
use serde::{Serialize, Deserialize};
use rand::Rng;

pub use tokio::time::Duration;

use super::{Thread, ThreadContext, ThreadRequest, ThreadResponse, Callback, Id};
use crate::runtime::channel::Channel;
use crate::{hardware, State};

use async_trait::async_trait;

pub struct InlineHandle<S>(Id, PhantomData<S>);

pub struct InlineChannel<S, R> (pub Box<dyn Channel<ThreadResponse, ThreadRequest>>, pub PhantomData<S>, pub PhantomData<R>);
unsafe impl<S, R> Send for InlineChannel<S, R> {}
impl<
    S: Serialize + for<'a> Deserialize <'a>,
    R: Serialize + for<'a> Deserialize <'a>,
>Channel<S, R> for InlineChannel<S, R> {
    fn send(&mut self, payload: &S) {
        self.0.send(&ThreadResponse::Callback(serde_json::to_string(payload).unwrap()));
    }

    fn receive(&mut self) -> Option<R> {
        while let Some(r) = self.0.receive() {
            match r {
                ThreadRequest::Post(r) => return Some(serde_json::from_str(&r).unwrap()),
                _ => todo!()
            }
        }
        None
    }
}

impl<T: Thread + 'static> From<T> for Box<dyn Thread> {
    fn from(t: T) -> Box<dyn Thread> {Box::new(t)}
}

pub struct InlineThread<S, R, Fut, CR>(Box<dyn FnOnce(ThreadContext, Box<dyn Channel<S, R>>) -> Fut + Send>, Option<Box<dyn FnMut(&mut State, CR) -> () + Send>>);
#[async_trait]
impl<
    S: Serialize + for<'a> Deserialize <'a> + 'static,
    R: Serialize + for<'a> Deserialize <'a> + 'static,
    Fut: Future<Output = ()> + Send + 'static,
    CR: Serialize + for<'a> Deserialize <'a> + 'static,
> Thread for InlineThread<S, R, Fut, CR> {
  //type Send = S;
  //type Receive = R;
    async fn run(self: Box<Self>, ctx: ThreadContext, channel: Box<dyn Channel<ThreadResponse, ThreadRequest>>) {
        let mut channel = Box::new(InlineChannel(channel, PhantomData::<S>, PhantomData::<R>));
        (self.0)(ctx, channel).await
    }

    fn callback(&mut self) -> Box<Callback> {
        let mut func = self.1.take().unwrap();
        Box::new(move |state: &mut State, response: String| {
            func(state, serde_json::from_str(&response).unwrap())
        })
    }
}

impl<
    S: Serialize + for<'a> Deserialize <'a> + 'static,
    R: Serialize + for<'a> Deserialize <'a> + 'static,
    Fut: Future<Output = ()> + Send + 'static,
    F: FnOnce(ThreadContext, Box<dyn Channel<S, R>>) -> Fut + Send + 'static
> From<F> for InlineThread<S, R, Fut, String> {
    fn from(f: F) -> Self {
        InlineThread(Box::new(f), Some(Box::new(|_state: &mut State, _response: String| {})))
    }
}

impl<
    S: Serialize + for<'a> Deserialize <'a> + 'static,
    R: Serialize + for<'a> Deserialize <'a> + 'static,
    Fut: Future<Output = ()> + Send + 'static,
    F: FnOnce(ThreadContext, Box<dyn Channel<S, R>>) -> Fut + Send + 'static,
    CR: Serialize + for<'a> Deserialize <'a> + 'static,
    CF: FnMut(&mut State, CR) -> () + Send + 'static
> From<(F, CF)> for InlineThread<S, R, Fut, CR> {
    fn from(f: (F, CF)) -> Self {
        InlineThread(Box::new(f.0), Some(Box::new(f.1)))
    }
}
