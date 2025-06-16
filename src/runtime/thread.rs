use std::collections::VecDeque;
use std::hash::{DefaultHasher, Hasher, Hash};
use std::future::Future;
use std::any::TypeId;
use std::pin::Pin;

use tokio::time::{Instant, Duration};
use serde::{Serialize, Deserialize};
use rand::Rng;

use crate::{State, hardware};
use super::{Callback, Id, Error, Channel};

pub type ThreadChannel = Channel<ThreadResponse, ThreadRequest>;
pub type ThreadChannelR = Channel<ThreadRequest, ThreadResponse>;

#[derive(Serialize, Deserialize, Debug)]
pub enum ThreadRequest {
    Request(Id, String),
    Resume,
    Pause,
}

#[derive(Serialize, Deserialize)]
pub enum ThreadResponse {
    Response(Id, String),
    Error(Error),
}

#[async_trait::async_trait]
pub trait Thread: Send {
    type Send: Serialize + for<'a> Deserialize <'a> + Send where Self: Sized;
    type Receive: Serialize + for<'a> Deserialize <'a> + Send where Self: Sized;

    async fn run(self: Box<Self>, ctx: hardware::Context, channel: ThreadChannel);

    fn id(&self) -> Id;
}

pub trait Task<S, R, X> {
    fn get(self) -> (Box<dyn Thread>, Callback<S>);
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

pub struct Context<S, R> {
    pub hardware: hardware::Context,
    send: VecDeque<(Id, S)>,
    receive: VecDeque<(Id, R)>,
}
impl<S, R> Context<S, R> {
    pub async fn request<T: Thread>(&mut self, request: T::Receive) {
        todo!()
    }

    pub fn get_request(&mut self) -> Option<(Id, R)> {
        self.receive.pop_back()
    }

    pub fn respond(&mut self, id: Id, payload: S) {
        self.send.push_front((id, payload));
    }

    pub fn callback(&mut self, payload: S) {
        self.send.push_front((0, payload));
    }
}

#[async_trait::async_trait]
impl<
    S: Serialize + for<'a> Deserialize <'a> + Send + 'static,
    R: Serialize + for<'a> Deserialize <'a> + Send + 'static,
> Thread for TickingTask<S, R> {
    type Send = S;
    type Receive = R;

    async fn run(mut self: Box<Self>, hardware: hardware::Context, mut channel: ThreadChannel) {
        let mut ctx = Context{hardware, send: VecDeque::new(), receive: VecDeque::new()};
        let mut error_count = 0;
        let mut last_run = Instant::now();
        let mut duration = Duration::ZERO;
        let mut paused = false;
        loop {
            while let Some(request) = channel.receive() {match request {
                ThreadRequest::Request(id, payload) => ctx.receive.push_front((id, serde_json::from_str(&payload).unwrap())),
                ThreadRequest::Pause => paused = true,
                ThreadRequest::Resume => paused = false,
            }}
            if !paused {
                let elapsed = last_run.elapsed();
                if elapsed > duration {
                    last_run = Instant::now();
                    let result = (self.1)(&mut ctx).await;
                    for (id, payload) in ctx.send.drain(..) {
                        channel.send(ThreadResponse::Response(id, serde_json::to_string(&payload).unwrap()));  
                    }
                    match result {
                        Ok(None) => return,
                        Ok(Some(dur)) => duration = dur,
                        Err(e) if error_count < 3 => {
                            error_count += 1; 
                            log::error!("Thread {}, Errored {} :? {:?}", self.id(), e, e)
                        },
                        Err(e) => channel.send(ThreadResponse::Error(e))
                    }
                } else {
                    tokio::time::sleep(duration - elapsed).await
                }
            }
        }
    }

    fn id(&self) -> Id {self.0}
}

impl<
    S: Serialize + for<'a> Deserialize <'a> + Send + 'static,
    R: Serialize + for<'a> Deserialize <'a> + Send + 'static,
    F: for<'b> AsyncFnMutSend<&'b mut Context<S, R>, Out = Res> + Send + 'static
> Task<S, R, TaskTick<S, R>> for F {
    fn get(mut self) -> (Box<dyn Thread>, Callback<S>){
        let task: TaskTick<S, R> = Box::new(move |ctx: &mut Context<S, R>| Box::pin(self(ctx)));
        (Box::new(TickingTask(rand::rng().random(), task)), Box::new(|_: &mut State, _: S| {}))
    }
}

impl<
    S: Serialize + for<'a> Deserialize <'a> + Send + 'static,
    R: Serialize + for<'a> Deserialize <'a> + Send + 'static,
    F: for<'b> AsyncFnMutSend<&'b mut Context<S, R>, Out = Res> + Send + 'static,
    CF: FnMut(&mut State, S) + 'static
> Task<S, R, TaskTick<S, R>> for (F, CF) {
    fn get(mut self) -> (Box<dyn Thread>, Callback<S>){
        let task: TaskTick<S, R> = Box::new(move |ctx: &mut Context<S, R>| Box::pin((self.0)(ctx)));
        (Box::new(TickingTask(rand::rng().random(), task)), Box::new(self.1))
    }
}

//SERVICE

#[async_trait::async_trait]
pub trait Service: Send {
    type Send: Serialize + for<'a> Deserialize <'a> + Send + 'static;
    type Receive: Serialize + for<'a> Deserialize <'a> + Send + 'static;

    async fn new(ctx: &mut hardware::Context) -> Self where Self: Sized;

    async fn run(&mut self, ctx: &mut Context<Self::Send, Self::Receive>) -> Result<Option<Duration>, Error>;

    fn callback(state: &mut State, payload: String); //-> Box<Callback> {Box::new(|_state: &mut State, _response: String| {})}

  //fn background_tasks(&self) -> Vec<Box<dyn BackgroundTask>> {vec![]}
  //fn services(&self) -> ServiceList {BTreeMap::new()}
}

#[async_trait::async_trait]
impl<
    SE: Service + 'static
> Thread for SE {
    type Send = SE::Send;
    type Receive = SE::Receive;

    async fn run(mut self: Box<Self>, hardware: hardware::Context, mut channel: ThreadChannel) {
        let mut ctx = Context{hardware, send: VecDeque::new(), receive: VecDeque::new()};
        let mut error_count = 0;
        let mut last_run = Instant::now();
        let mut duration = Duration::ZERO;
        let mut paused = false;
        loop {
            while let Some(request) = channel.receive() {match request {
                ThreadRequest::Request(id, payload) => ctx.receive.push_front((id, serde_json::from_str(&payload).unwrap())),
                ThreadRequest::Pause => paused = true,
                ThreadRequest::Resume => paused = false,
            }}
            if !paused {
                let elapsed = last_run.elapsed();
                if elapsed > duration {
                    last_run = Instant::now();
                    let result = SE::run(&mut self, &mut ctx).await;
                    for (id, payload) in ctx.send.drain(..) {
                        channel.send(ThreadResponse::Response(id, serde_json::to_string(&payload).unwrap()));  
                    }
                    match result {
                        Ok(None) => return,
                        Ok(Some(dur)) => duration = dur,
                        Err(e) if error_count < 3 => {
                            error_count += 1; 
                            log::error!("Thread {}, Errored {} :? {:?}", self.id(), e, e)
                        },
                        Err(e) => channel.send(ThreadResponse::Error(e))
                    }
                } else {
                    tokio::time::sleep(duration - elapsed).await
                }
            }
        }
    }

    fn id(&self) -> Id {
        let mut hasher = DefaultHasher::default();
        TypeId::of::<SE>().hash(&mut hasher);
        hasher.finish()
    }
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

    fn id(&self) -> Id {self.0}
}

impl<
    S: Serialize + for<'a> Deserialize <'a> + Send + 'static,

    Fut: Future<Output = S> + Send + 'static,
    F: FnOnce() -> Fut + Send + 'static,
> Task<S, (), TaskOneshot<S>> for F {
    fn get(self) -> (Box<dyn Thread>, Callback<S>){
        let task: TaskOneshot<S> = Box::new(move || Box::pin(self()));
        (Box::new((rand::rng().random(), task)), Box::new(|_: &mut State, _: S| {}))
    }
}

impl<
    S: Serialize + for<'a> Deserialize <'a> + Send + 'static,

    Fut: Future<Output = S> + Send + 'static,
    F: FnOnce() -> Fut + Send + 'static,
    CF: FnMut(&mut State, S) + 'static
> Task<S, (), TaskOneshot<S>> for (F, CF) {
    fn get(self) -> (Box<dyn Thread>, Callback<S>){
        let task: TaskOneshot<S> = Box::new(move || Box::pin((self.0)()));
        (Box::new((rand::rng().random(), task)), Box::new(self.1))
    }
}
