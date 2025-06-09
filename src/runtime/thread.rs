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

use crate::runtime::channel::{Channel, SerdeChannel};
use crate::runtime;

use crate::{State, hardware};
use super::{Callback, Id, Error};

//pub type TaskTick<'b, S, R, Fut> = Box<dyn FnMut(&mut Context<S, R>) -> Fut + Send>;
pub type ThreadChannel = SerdeChannel<ThreadResponse, ThreadRequest>;
pub type ThreadChannelR = SerdeChannel<ThreadRequest, ThreadResponse>;

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
pub trait _Thread: Send {
    async fn run(self: Box<Self>, ctx: hardware::Context, channel: ThreadChannel);

    fn id(&self) -> Id {rand::rng().random()} 
}

type Res = Result<Option<Duration>, Error>;

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

pub struct Thread<S, R>(
    pub Box<dyn for<'a> FnMut(&'a mut Context<S, R>) -> Pin<Box<dyn Future<Output = Res> + Send + 'a>> + Send>,
);
#[async_trait::async_trait]
impl<
    S: Serialize + for<'a> Deserialize <'a> + Send + 'static,
    R: Serialize + for<'a> Deserialize <'a> + Send + 'static,
    //Fut: Future<Output = Res> + Send + 'b,
> _Thread for Thread<S, R> {
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
                    let result = (self.0)(&mut ctx).await;
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

pub trait Task<S, R, Fut> {
    fn id(&self) -> Id {rand::rng().random()} 

    //fn get(self) -> (Box<dyn FnMut(&mut Context<S, R>) -> Fut + Send>, Callback<S>);
    fn get(self) -> (Box<dyn for<'b> FnMut(&'b mut Context<S, R>) -> Pin<Box<dyn Future<Output = Res> + Send + 'b>> + Send>, Callback<S>);
}

trait AsyncFnMutSend<I>: FnMut(I) -> Self::Fut {
    type Fut: Future<Output = <Self as AsyncFnMutSend::<I>>::Outpu> + Send;
    type Outpu;
}

impl<I, F, Fut> AsyncFnMutSend<I> for F
where
    F: FnMut(I) -> Fut + Send,
    Fut: Future + Send,
{
    type Fut = Fut;
    type Outpu = Fut::Output;
}

impl<
    S: Serialize + for<'a> Deserialize <'a> + Send + 'static,
    R: Serialize + for<'a> Deserialize <'a> + Send + 'static,
    //Fut: Future<Output = Res> + Send + 'b,
    F: for<'b> AsyncFnMutSend<&'b mut Context<S, R>, Outpu = Res> + Send + 'static
> Task<S, R, Pin<Box<dyn Future<Output = Res> + Send>>> for F {
    fn get(mut self) -> (Box<dyn for<'b> FnMut(&'b mut Context<S, R>) -> Pin<Box<dyn Future<Output = Res> + Send + 'b>> + Send>, Callback<S>){
        (Box::new(move |ctx: &mut Context<S, R>| {Box::pin(self(ctx))}), Box::new(|_: &mut State, _: S| {}))
    }
}

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


//  impl<
//      S: Serialize + for<'a> Deserialize <'a> + 'static,
//      R: Serialize + for<'a> Deserialize <'a> + 'static,
//      Fut: Future<Output = ()> + Send + 'static,
//      F: FnOnce(ThreadContext, Box<dyn Channel<S, R>>) -> Fut + Send + 'static,
//      CF: FnMut(&mut State, S) -> () + Send + 'static
//  > From<(F, CF)> for Thread<S, R, Pin<Box<dyn Future<Output = ()> + Send>>> {
//      fn from(f: (F, CF)) -> Self {
//          Thread(Box::new(|ctx: ThreadContext, channel: ThreadChannel<S, R> | {Box::pin(async move {
//              let mut channel = InlineChannel(channel);
//              (f.0)(ctx, Box::new(channel)).await
//          })}), Some(Box::new(f.1)))
//      }
//  }

//  impl<
//      S: Serialize + for<'a> Deserialize <'a> + 'static,
//      R: Serialize + for<'a> Deserialize <'a> + 'static,
//      Fut: Future<Output = Res> + Send + 'static,
//      //Fut2: Future<Output = ()> + Send + 'static,
//      T: Task<S, R, Fut> 
//  > From<T> for Thread<S, R, Pin<Box<dyn Future<Output = Res> + Send>>> {
//      fn from(f: F) -> Self {
//          Thread(Box::new(|ctx: ThreadContext, channel: ThreadChannel<S, R> | {Box::pin(async move {
//              let mut channel = InlineChannel(channel);
//              f(ctx, Box::new(channel)).await
//          })}), Some(Box::new(|_state: &mut State, _response: S| {})))
//      }
//  }






//  impl<
//      S: Serialize + for<'a> Deserialize <'a> + 'static,
//      R: Serialize + for<'a> Deserialize <'a> + 'static,
//      Fut: Future<Output = Res> + Send + 'static,
//      //Fut2: Future<Output = ()> + Send + 'static,
//      F: FnOnce(ThreadContext, Box<dyn Channel<S, R>>) -> Fut + Send + 'static
//  > From<F> for Thread<S, R, Pin<Box<dyn Future<Output = Res> + Send>>> {
//      fn from(f: F) -> Self {
//          Thread(Box::new(|ctx: ThreadContext, channel: ThreadChannel<S, R> | {Box::pin(async move {
//              let mut channel = InlineChannel(channel);
//              f(ctx, Box::new(channel)).await
//          })}), Some(Box::new(|_state: &mut State, _response: S| {})))
//      }
//  }



//Validate works for service
//Communication between threads needs to be handle between ctx/channel hybrid. await on ctx and get
//response should be user interface


//  pub struct VecDequeChannel<S, R>(Vec<S>, VecDeque<R>);
//  impl<
//      S: Serialize + for<'a> Deserialize <'a>,
//      R: Serialize + for<'a> Deserialize <'a>,
//  >Channel<S, R> for InlineChannel<S, R> {
//      fn send(&mut self, payload: S) {
//          self.0.send(ThreadResponse::Response(0, payload));
//      }

//      fn receive(&mut self) -> Option<R> {
//          while let Some(r) = self.0.receive() {
//              match r {
//                  ThreadRequest::Request(i, r) => return Some(r),
//                  _ => {}
//              }
//          }
//          None
//      }
//  }

//  pub struct InlineChannel<S, R>(ThreadChannel<S, R>);
//  impl<
//      S: Serialize + for<'a> Deserialize <'a>,
//      R: Serialize + for<'a> Deserialize <'a>,
//  >Channel<S, R> for InlineChannel<S, R> {
//      fn send(&mut self, payload: S) {
//          self.0.send(ThreadResponse::Response(0, payload));
//      }

//      fn receive(&mut self) -> Option<R> {
//          while let Some(r) = self.0.receive() {
//              match r {
//                  ThreadRequest::Request(i, r) => return Some(r),
//                  _ => {}
//              }
//          }
//          None
//      }
//  }


//  impl<
//      S: Serialize + for<'a> Deserialize <'a> + 'static,
//      R: Serialize + for<'a> Deserialize <'a> + 'static,
//      Fut: Future<Output = ()> + Send + 'static,
//      //Fut2: Future<Output = ()> + Send + 'static,
//      F: FnOnce(ThreadContext, Box<dyn Channel<S, R>>) -> Fut + Send + 'static
//  > From<F> for Thread<S, R, Pin<Box<dyn Future<Output = ()> + Send>>> {
//      fn from(f: F) -> Self {
//          Thread(Box::new(|ctx: ThreadContext, channel: ThreadChannel<S, R> | {Box::pin(async move {
//              let mut channel = InlineChannel(channel);
//              f(ctx, Box::new(channel)).await
//          })}), Some(Box::new(|_state: &mut State, _response: S| {})))
//      }
//  }


//  impl<
//      S: Serialize + for<'a> Deserialize <'a> + 'static,
//      R: Serialize + for<'a> Deserialize <'a> + 'static,
//      Fut: Future<Output = ()> + Send + 'static,
//      F: FnOnce(ThreadContext, Box<dyn Channel<S, R>>) -> Fut + Send + 'static,
//      CF: FnMut(&mut State, S) -> () + Send + 'static
//  > From<(F, CF)> for Thread<S, R, Pin<Box<dyn Future<Output = ()> + Send>>> {
//      fn from(f: (F, CF)) -> Self {
//          Thread(Box::new(|ctx: ThreadContext, channel: ThreadChannel<S, R> | {Box::pin(async move {
//              let mut channel = InlineChannel(channel);
//              (f.0)(ctx, Box::new(channel)).await
//          })}), Some(Box::new(f.1)))
//      }
//  }
