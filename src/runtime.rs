use std::sync::mpsc::{channel, Sender, Receiver};
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::future::Future;

pub use async_trait::async_trait;
pub use tokio::time::Duration;
use tokio::task::JoinHandle;
use serde::{Serialize, Deserialize};

use crate::State;
use crate::hardware;

pub mod thread;
pub use thread::{Thread, ThreadRequest, ThreadResponse, ThreadChannelR, Task};

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

    fn send(&mut self, payload: S) {
        let _ = self.0.send(serde_json::to_string(&payload).unwrap());
    }

    fn receive(&mut self) -> Option<R> {
        self.1.try_recv().ok().map(|r| serde_json::from_str(&r).unwrap())
    }
}


pub type Callback<S> = Box<dyn FnMut(&mut State, S)>;

#[derive(Serialize, Deserialize, Clone)]
pub struct Error(String, String);
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {write!(f, "{}", self.0)}
}
impl std::fmt::Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {write!(f, "{}", self.1)}
}
impl<E: std::error::Error> From<E> for Error {
    fn from(error: E) -> Error {Error(error.to_string(), format!("{:?}", error))}
}

//  //Lives on the background thread
//  #[async_trait::async_trait]
//  pub trait BackgroundTask {
//      async fn run(&mut self, ctx: &mut hardware::Context) -> Result<Duration, Error>;
//  }

pub type Id = u64;

pub enum RuntimeRequest {
    Request(Id, String),
    Spawn(Id, Box<dyn Thread>, Callback<String>)
}

pub struct Handle<S>(Context, Id, PhantomData<S>);
impl<S: Serialize> Handle<S> {
    pub fn send(&self, payload: S) {
        self.0.send(self.1, serde_json::to_string(&payload).unwrap());
    }
}

///Runtime Context enables communication between threads, cheap to clone and messages can be sent
///from anywhere
#[derive(Clone)]
pub struct Context {
    sender: Sender<RuntimeRequest>
}
impl Context {
    fn send(&self, id: Id, payload: String) {
        self.sender.send(RuntimeRequest::Request(id, payload)).unwrap();
    }

    pub fn spawn<
        S: Serialize + for<'a> Deserialize <'a> + Send + 'static,
        R: Serialize + for<'a> Deserialize <'a> + Send + 'static,
        X: 'static,
        T: Task<S, R, X> + 'static
    >(&self, task: T) -> Handle<R> {
        let (thread, mut callback) = task.get();
        let id = thread.id();
        self.sender.send(RuntimeRequest::Spawn(
            id,
            thread, 
            Box::new(move |state: &mut State, r: String| {
                callback(state, serde_json::from_str(&r).unwrap())
            })
        )).unwrap();
        Handle(self.clone(), id, PhantomData::<R>)
    }
}

pub struct Runtime {
    hardware: hardware::Context,
    context: Context,
    receiver: Receiver<RuntimeRequest>,
    runtime: tokio::runtime::Runtime,
    threads: HashMap<Id, (ThreadChannelR, Callback<String>, JoinHandle<()>)>,
}

impl Runtime {
    pub fn start(hardware: hardware::Context) -> Self {
        let (sender, receiver) = channel();
        let context = Context{sender};
        let runtime = tokio::runtime::Builder::new_multi_thread().enable_time().enable_io().build().unwrap();
        Runtime{
            hardware,
            context,
            receiver,
            runtime,
            threads: HashMap::new(),
        }
    }

    pub fn context(&self) -> &Context {&self.context}

    pub fn tick(&mut self, state: &mut State) {
        let mut requests = Vec::new();
        while let Ok(request) = self.receiver.try_recv() {
            match request {
                RuntimeRequest::Spawn(id, thread, callback) => {self.spawn(id, thread, callback);},
                RuntimeRequest::Request(id, payload) => {
                    requests.push((id, payload));
                }
            }
        }

        for (id, payload) in requests {
            if let Some(thread) = self.threads.get_mut(&id) {
                thread.0.send(ThreadRequest::Request(0, payload));
            }
        }

        self.threads = self.threads.drain().filter_map(|(id, mut thread)| {
            while let Some(recv) = thread.0.receive() {match recv {
                ThreadResponse::Response(0, r) => (thread.1)(state, r),
                _ => todo!()
            }}
            match thread.2.is_finished() {
                true => {self.runtime.block_on(thread.2).unwrap(); None},
                false => Some((id, thread))
            }
        }).collect();
    }

    fn spawn(&mut self, id: Id, thread: Box<dyn Thread>, callback: Callback<String>) -> bool {
        if let Entry::Vacant(e) = self.threads.entry(id) {
            let (a, b) = Channel::new();
            let handle = self.runtime.spawn(thread.run(self.hardware.clone(), b));
            e.insert((a, callback, handle));
            true
        } else {false}
    }

    ///Blocks on non wasm on wasm local spawned threads block until completed
    ///In either case has to be treated as a seperate thread
    pub fn block_on(&self, future: impl Future<Output = ()>) {
        self.runtime.block_on(future);
    }

    pub fn pause(&mut self) {
        self.threads.values_mut().for_each(|t| t.0.send(ThreadRequest::Pause));
    }
    pub fn resume(&mut self) {
        self.threads.values_mut().for_each(|t| t.0.send(ThreadRequest::Resume));
    }   
    pub fn close(self) {
      //self.runtime.block_on(async {
      //    self.threads.values_mut().for_each(|t| t.0.send(ThreadRequest::Close));
      //    for thread in self.threads.into_values() {
      //        thread.2.await.unwrap()
      //    }
      //});
        self.runtime.shutdown_background();
    }
}
