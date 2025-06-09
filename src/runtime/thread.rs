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
use tokio::time::Duration;
use tokio::task::JoinHandle;
use serde::{Serialize, Deserialize};
use rand::Rng;

use crate::runtime::channel::{Channel, SerdeChannel};
use crate::runtime;

use crate::{State, hardware};
use super::{Callback, Id};

#[derive(Serialize, Deserialize, Debug)]
pub enum ThreadRequest<S> {
    Request(Id, S),
    Resume,
    Pause,
    Close,
}
impl<S: for<'a> Deserialize<'a>> ThreadRequest<S> {
    fn from_str(t: ThreadRequest<String>) -> Self {
        match t {
            ThreadRequest::Request(id, string) => ThreadRequest::Request(id, serde_json::from_str(&string).unwrap()),
            ThreadRequest::Resume => ThreadRequest::Resume,
            ThreadRequest::Pause => ThreadRequest::Pause,
            ThreadRequest::Close => ThreadRequest::Close,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub enum ThreadResponse<R> {
    Response(Id, R),
    Error,
}
impl<R: Serialize> ThreadResponse<R> {
    fn to_str(&self) -> ThreadResponse<String> {
        match self {
            ThreadResponse::Response(id, s) => ThreadResponse::Response(*id, serde_json::to_string(s).unwrap()),
            ThreadResponse::Error => ThreadResponse::Error
        }
    }
}

pub struct ThreadContext {
    pub hardware: hardware::Context,
}
impl ThreadContext {
    //pub async fn request<T: Thread>(&mut self, request: T::Receive)

    pub async fn sleep(&mut self, time: Duration) {
        tokio::time::sleep(time).await
    }
}

#[async_trait::async_trait]
pub trait _Thread: Send {
  type Receive: Serialize + for<'a> Deserialize<'a> where Self: Sized;
  //type Send: Serialize + for<'a> Deserialize<'a> where Self: Sized;
  //type CallbackReceive: Serialize + for<'a> Deserialize<'a> where Self: Sized;
    
    async fn run(self: Box<Self>, ctx: ThreadContext, channel: _ThreadChannel);

    fn id(&self) -> Id {rand::rng().random()} 
    fn callback(&mut self) -> Box<Callback> {Box::new(|_state: &mut State, _response: String| {})}
}

pub type _ThreadChannel = SerdeChannel<ThreadResponse<String>, ThreadRequest<String>>;
pub type _ThreadChannelR = SerdeChannel<ThreadRequest<String>, ThreadResponse<String>>;

pub struct ThreadChannel<S, R>(_ThreadChannel, PhantomData<fn() -> S>, PhantomData<fn() -> R>);
impl<
    S: Serialize + for<'a> Deserialize <'a>,
    R: Serialize + for<'a> Deserialize <'a>,
> Channel<ThreadResponse<S>, ThreadRequest<R>> for ThreadChannel<S, R> {
    fn send(&mut self, payload: ThreadResponse<S>) {
        self.0.send(payload.to_str())
    }
    fn receive(&mut self) -> Option<ThreadRequest<R>> {
        self.0.receive().map(|r| ThreadRequest::<R>::from_str(r))
    }
}

pub struct Thread<S, R, Fut>(
    Box<dyn FnOnce(ThreadContext, ThreadChannel<S, R>) -> Fut + Send>,
    Option<Box<dyn FnMut(&mut State, R) -> () + Send>>
);
#[async_trait::async_trait]
impl<
    S: Serialize + for<'a> Deserialize <'a> + 'static,
    R: Serialize + for<'a> Deserialize <'a> + 'static,
    Fut: Future<Output = ()> + Send + 'static,
    //CR: Serialize + for<'a> Deserialize <'a> + 'static,
> _Thread for Thread<S, R, Fut> {
  //type Send = S;
    type Receive = R;
    async fn run(self: Box<Self>, ctx: ThreadContext, channel: _ThreadChannel) {
        let channel = ThreadChannel(channel, PhantomData::<fn() -> S>, PhantomData::<fn() -> R>);
        (self.0)(ctx, channel).await
    }

    fn callback(&mut self) -> Box<Callback> {
        let mut func = self.1.take().unwrap();
        Box::new(move |state: &mut State, response: String| {
            func(state, serde_json::from_str(&response).unwrap())
        })
    }
}


//Validate works for service
//Communication between threads needs to be handle between ctx/channel hybrid. await on ctx and get
//response should be user interface


pub struct InlineChannel<S, R>(ThreadChannel<S, R>);
impl<
    S: Serialize + for<'a> Deserialize <'a>,
    R: Serialize + for<'a> Deserialize <'a>,
>Channel<S, R> for InlineChannel<S, R> {
    fn send(&mut self, payload: S) {
        self.0.send(ThreadResponse::Response(0, payload));
    }

    fn receive(&mut self) -> Option<R> {
        while let Some(r) = self.0.receive() {
            match r {
                ThreadRequest::Request(i, r) => return Some(r),
                _ => {}
            }
        }
        None
    }
}


impl<
    S: Serialize + for<'a> Deserialize <'a> + 'static,
    R: Serialize + for<'a> Deserialize <'a> + 'static,
    Fut: Future<Output = ()> + Send + 'static,
    //Fut2: Future<Output = ()> + Send + 'static,
    F: FnOnce(ThreadContext, Box<dyn Channel<S, R>>) -> Fut + Send + 'static
> From<F> for Thread<S, R, Pin<Box<dyn Future<Output = ()> + Send>>> {
    fn from(f: F) -> Self {
        Thread(Box::new(|ctx: ThreadContext, channel: ThreadChannel<S, R> | {Box::pin(async move {
            let mut channel = InlineChannel(channel);
            f(ctx, Box::new(channel)).await
        })}), Some(Box::new(|_state: &mut State, _response: R| {})))
    }
}
