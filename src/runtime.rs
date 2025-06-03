use std::sync::mpsc::{channel, Sender, Receiver};
use std::collections::hash_map::DefaultHasher;
use std::time::{Instant, Duration};
use std::collections::BTreeMap;
use std::hash::{Hasher, Hash};
use std::future::Future;
use std::any::Any;
use std::any::TypeId;
use downcast_rs::{impl_downcast, Downcast};
use tokio::task::JoinHandle;
use serde::{Serialize, Deserialize};

const THREAD_TICK: u64 = 16;

pub use async_trait::async_trait;

use crate::State;
use crate::hardware;

pub type Callback = dyn Fn(&mut State, String);

pub trait Services {
    fn services() -> ServiceList {BTreeMap::new()}
}

pub type ServiceList = BTreeMap<TypeId, Box<dyn FnOnce(&mut hardware::Context) -> Box<dyn Future<Output = Box<dyn Service>> + Unpin>>>;

//Lives on the active thread, Services can talk to each other through the runtime ctx which lives
//on the active thread.
#[async_trait::async_trait]
pub trait Service: Downcast + Send + Sync + Any {
    async fn new(ctx: &mut hardware::Context) -> Self where Self: Sized;
    async fn run(&mut self, ctx: &mut ServiceContext, channel: &mut Channel) -> Duration;

    fn background_tasks(&self) -> Vec<Box<dyn BackgroundTask>> {vec![]}
    fn services(&self) -> ServiceList {BTreeMap::new()}
    fn callback(&self) -> Box<Callback> {Box::new(|_state: &mut State, _response: String| {})}
}
impl_downcast!(Service);

//Lives on the background thread
#[async_trait::async_trait]
pub trait BackgroundTask {
    async fn run(&mut self, ctx: &mut hardware::Context) -> Duration;
}

///Runtime Context enables communication between threads, cheap to clone and messages can be sent
///from anywhere
#[derive(Clone)]
pub struct Context {
    sender: Sender<(u64, String)>
}
impl Context {
    pub fn send<S: Service>(&mut self, payload: String) {
        self.sender.send((TypeId::of::<S>().get(), payload)).unwrap();
    }
}

pub struct Runtime {
    runtime: tokio::runtime::Runtime,
    receiver: Receiver<(u64, String)>,
    context: Context,
    channel: Channel,
    callbacks: BTreeMap<u64, Box<Callback>>,
    handles: Vec<JoinHandle<()>>
}

impl Runtime {
    pub async fn background(tasks: Vec<Box<dyn BackgroundTask>>, mut ctx: hardware::Context) {
        let mut tasks = tasks.into_iter().map(|t| (t, Instant::now(), Duration::ZERO)).collect::<Vec<_>>();
        loop {
            for (task, time, duration) in tasks.iter_mut() {
                if time.elapsed() > *duration {
                    *time = Instant::now();
                    *duration = task.run(&mut ctx).await;
                }
            }
            std::thread::sleep(Duration::from_secs(THREAD_TICK));
        }
    }

    pub fn start<S: Services>(mut hardware: hardware::Context) -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread().enable_time().enable_io().build().unwrap();
        let mut background = BTreeMap::new();
        let mut services = BTreeMap::new();
        let mut pre_serv = S::services();
        while let Some((id, service_gen)) = pre_serv.pop_first() {
            services.entry(id).or_insert_with(|| {
            //if !services.contains_key(&id) {
                let service = runtime.block_on(service_gen(&mut hardware));
                pre_serv.extend(service.services().into_iter());
                background.extend(service.background_tasks().into_iter().map(|s| ((*s).type_id(), s)));
                //services.insert(id, service);
                service
            });
        }
        let mut handles = Vec::new();
        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
        {
            if std::env::args().len() > 1 {
                runtime.block_on(Self::background(background.into_values().collect(), hardware));
                panic!("Background tasks shutdown");
            }
        }
        #[cfg(any(target_os = "ios", target_os = "android"))]
        {
            handles.push(runtime.spawn(Self::background(background.into_values().collect(), hardware.clone())));
        }

        let (sender, receiver) = channel();
        let (channel, b) = Channel::new();
        let callbacks = services.iter().map(|(k, s)| (k.get(), s.callback())).collect();
        let context = ServiceContext{hardware, services};
        handles.push(runtime.spawn(ActiveThread::new(context, b).run()));
        Runtime{
            runtime,
            receiver,
            context: Context{sender},
            channel,
            callbacks,
            handles
        }
    }

    pub fn context(&self) -> &Context {&self.context}

    ///Reads any requests from the context and passes them onto the tasks
    pub fn tick(&mut self, state: &mut State) {
        while let Ok((id, payload)) = self.receiver.try_recv() {
            if self.callbacks.contains_key(&id) {
                self.channel.send(serde_json::to_string(&Request::Request(id, payload)).unwrap());
            }
        }

        while let Some(recv) = self.channel.receive() {
            let (id, payload) = serde_json::from_str::<(u64, String)>(&recv).unwrap();
            self.callbacks.get_mut(&id).unwrap()(state, payload)
        }

        for handle in &mut self.handles {
            if handle.is_finished() {
                self.runtime.block_on(handle).unwrap()
            }
        }
    }

    ///Blocks on non wasm on wasm local spawned threads block until completed
    ///In either case has to be treated as a seperate thread
    pub fn block_on(&self, future: impl Future<Output = ()>) {
        self.runtime.block_on(future);
    }

    pub fn pause(&mut self) {self.channel.send(serde_json::to_string(&Request::Lifetime(true)).unwrap());}
    pub fn resume(&mut self) {self.channel.send(serde_json::to_string(&Request::Lifetime(false)).unwrap());}
    pub fn close(self) {self.runtime.shutdown_background()}
}

pub struct ServiceContext {
    pub hardware: hardware::Context,
    services: BTreeMap<TypeId, Box<dyn Service>>,
}
impl ServiceContext {
    pub fn service<S: Service>(&mut self) -> &mut S {self.services.get_mut(&TypeId::of::<S>()).unwrap().downcast_mut().unwrap()}
}

struct ActiveThread {
    context: ServiceContext,
    channel: Channel,
    channels: BTreeMap<u64, Channel>,
    handles: BTreeMap<TypeId, (Channel, Instant, Duration)> 
}

impl ActiveThread {
    pub fn new(context: ServiceContext, channel: Channel) -> Self {
        let (channels, handles): (BTreeMap<_,_>, BTreeMap<_,_>) = context.services.keys().map(|k| {
            let (a, b) = Channel::new();
            ((k.get(), a), (*k, (b, Instant::now(), Duration::ZERO)))
        }).unzip();
        ActiveThread{context, channel, channels, handles}
    }

    pub async fn run(mut self) {
        let mut paused = false;
        loop {
            while let Some(request) = self.channel.receive() {
                match serde_json::from_str::<Request>(&request).unwrap() {
                    Request::Request(id, payload) => self.channels.get_mut(&id).unwrap().send(payload),
                    Request::Lifetime(p) => paused = p
                }
            }

            if !paused {
                for (id, (channel, time, duration)) in &mut self.handles {
                    if time.elapsed() > *duration {
                        *time = Instant::now();
                        let mut service = self.context.services.remove(id).unwrap();
                        *duration = service.run(&mut self.context, channel).await;
                        self.context.services.insert(*id, service);
                    }
                }

                for (id, channel) in &mut self.channels {
                    while let Some(payload) = channel.receive() {
                        self.channel.send(serde_json::to_string(&(id, payload)).unwrap());
                    }
                }
                //TODO: sleep for min duration till next task or THREAD_TICK if paused
            }

            std::thread::sleep(Duration::from_millis(THREAD_TICK));
        }
    }
}

pub struct Channel(Sender<String>, Receiver<String>);
impl Channel {
    fn new() -> (Self, Self) {
        let (a, b) = channel();
        let (c, d) = channel();
        (Channel(a, d), Channel(c, b))
    }

    pub fn send(&mut self, payload: String) {
        self.0.send(payload).unwrap();
    }

    pub fn receive(&mut self) -> Option<String> {
        self.1.try_recv().ok()
    }
}

#[derive(Serialize, Deserialize)]
enum Request {
    Request(u64, String),
    Lifetime(bool)
}

trait TypeIdId {
    fn get(self) -> u64;
}
impl TypeIdId for TypeId {
    fn get(self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish()
    }
}
