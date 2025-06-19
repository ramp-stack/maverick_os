use std::collections::{BTreeMap, VecDeque};
use std::hash::{DefaultHasher, Hasher, Hash};
use std::marker::PhantomData;
use std::future::Future;
use std::any::TypeId;
use std::pin::Pin;

use tokio::time::{Instant, Duration};
use serde::{Serialize, Deserialize};
use rand::Rng;

use crate::{State, hardware};
use super::{StringifyCallback, Callback, Id, Error, Channel, Services, BackgroundList};

pub type ThreadChannel = Channel<ThreadResponse, ThreadRequest>;
pub type ThreadChannelR = Channel<ThreadRequest, ThreadResponse>;

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

#[async_trait::async_trait]
pub trait Thread: Send {
    type Send: Serialize + for<'a> Deserialize <'a> + Send where Self: Sized;
    type Receive: Serialize + for<'a> Deserialize <'a> + Send where Self: Sized;

    async fn run(self: Box<Self>, ctx: hardware::Context, channel: ThreadChannel);

    fn type_id() -> Option<Id> where Self: Sized;
    fn id(&self) -> Id;
}

pub trait Task<S, R, X> {
    fn get(self) -> (Box<dyn Thread>, Callback<String>);
}

impl Task<(), (), ()> for (Box<dyn Thread>, Callback<String>) {
    fn get(self) -> (Box<dyn Thread>, Callback<String>) {self}
}

trait AsyncFnMutSend<I>: FnMut(I) -> Self::Fut {
    type Fut: Future<Output = <Self as AsyncFnMutSend::<I>>::Out> + Send;
    type Out;
}
impl<I, F: FnMut(I) -> Fut + Send, Fut: Future + Send> AsyncFnMutSend<I> for F {
    type Fut = Fut;
    type Out = Fut::Output;
}

//SERVICE THREAD

type Res = Result<Option<Duration>, Error>;
type TaskTick<S, R> = Box<dyn for<'b> FnMut(&'b mut Context<S, R>) -> Pin<Box<dyn Future<Output = Res> + Send + 'b>> + Send>;

struct TickingTask<S, R>(Id, TaskTick<S, R>);

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
        let req_id = rand::rng().random();
        self.channel.send(ThreadResponse::Request(req_id, T::type_id().expect("Cannot send messages to this thread"), serde_json::to_string(&request).unwrap()));
        loop {
            self.check_received();
            if let Some(result) = self.received.remove(&req_id) {
                break serde_json::from_str(&result).unwrap();
            }
        }
    }

    pub fn request<T: Thread>(&mut self, request: T::Receive) -> RequestHandle<T::Send> {
        let req_id = rand::rng().random();
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
        self.channel.send(ThreadResponse::Response(0, serde_json::to_string(&payload).unwrap()));  
    }

    fn check_received(&mut self) {
        while let Some(request) = self.channel.receive() {match request {
            ThreadRequest::Response(id, payload) => {self.received.insert(id, payload);},
            ThreadRequest::Request(id, payload) => self.receive.push_front((id, serde_json::from_str(&payload).unwrap())),
            ThreadRequest::Pause => self.paused = true,
            ThreadRequest::Resume => self.paused = false,
        }}
    }
}

#[async_trait::async_trait]
impl<
    S: Serialize + for<'a> Deserialize <'a> + Send + 'static,
    R: Serialize + for<'a> Deserialize <'a> + Send + 'static,
> Thread for TickingTask<S, R> {
    type Send = S;
    type Receive = R;

    async fn run(mut self: Box<Self>, hardware: hardware::Context, channel: ThreadChannel) {
        let mut ctx = Context::new(hardware, channel);
        let mut error_count = 0;
        let mut last_run = Instant::now();
        let mut duration = Duration::ZERO;
        loop {
            ctx.check_received();
            if !ctx.paused {
                let elapsed = last_run.elapsed();
                if elapsed > duration {
                    last_run = Instant::now();
                    let result = (self.1)(&mut ctx).await;
                    match result {
                        Ok(None) => return,
                        Ok(Some(dur)) => duration = dur,
                        Err(e) if error_count < 2 => {
                            error_count += 1; 
                            log::error!("Thread {}, Errored {} :? {:?}", self.id(), e, e)
                        },
                        Err(e) => {
                            ctx.channel.send(ThreadResponse::Error(e));
                            break;
                        }
                    }
                } else {
                    tokio::time::sleep(duration - elapsed).await
                }
            }
        }
    }

    fn type_id() -> Option<Id> {None}

    fn id(&self) -> Id {self.0}
}

impl<
    S: Serialize + for<'a> Deserialize <'a> + Send + 'static,
    R: Serialize + for<'a> Deserialize <'a> + Send + 'static,
    F: for<'b> AsyncFnMutSend<&'b mut Context<S, R>, Out = Res> + Send + 'static
> Task<S, R, TaskTick<S, R>> for F {
    fn get(mut self) -> (Box<dyn Thread>, Callback<String>){
        let task: TaskTick<S, R> = Box::new(move |ctx: &mut Context<S, R>| Box::pin(self(ctx)));
        (Box::new(TickingTask(rand::rng().random(), task)), Box::new(|_: &mut State, _: String| {}))
    }
}

impl<
    S: Serialize + for<'a> Deserialize <'a> + Send + 'static,
    R: Serialize + for<'a> Deserialize <'a> + Send + 'static,
    F: for<'b> AsyncFnMutSend<&'b mut Context<S, R>, Out = Res> + Send + 'static,
    CF: FnMut(&mut State, S) + StringifyCallback + 'static
> Task<S, R, TaskTick<S, R>> for (F, CF) {
    fn get(mut self) -> (Box<dyn Thread>, Callback<String>){
        let task: TaskTick<S, R> = Box::new(move |ctx: &mut Context<S, R>| Box::pin((self.0)(ctx)));
        (Box::new(TickingTask(rand::rng().random(), task)), Box::new(self.1.stringify()))
    }
}

//SERVICE

#[async_trait::async_trait]
pub trait Service: Services + Send {
    type Send: Serialize + for<'a> Deserialize <'a> + Send + 'static;
    type Receive: Serialize + for<'a> Deserialize <'a> + Send + 'static;

    async fn new(ctx: &mut hardware::Context) -> Self where Self: Sized;

    async fn run(&mut self, ctx: &mut Context<Self::Send, Self::Receive>) -> Result<Option<Duration>, Error>;

    fn callback(_state: &mut State, _payload: Self::Send) where Self: Sized {}

    fn background_tasks() -> BackgroundList where Self: Sized {BackgroundList::default()}
}

impl<
    SE: Service + 'static
> Task<SE::Send, SE::Receive, u32> for SE {
    fn get(self) -> (Box<dyn Thread>, Callback<String>){
        (Box::new(self), (Box::new(SE::callback) as Callback<SE::Send>).stringify())
    }
}

#[async_trait::async_trait]
impl<
    SE: Service + 'static
> Thread for SE {
    type Send = SE::Send;
    type Receive = SE::Receive;

    async fn run(mut self: Box<Self>, hardware: hardware::Context, channel: ThreadChannel) {
        let mut ctx = Context::new(hardware, channel);
        let mut error_count = 0;
        let mut last_run = Instant::now();
        let mut duration = Duration::ZERO;
        loop {
            ctx.check_received();
            if !ctx.paused {
                let elapsed = last_run.elapsed();
                if elapsed > duration {
                    last_run = Instant::now();
                    let result = SE::run(&mut self, &mut ctx).await;
                    match result {
                        Ok(None) => return,
                        Ok(Some(dur)) => duration = dur,
                        Err(e) if error_count < 2 => {
                            error_count += 1; 
                            log::error!("Thread {}, Errored {} :? {:?}", self.id(), e, e)
                        },
                        Err(e) => {
                            ctx.channel.send(ThreadResponse::Error(e));
                            break;
                        }
                    }
                } else {
                    tokio::time::sleep(duration - elapsed).await
                }
            }
        }
    }

    fn type_id() -> Option<Id> {
        let mut hasher = DefaultHasher::default();
        TypeId::of::<SE>().hash(&mut hasher);
        Some(hasher.finish())
    }

    fn id(&self) -> Id {Self::type_id().unwrap()}
}

//BACKGROUND TASK
#[async_trait::async_trait]
pub trait BackgroundTask: Send {
    async fn new(ctx: &mut hardware::Context) -> Self where Self: Sized;

    async fn run(&mut self, ctx: &mut hardware::Context) -> Result<Option<Duration>, Error>;
}

impl<
    BT: BackgroundTask + 'static
> Task<(), (), i32> for BT {
    fn get(self) -> (Box<dyn Thread>, Callback<String>){
        (Box::new(_BackgroundTask(self)), Box::new(|_: &mut State, _: String| {}))
    }
}

struct _BackgroundTask<BT: BackgroundTask>(BT);

#[async_trait::async_trait]
impl<
    BT: BackgroundTask + 'static
> Thread for _BackgroundTask<BT> {
    type Send = ();
    type Receive = ();

    async fn run(mut self: Box<Self>, mut hardware: hardware::Context, _channel: ThreadChannel) {
        let mut last_run = Instant::now();
        let mut duration = Duration::ZERO;
        loop {
            let elapsed = last_run.elapsed();
            if elapsed > duration {
                last_run = Instant::now();
                let result = self.0.run(&mut hardware).await;
                match result {
                    Ok(None) => return,
                    Ok(Some(dur)) => duration = dur,
                    Err(e) => log::error!("Thread {}, Errored {} :? {:?}", self.id(), e, e),
                }
            }
        }
    }

    fn type_id() -> Option<Id> {
        let mut hasher = DefaultHasher::default();
        TypeId::of::<BT>().hash(&mut hasher);
        Some(hasher.finish())
    }

    fn id(&self) -> Id {Self::type_id().unwrap()}
}

//ONESHOT

type TaskOneshot<S> = Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = S> + Send>> + Send>;

#[async_trait::async_trait]
impl<
    S: Serialize + for<'a> Deserialize <'a> + Send + 'static,
> Thread for (Id, TaskOneshot<S>) {
    type Send = S;
    type Receive = ();

    async fn run(mut self: Box<Self>, _hardware: hardware::Context, mut channel: ThreadChannel) {
        let s = (self.1)().await;
        channel.send(ThreadResponse::Response(0, serde_json::to_string(&s).unwrap()));
    }

    fn type_id() -> Option<Id> {None}

    fn id(&self) -> Id {self.0}
}

impl<
    S: Serialize + for<'a> Deserialize <'a> + Send + 'static,

    Fut: Future<Output = S> + Send + 'static,
    F: FnOnce() -> Fut + Send + 'static,
> Task<S, (), TaskOneshot<S>> for F {
    fn get(self) -> (Box<dyn Thread>, Callback<String>){
        let task: TaskOneshot<S> = Box::new(move || Box::pin(self()));
        (Box::new((rand::rng().random(), task)), Box::new(|_: &mut State, _: String| {}))
    }
}

impl<
    S: Serialize + for<'a> Deserialize <'a> + Send + 'static,

    Fut: Future<Output = S> + Send + 'static,
    F: FnOnce() -> Fut + Send + 'static,
    CF: FnMut(&mut State, S) + StringifyCallback + 'static
> Task<S, (), TaskOneshot<S>> for (F, CF) {
    fn get(self) -> (Box<dyn Thread>, Callback<String>){
        let task: TaskOneshot<S> = Box::new(move || Box::pin((self.0)()));
        (Box::new((rand::rng().random(), task)), Box::new(self.1.stringify()))
    }
}
