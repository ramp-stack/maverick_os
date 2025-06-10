use std::sync::mpsc::{channel, Sender, Receiver};
use std::collections::hash_map::DefaultHasher;
use std::collections::hash_map::Entry;
use std::collections::VecDeque;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::hash::{Hasher, Hash};
use std::future::Future;
use std::sync::Arc;
use std::any::TypeId;
use std::pin::Pin;
use std::any::Any;

use downcast_rs::{impl_downcast, Downcast};
use tokio::time::{Instant, Duration};
use tokio::task::JoinHandle;
use serde::{Serialize, Deserialize};
use rand::Rng;

use crate::runtime;

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
    async fn run(self: Box<Self>, ctx: hardware::Context, channel: ThreadChannel);

    fn id(&self) -> Id {rand::rng().random()} 
}

pub trait Task<S, R, X> {
    fn id(&self) -> Id {rand::rng().random()} 

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

pub struct Context<S, R> {
    pub hardware: hardware::Context,
    send: VecDeque<(Id, S)>,
    receive: VecDeque<(Id, R)>,
}
impl<S, R> Context<S, R> {
  //pub async fn request<T: Task<S, R>, S, R>(&mut self, request: R) {
  //    todo!()
  //}

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
> Thread for TaskTick<S, R> {
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
                    let result = (self)(&mut ctx).await;
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
}

impl<
    S: Serialize + for<'a> Deserialize <'a> + Send + 'static,
    R: Serialize + for<'a> Deserialize <'a> + Send + 'static,
    F: for<'b> AsyncFnMutSend<&'b mut Context<S, R>, Out = Res> + Send + 'static
> Task<S, R, TaskTick<S, R>> for F {
    fn get(mut self) -> (Box<dyn Thread>, Callback<S>){
        let task: TaskTick<S, R> = Box::new(move |ctx: &mut Context<S, R>| Box::pin(self(ctx)));
        (Box::new(task), Box::new(|_: &mut State, _: S| {}))
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
        (Box::new(task), Box::new(self.1))
    }
}

//ONESHOT

type TaskOneshot<S> = Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = S> + Send>> + Send>;

#[async_trait::async_trait]
impl<
    S: Serialize + for<'a> Deserialize <'a> + Send + 'static,
> Thread for TaskOneshot<S> {
    async fn run(mut self: Box<Self>, hardware: hardware::Context, mut channel: ThreadChannel) {
        let s = (self)().await;
        channel.send(ThreadResponse::Response(0, serde_json::to_string(&s).unwrap()));
    }
}

impl<
    S: Serialize + for<'a> Deserialize <'a> + Send + 'static,

    Fut: Future<Output = S> + Send + 'static,
    F: FnOnce() -> Fut + Send + 'static,
> Task<S, (), TaskOneshot<S>> for F {
    fn get(mut self) -> (Box<dyn Thread>, Callback<S>){
        let task: TaskOneshot<S> = Box::new(move || Box::pin(self()));
        (Box::new(task), Box::new(|_: &mut State, _: S| {}))
    }
}

impl<
    S: Serialize + for<'a> Deserialize <'a> + Send + 'static,

    Fut: Future<Output = S> + Send + 'static,
    F: FnOnce() -> Fut + Send + 'static,
    CF: FnMut(&mut State, S) + 'static
> Task<S, (), TaskOneshot<S>> for (F, CF) {
    fn get(mut self) -> (Box<dyn Thread>, Callback<S>){
        let task: TaskOneshot<S> = Box::new(move || Box::pin((self.0)()));
        (Box::new(task), Box::new(self.1))
    }
}

//  impl<
//      S: Serialize + for<'a> Deserialize <'a> + Send + 'static,
//      R: Serialize + for<'a> Deserialize <'a> + Send + 'static,
//      F: for<'b> AsyncFnMutSend<&'b mut Context<S, R>, Out = Res> + Send + 'static,
//      CF: FnMut(&mut State, S) + 'static
//  > Task<S, R> for (F, CF) {
//      fn get(mut self) -> (TaskTick<S, R>, Callback<S>){
//          (Box::new(move |ctx: &mut Context<S, R>| Box::pin((self.0)(ctx))), Box::new(self.1))
//      }
//  }

//  impl<
//      S: Serialize + for<'a> Deserialize <'a> + Send + 'static,
//      R: Serialize + for<'a> Deserialize <'a> + Send + 'static,
//      //F: for<'b> AsyncFnMutSend<&'b mut Context<S, R>, Out = Res> + Send + 'static

//      Fut: Future<Output = S> + Send + 'static,
//      F: FnMut() -> Fut + Send + 'static,
//  > Task<S, R> for F {
//      fn get(mut self) -> (Box<dyn Thread>, Callback<S>){
//          (Box::new(Thread::<S, R, TaskTick<S, R>>::new(Box::new(move |ctx: &mut Context<S, R>| {
//              Box::pin(async move {self().await; Ok(None)})
//          }))), Box::new(|_: &mut State, _: S| {}))
//      }
//  }

//  impl<
//      S: Serialize + for<'a> Deserialize <'a> + Send + 'static,
//      Fut: Future<Output = S> + Send + 'static,
//      F: FnOnce() -> Fut + Send + 'static,
//  > Task<S, ()> for F {
//      fn get(mut self) -> (TaskTick<S, ()>, Callback<S>){
//          (Box::new(move |ctx: &mut Context<S, ()>| Box::pin(async move {
//              ctx.callback(self().await);
//              Ok(None)
//          })), Box::new(|_: &mut State, _: S| {}))
//      }
//  }

//  fn test<'c, 
//      S: Serialize + for<'a> Deserialize <'a> + Send + 'static,
//      R: Serialize + for<'a> Deserialize <'a> + Send + 'static,
//      F: for<'b> AsyncFnMutSend<&'b mut Context<S, R>, Out = Res> + Send + 'static
//  >(f: &'c mut F, ctx: &'c mut Context<S, R>) -> Pin<Box<dyn Future<Output = Res> + Send + 'c>> {
//       Box::pin(async move {f(ctx).await})
//  }

//  impl<
//      S: Serialize + for<'a> Deserialize <'a> + Send + 'static,
//      R: Serialize + for<'a> Deserialize <'a> + Send + 'static,
//      F: for<'b> AsyncFnMutSend<&'b mut Context<S, R>, Out = Res> + Send + 'static,
//      CF: FnMut(&mut State, S) -> () + Send + 'static
//  > Task<S, R> for (F, CF) {
//      fn get(mut self) -> (Box<dyn for<'b> FnMut(&'b mut Context<S, R>) -> Pin<Box<dyn Future<Output = Res> + Send + 'b>> + Send>, Callback<S>){
//          (Box::new(move |ctx: &mut Context<S, R>| {Box::pin(self.0(ctx))}), Box::new(self.1))
//      }
//  }
//





//  impl<
//      S: Serialize + for<'a> Deserialize <'a> + Send + 'static,
//      Fut: Future<Output = S> + Send + 'static,
//      F: FnMut() -> Fut + Send + 'static,
//      //F: for<'b> AsyncFnMutSend<&'b mut Context<S, ()>, Out = S> + Send + 'static,
//      CF: FnMut(&mut State, S) + Send + 'static
//  > Task<S, ()> for (F, CF) {
//      fn get(mut self) -> (TaskTick<S, ()>, Callback<S>){
//          
//          //let test: Box<dyn for<'b> FnMut(&'b mut Context<S, ()>) -> Pin<Box<dyn Future<Output = S> + Send + 'b>>> = Box::new(move |ctx: &mut Context<S, ()>| {Box::pin((self.0)())});
//          //let mut test: Box<dyn FnMut() -> Pin<Box<dyn Future<Output = S> + Send>> + Send> = Box::new(move || {Box::pin((self.0)())});
//        //let test2: Box<dyn FnOnce(&mut Context<S, ()>) -> Pin<Box<dyn Future<Output = Res> + Send>>> = Box::new(move |ctx: &mut Context<S, ()>| {
//        //    let func = &mut self.0;
//        //    Box::pin(async move {
//        //        let val = func().await;
//        //        Ok::<_, Error>(None::<Duration>)
//        //    })
//        //});
//        //    (self.0)().await;
//        //    Ok(None)
//        //})});

//          (Box::new(move |ctx: &mut Context<S, ()>, _: []| {
//              Box::pin(async {
//                  //let test = (self.0)().await;
//                  Ok(None)
//              })
//          }), Box::new(self.1))

//          //let test: Box<dyn for<'b> AsyncFnMutSend<&'b mut Context<S, ()>, Out = S, Output = Pin<Box<dyn Future<Output = S> + Send + 'b>>> + Send + 'static> = Box::new(self.0);
//        //let mut closure: Box<dyn for<'a> FnMut(&'a mut Context<S, ()>, [&'a &'max (); 0]) -> Pin<Box<dyn Future<Output = Res> + Send + 'a>> + Send> = Box::new(move |ctx: &mut Context<S, ()>, _: [&'a &'max (); 0]| {
//        //    Box::pin(async {
//        //        (self.0)().await; 
//        //        Ok(None) 
//        //    })
//        //});
//        //let test: Box<dyn for<'b> FnMut(&'b mut Context<S, ()>) -> Pin<Box<dyn Future<Output = Res> + Send + 'b>> + Send> = Box::new(
//        //    move |ctx: &mut Context<S, ()>| {Box::pin(closure(ctx))}
//        //);
//        //let (task, _) = (|ctx: &mut Context<S, ()>| {let func = &mut self.0; async move {
//        //    let val = func(ctx).await;
//        //    ctx.callback(val);
//        //    Ok(None)
//        //}}).get();
//        //(task, Box::new(self.1))
//      }
//  }

//fn hr_bump<A, F: Fn(&mut A) -> &A>(f: F) -> F { f }

//  impl<
//      S: Serialize + for<'a> Deserialize <'a> + 'static,
//      R: Serialize + for<'a> Deserialize <'a> + 'static,
//      Fut: Future<Output = Res> + Send + 'static,
//      F: FnMut(&mut Context<S, R>) -> Fut + Send + 'static,
//      CF: FnMut(&mut State, S) -> () + Send + 'static
//  > Task<S, R, Fut> for (F, CF) {
//      fn get(self) -> (TaskTick<S, R, Fut>, Callback<S>) {
//          (Box::new(self.0), Box::new(self.1))
//      }
//  }
