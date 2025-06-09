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
pub use tokio::time::Duration;
use tokio::task::JoinHandle;
use serde::{Serialize, Deserialize};
use rand::Rng;

const THREAD_TICK: u64 = 16;

pub use async_trait::async_trait;

use crate::State;
use crate::hardware;

mod channel;
pub use channel::{Channel, SerdeChannel};

pub mod thread;
pub use thread::{_Thread, Thread, ThreadRequest, ThreadResponse, ThreadChannelR, Task};

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
    Spawn(Box<dyn _Thread>, Callback<String>)
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
        //Fut: Future<Output = Result<Option<Duration>, Error>> + Send,
        T: Task<S, R, Pin<Box<dyn Future<Output = Result<Option<Duration>, Error>> + Send>>> + 'static
    >(&self, task: T) -> Handle<S> {
        let id = task.id();
        let (task, mut callback) = task.get();
        self.sender.send(RuntimeRequest::Spawn(
            Box::new(Thread(Box::new(task))), 
            Box::new(move |state: &mut State, r: String| {
                callback(state, serde_json::from_str(&r).unwrap())
            })
        )).unwrap();
        Handle(self.clone(), id, PhantomData::<S>)
    }


  //pub fn spawn<
  //    S: Serialize + for<'a> Deserialize <'a> + 'static,
  //    R: Serialize + for<'a> Deserialize <'a> + 'static,
  //    Fut: Future<Output = Result<Option<Duration>, Error>> + Send + 'static,
  //    T: Into<(Thread<S, R, Fut>, Box<Callback<S>>)> + 'static
  //>(&self, thread: T) -> Handle<S> {
  //    let (thread, mut callback) = thread.into();
  //    let id = thread.id();
  //    self.sender.send(RuntimeRequest::Spawn(
  //        Box::new(thread), 
  //        Box::new(move |state: &mut State, r: String| {
  //            callback(state, serde_json::from_str(&r).unwrap())
  //        })
  //    )).unwrap();
  //    Handle(self.clone(), id, PhantomData::<S>)
  //}
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
        while let Ok(request) = self.receiver.try_recv() {
            match request {
                RuntimeRequest::Spawn(thread, callback) => {self.spawn(thread, callback);},
                RuntimeRequest::Request(id, payload) => {
                    if let Some(thread) = self.threads.get_mut(&id) {
                        thread.0.send(ThreadRequest::Request(0, payload));
                    }
                }
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

    fn spawn(&mut self, thread: Box<dyn _Thread>, callback: Callback<String>) -> bool {
        let id = thread.id();
        if let Entry::Vacant(e) = self.threads.entry(id) {
            let (a, b) = SerdeChannel::new();
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
    pub fn close(mut self) {
      //self.runtime.block_on(async {
      //    self.threads.values_mut().for_each(|t| t.0.send(ThreadRequest::Close));
      //    for thread in self.threads.into_values() {
      //        thread.2.await.unwrap()
      //    }
      //});
        self.runtime.shutdown_background();
    }
}



//  trait TypeIdId {
//      fn get(self) -> u64;
//  }
//  impl TypeIdId for TypeId {
//      fn get(self) -> u64 {
//          let mut hasher = DefaultHasher::new();
//          self.hash(&mut hasher);
//          hasher.finish()
//      }
//  }
//
//  trait AsyncFnOnceSend<I, O>: FnOnce(ThreadContext, I) -> Self::Fut + Send + 'static {
//      type Fut: Future<Output = O> + Send;
//  }

//  impl<
//      I, Fut: Future<Output = ()> + Send,
//      F: FnOnce(ThreadContext, I) -> Fut + Send + 'static
//  > AsyncFnOnceSend<I, ()> for F {
//      type Fut = Fut;
//  }
//
////  #[async_trait]
//  impl<F: FnOnce(ThreadContext) -> Fut + Send + 'static, Fut: Future<Output = ()> + Send, C: FnMut(&mut State, String) + Clone + Send + 'static> Thread for (F, C) {
//      async fn run(self: Box<Self>, ctx: ThreadContext) {
//          (self.0)(ctx).await
//      }

//      fn callback(&self) -> Box<Callback> {Box::new(self.1.clone())}
//  }



//  pub struct ServiceContext {
//      pub hardware: hardware::Context,
//      services: BTreeMap<TypeId, Box<dyn Service>>,
//  }
//  impl ServiceContext {
//      pub fn get<S: Service>(&mut self) -> &mut S {self.services.get_mut(&TypeId::of::<S>()).expect("Service Not Found").downcast_mut().unwrap()}
//  }

//  struct ActiveThread {
//      context: ServiceContext,
//      channel: Channel,
//      channels: BTreeMap<u64, Channel>,
//      handles: BTreeMap<TypeId, (Channel, Instant, Duration)> 
//  }

//  impl ActiveThread {
//      pub fn new(context: ServiceContext, channel: Channel) -> Self {
//          let (channels, handles): (BTreeMap<_,_>, BTreeMap<_,_>) = context.services.keys().map(|k| {
//              let (a, b) = Channel::new();
//              ((k.get(), a), (*k, (b, Instant::now(), Duration::ZERO)))
//          }).unzip();
//          ActiveThread{context, channel, channels, handles}
//      }

//      pub async fn run(mut self) {
//          let mut paused = false;
//          loop {
//              while let Some(request) = self.channel.receive() {
//                  match serde_json::from_str::<Request>(&request).unwrap() {
//                      Request::Request(id, payload) => {
//                          self.channels.get_mut(&id).unwrap().send(payload)
//                      },
//                      Request::Lifetime(p) => paused = p
//                  }
//              }

//              if !paused {
//                  for (id, (channel, time, duration)) in &mut self.handles {
//                      if time.elapsed() > *duration {
//                          *time = Instant::now();
//                          let mut service = self.context.services.remove(id).unwrap();
//                          match service.run(&mut self.context, channel).await {
//                              Ok(d) => {*duration = d;},
//                              Err(e) => log::error!("Service {} Error:\n{},\n{:?}", std::any::type_name_of_val(&*service), e, e)
//                          }
//                          self.context.services.insert(*id, service);
//                      }
//                  }

//                  for (id, channel) in &mut self.channels {
//                      while let Some(payload) = channel.receive() {
//                          self.channel.send(serde_json::to_string(&(id, payload)).unwrap());
//                      }
//                  }
//                  //TODO: sleep for min duration till next task or THREAD_TICK if paused
//              }

//              std::thread::sleep(Duration::from_millis(THREAD_TICK));
//          }
//      }
//  }


