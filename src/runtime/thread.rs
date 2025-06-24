use std::sync::mpsc::{channel, Sender, Receiver};
use std::collections::{BTreeMap, VecDeque};
use std::marker::PhantomData;
use std::future::Future;
use std::pin::Pin;

use serde::{Serialize, Deserialize};

use crate::{hardware, State, Id};
use super::Error;

pub mod service;
pub mod tasks;

pub type ThreadChannel = Channel<ThreadResponse, ThreadRequest>;

pub type Constructor = Box<dyn for<'a> Fn(&'a mut hardware::Context) -> Pin<Box<dyn Future<Output = (Box<dyn Thread>, Callback<String>)> + 'a>>>;
pub type Callback<S> = Box<dyn FnMut(&mut State, S)>;

#[async_trait::async_trait]
pub trait Thread: Send {
    type Send: Serialize + for<'a> Deserialize <'a> + Send where Self: Sized;
    type Receive: Serialize + for<'a> Deserialize <'a> + Send where Self: Sized;

    async fn run(self: Box<Self>, ctx: hardware::Context, channel: ThreadChannel);

    fn type_id() -> Option<Id> where Self: Sized;
    fn id(&self) -> Id;
}

pub trait IntoThread<S, R, X> {
    fn into(self) -> (Box<dyn Thread>, Callback<String>);
}

impl IntoThread<(), (), ()> for (Box<dyn Thread>, Callback<String>) {
    fn into(self) -> (Box<dyn Thread>, Callback<String>) {self}
}

trait StringifyCallback {
    fn stringify(self) -> Callback<String>;
}
impl<S: Serialize + for<'a> Deserialize <'a> + Send + 'static> StringifyCallback for Callback<S> {
    fn stringify(mut self) -> Callback<String> {
        Box::new(move |state: &mut State, r: String| {
            (self)(state, serde_json::from_str(&r).unwrap())
        })
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub enum ThreadRequest {
    Response(Id, String),
    Request(Id, String),
    Resume,
    Pause,
}

#[derive(Serialize, Deserialize)]
pub enum ThreadResponse {
    Request(Id, Id, String),
    Response(Id, String),
    Error(Error),
}

pub struct Channel<S, R>(Sender<String>, Receiver<String>, PhantomData<fn() -> S>, PhantomData<fn() -> R>);
impl< 
    S: Serialize + for<'a> Deserialize <'a>,
    R: Serialize + for<'a> Deserialize <'a>,
> Channel<S, R> {
    pub fn new() -> (Self, Channel<R, S>) {
        let (a, b) = channel();
        let (c, d) = channel();
        (Channel(a, d, PhantomData::<fn() -> S>, PhantomData::<fn() -> R>), Channel(c, b, PhantomData::<fn() -> R>, PhantomData::<fn() -> S>))
    }

    pub fn send(&mut self, payload: S) {
        let _ = self.0.send(serde_json::to_string(&payload).unwrap());
    }

    pub fn try_receive(&mut self) -> Option<R> {
        self.1.try_recv().ok().map(|r| serde_json::from_str(&r).unwrap())
    }

    pub async fn receive(&mut self) -> R {
        loop {
            if let Some(r) = self.try_receive() {
                break r;
            }
            tokio::time::sleep(tokio::time::Duration::ZERO).await
        }
    }
}

pub struct RequestHandle<T>(Id, PhantomData<fn() -> T>);

pub struct Context<S, R> {
    pub hardware: hardware::Context,
    channel: ThreadChannel,
    receive: VecDeque<(Id, R)>,
    received: BTreeMap<Id, String>,
    paused: bool,
    _p: PhantomData<fn() -> S>,
}
impl<
    S: Serialize + for<'a> Deserialize <'a> + Send + 'static,
    R: Serialize + for<'a> Deserialize <'a> + Send + 'static,
> Context<S, R> {
    pub fn new(hardware: hardware::Context, channel: ThreadChannel) -> Self {
        Context{hardware, channel, receive: VecDeque::new(), received: BTreeMap::new(), paused: false, _p: PhantomData::<fn() -> S>}
    }
    pub async fn blocking_request<T: Thread>(&mut self, request: T::Receive) -> T::Send {
        let req_id = Id::random();
        println!("req: {:?}", req_id);
        self.channel.send(ThreadResponse::Request(req_id, T::type_id().expect("Cannot send messages to this thread"), serde_json::to_string(&request).unwrap()));
        loop {
            let res = self.channel.receive().await;
            println!("check");
            self.handle(res);
            if let Some(result) = self.received.remove(&req_id) {
                break serde_json::from_str(&result).unwrap();
            }
        }
    }

    pub fn request<T: Thread>(&mut self, request: T::Receive) -> RequestHandle<T::Send> {
        let req_id = Id::random();
        self.channel.send(ThreadResponse::Request(req_id, T::type_id().expect("Cannot send messages to this thread"), serde_json::to_string(&request).unwrap()));
        RequestHandle(req_id, PhantomData::<fn() -> T::Send>)
    }

    pub fn check_request<T: for<'a> Deserialize<'a>>(&mut self, request: &RequestHandle<T>) -> Option<T> {
        self.received.remove(&request.0).and_then(|r| serde_json::from_str(&r).unwrap())
    }

    pub fn get_request(&mut self) -> Option<(Id, R)> {
        self.receive.pop_back()
    }

    pub fn get_requests(&mut self) -> Vec<(Id, R)> {
        self.receive.drain(..).collect()
    }

    pub fn respond(&mut self, id: Id, payload: S) {
        self.channel.send(ThreadResponse::Response(id, serde_json::to_string(&payload).unwrap()));  
    }

    pub fn callback(&mut self, payload: S) {
        self.channel.send(ThreadResponse::Response(Id::MIN, serde_json::to_string(&payload).unwrap()));  
    }

    fn check_received(&mut self) {
        while let Some(request) = self.channel.try_receive() {
            self.handle(request);
        }
    }

    fn handle(&mut self, request: ThreadRequest) {
        match request {
            ThreadRequest::Response(id, payload) => {self.received.insert(id, payload);},
            ThreadRequest::Request(id, payload) => self.receive.push_front((id, serde_json::from_str(&payload).unwrap())),
            ThreadRequest::Pause => self.paused = true,
            ThreadRequest::Resume => self.paused = false,
        }
    }
}
