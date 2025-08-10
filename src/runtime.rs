use std::sync::mpsc::{channel, Sender, Receiver};
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::future::Future;

pub use async_trait::async_trait;
pub use tokio::time::Duration;
use tokio::task::JoinHandle;
use serde::{Serialize, Deserialize};

use crate::{hardware, State, Id};

mod thread;
use thread::{Thread, ThreadRequest, ThreadResponse, Channel, IntoThread, Callback};
pub use thread::{Context as ThreadContext, Constructor as ThreadConstructor};
pub use thread::service::{Service, ServiceList, BackgroundTask, BackgroundList, Services}; 

type ThreadChannel = Channel<ThreadRequest, ThreadResponse>;

#[derive(Serialize, Deserialize, Clone)]
pub struct Error(String, String);
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {write!(f, "{}", self.0)}
}
impl std::fmt::Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {write!(f, "{}", self.1)}
}
impl<E: std::error::Error> From<E> for Error {
    fn from(error: E) -> Error {Error(error.to_string(), format!("{error:?}"))}
}

pub enum RuntimeRequest {
    Request(Id, String),
    Spawn(Box<dyn Thread>, Callback<String>)
}

pub struct Handle<R>(Context, Id, PhantomData<R>);
impl<R: Serialize> Handle<R> {
    pub fn send(&self, payload: &R) {
        self.0.sender.send(RuntimeRequest::Request(self.1, serde_json::to_string(payload).unwrap())).unwrap();
    }
}

///Runtime Context enables communication between threads, cheap to clone and messages can be sent
///from anywhere
#[derive(Clone)]
pub struct Context {
    sender: Sender<RuntimeRequest>
}
impl Context {
    pub fn send<
        T: Thread + 'static,
    >(&self, payload: &T::Receive) {
        self.sender.send(RuntimeRequest::Request(T::type_id().expect("Can not send messages to this thread"), serde_json::to_string(payload).unwrap())).unwrap();
    }

    pub fn spawn<S, R, X: 'static, T: IntoThread<S, R, X> + 'static>(&self, task: T) -> Handle<R> {
        let (thread, callback) = task.into();
        let id = thread.id();
        self.sender.send(RuntimeRequest::Spawn(
            thread, callback
        )).unwrap();
        Handle(self.clone(), id, PhantomData::<R>)
    }
}

pub struct Runtime {
    hardware: hardware::Context,
    context: Context,
    receiver: Receiver<RuntimeRequest>,
    runtime: tokio::runtime::Runtime,
    threads: HashMap<Id, (ThreadChannel, Callback<String>, JoinHandle<()>)>,
    requests: Vec<(Id, Id)>
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
            requests: Vec::new()
        }
    }

    pub fn background(mut self, hardware: &mut hardware::Context, tasks: Vec<ThreadConstructor>) {
        let mut threads = Vec::new();
        self.runtime.block_on(async {
            for bt in tasks {
                threads.push(bt(hardware).await);
            }
        });
        for (thread, callback) in threads {
            self.spawn(thread, callback);
        }
        self.runtime.block_on(async {
            for thread in self.threads.into_values() {
                thread.2.await.unwrap()
            }
        });
    }

    pub fn context(&self) -> &Context {&self.context}

    pub fn tick(&mut self, state: &mut State) -> Result<(), Error> {
        let mut requests = Vec::new();
        while let Ok(request) = self.receiver.try_recv() {
            match request {
                RuntimeRequest::Spawn(thread, callback) => {self.spawn(thread, callback);},
                RuntimeRequest::Request(id, payload) => {
                    requests.push((id, payload));
                }
            }
        }

        for (id, payload) in requests {
            if let Some(thread) = self.threads.get_mut(&id) {
                thread.0.send(ThreadRequest::Request(Id::MIN, payload));
            }
        }

        let keys = self.threads.keys().copied().collect::<Vec<Id>>();
        for id in keys {
            let mut thread = self.threads.remove(&id).unwrap();
            while let Some(recv) = thread.0.try_receive() {match recv {
                ThreadResponse::Response(Id::MIN, r) => (thread.1)(state, r),
                ThreadResponse::Response(id, r) => {
                    let task_id = self.requests.iter().find_map(|(i, ti)| (*i == id).then_some(ti)).expect("Responded to missing request");
                    if let Some(thread) = self.threads.get_mut(task_id) {
                        thread.0.send(ThreadRequest::Response(id, r));
                    } else {panic!("Responded to missing thread")}
                },
                ThreadResponse::Error(e) => return Err(e),
                ThreadResponse::Request(req_id, task_id, payload) => {
                    if let Some(thread) = self.threads.get_mut(&task_id) {
                        thread.0.send(ThreadRequest::Request(req_id, payload));
                        self.requests.push((req_id, id))
                    } else {panic!("Requested to missing thread");}
                },
            }}
            match thread.2.is_finished() {
                true => {self.runtime.block_on(thread.2)?;},
                false => {self.threads.insert(id, thread);},
            }
        }

        Ok(())
    }

    fn spawn(&mut self, thread: Box<dyn Thread>, callback: Callback<String>) -> bool {
        let id = thread.id();
        if let Entry::Vacant(e) = self.threads.entry(id) {
            let (a, b) = ThreadChannel::new();
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
        self.runtime.shutdown_background();
    }
}
