use std::collections::VecDeque;
use std::future::Future;
use std::fmt::Debug;
use std::pin::Pin;

pub use tokio::time::{Instant, Duration};
use serde::{Serialize, Deserialize};

use crate::{hardware, State, Id};
use super::{Thread, Context, IntoThread, ThreadChannel, Callback, ThreadResponse, StringifyCallback, Error};


//TICKING TASK
trait AsyncFnMutSend<I>: FnMut(I) -> Self::Fut {
    type Fut: Future<Output = <Self as AsyncFnMutSend::<I>>::Out> + Send;
    type Out;
}
impl<I, F: FnMut(I) -> Fut + Send, Fut: Future + Send> AsyncFnMutSend<I> for F {
    type Fut = Fut;
    type Out = Fut::Output;
}

type Res = Result<Option<Duration>, Error>;
type TaskTick<S, R> = Box<dyn for<'b> FnMut(&'b mut Context<S, R>) -> Pin<Box<dyn Future<Output = Res> + Send + 'b>> + Send>;
struct TickingTask<S, R>(Id, TaskTick<S, R>);

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
> IntoThread<S, R, TaskTick<S, R>> for F {
    fn into(mut self) -> (Box<dyn Thread>, Callback<String>){
        let task: TaskTick<S, R> = Box::new(move |ctx: &mut Context<S, R>| Box::pin(self(ctx)));
        (Box::new(TickingTask(Id::random(), task)), Box::new(|_: &mut State, _: String| {}))
    }
}

impl<
    S: Serialize + for<'a> Deserialize <'a> + Send + 'static,
    R: Serialize + for<'a> Deserialize <'a> + Send + 'static,
    F: for<'b> AsyncFnMutSend<&'b mut Context<S, R>, Out = Res> + Send + 'static,
    CF: FnMut(&mut State, S) + StringifyCallback + 'static
> IntoThread<S, R, TaskTick<S, R>> for (F, CF) {
    fn into(mut self) -> (Box<dyn Thread>, Callback<String>){
        let task: TaskTick<S, R> = Box::new(move |ctx: &mut Context<S, R>| Box::pin((self.0)(ctx)));
        (Box::new(TickingTask(Id::random(), task)), Box::new(self.1.stringify()))
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
        channel.send(ThreadResponse::Response(Id::MIN, serde_json::to_string(&s).unwrap()));
    }

    fn type_id() -> Option<Id> {None}

    fn id(&self) -> Id {self.0}
}

impl<
    S: Serialize + for<'a> Deserialize <'a> + Send + 'static,
    Fut: Future<Output = S> + Send + 'static,
    F: FnOnce() -> Fut + Send + 'static,
> IntoThread<S, (), TaskOneshot<S>> for F {
    fn into(self) -> (Box<dyn Thread>, Callback<String>){
        let task: TaskOneshot<S> = Box::new(move || Box::pin(self()));
        (Box::new((Id::random(), task)), Box::new(|_: &mut State, _: String| {}))
    }
}

impl<
    S: Serialize + for<'a> Deserialize <'a> + Send + 'static,
    Fut: Future<Output = S> + Send + 'static,
    F: FnOnce() -> Fut + Send + 'static,
    CF: FnMut(&mut State, S) + StringifyCallback + 'static
> IntoThread<S, (), TaskOneshot<S>> for (F, CF) {
    fn into(self) -> (Box<dyn Thread>, Callback<String>){
        let task: TaskOneshot<S> = Box::new(move || Box::pin((self.0)()));
        (Box::new((Id::random(), task)), Box::new(self.1.stringify()))
    }
}

//BUFFERED TASK

type TaskBuffer<S> = Box<dyn for<'b> FnMut(&'b mut Context<S, ()>) -> Pin<Box<dyn Future<Output = Result<S, Error>> + Send + 'b>> + Send>;
struct BufferedTask<S>(Id, TaskBuffer<S>);

#[async_trait::async_trait]
impl<
    S: Serialize + for<'a> Deserialize <'a> + Send + 'static,
> Thread for BufferedTask<S> {
    type Send = S;
    type Receive = ();

    async fn run(mut self: Box<Self>, hardware: hardware::Context, channel: ThreadChannel) {
        let mut ctx = Context::new(hardware, channel);
        loop {
            ctx.check_received();
            if !ctx.paused {
                let len = ctx.receive.drain(..).len();
                for _ in 0..len {
                    match (self.1)(&mut ctx).await {
                        Ok(r) => ctx.callback(r),
                        Err(e) => ctx.channel.send(ThreadResponse::Error(e))
                    }
                }
                tokio::time::sleep(Duration::from_millis(16)).await
            }
        }
    }

    fn type_id() -> Option<Id> {None}

    fn id(&self) -> Id {self.0}
}

impl<
    S: Serialize + for<'a> Deserialize <'a> + Send + 'static + Debug,
    F: for<'b> AsyncFnMutSend<&'b mut Context<S, ()>, Out = Result<S, Error>> + Send + 'static
> IntoThread<Result<S, Error>, (), TaskBuffer<S>> for F {
    fn into(mut self) -> (Box<dyn Thread>, Callback<String>){
        let task: TaskBuffer<S> = Box::new(move |ctx: &mut Context<S, ()>| Box::pin(self(ctx)));
        let id = Id::random();
        (Box::new(BufferedTask(id, task)), (Box::new(|state: &mut State, result: S| {
            state.get_mut::<VecDeque<S>>().push_front(result);
        }) as Callback<S>).stringify())
    }
}
