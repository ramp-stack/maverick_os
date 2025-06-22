use std::collections::BTreeMap;
use std::time::{Instant, Duration};
use std::any::TypeId;

use crate::{hardware, State, Id};
use crate::runtime::Error;
use super::{Thread, Context, Constructor, IntoThread, ThreadChannel, Callback, ThreadResponse, StringifyCallback};
use serde::{Serialize, Deserialize};

type Dependancies = Box<dyn FnOnce() -> ServiceList>;

pub trait Services {
    fn services() -> ServiceList {ServiceList::default()}
}

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
> IntoThread<SE::Send, SE::Receive, u32> for SE {
    fn into(self) -> (Box<dyn Thread>, Callback<String>){
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
                        Ok(Some(dur)) => {error_count = 0; duration = dur},
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
        Some(Id::hash(&TypeId::of::<SE>()))
    }

    fn id(&self) -> Id {Self::type_id().unwrap()}
}

#[derive(Default)]
pub struct ServiceList(pub BTreeMap<TypeId, (Constructor, BackgroundList, Dependancies)>);
impl ServiceList {
    pub fn insert<S: Service + 'static>(&mut self) {
        self.0.insert(TypeId::of::<S>(), (
            Box::new(|ctx: &mut hardware::Context| Box::pin(async move {
                IntoThread::into(S::new(ctx).await)
            })),
            S::background_tasks(),
            Box::new(S::services)
        ));
    }
}

//BACKGROUND TASK
#[async_trait::async_trait]
pub trait BackgroundTask: Send {
    async fn new(ctx: &mut hardware::Context) -> Self where Self: Sized;

    async fn run(&mut self, ctx: &mut hardware::Context) -> Result<Option<Duration>, Error>;
}

impl<
    BT: BackgroundTask + 'static
> IntoThread<(), (), i32> for BT {
    fn into(self) -> (Box<dyn Thread>, Callback<String>){
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
        Some(Id::hash(&TypeId::of::<BT>()))
    }

    fn id(&self) -> Id {Self::type_id().unwrap()}
}

#[derive(Default)]
pub struct BackgroundList(pub BTreeMap<TypeId, Constructor>);
impl BackgroundList {
    pub fn insert<BT: BackgroundTask + 'static>(&mut self) {
        self.0.insert(TypeId::of::<BT>(),
            Box::new(|ctx: &mut hardware::Context| Box::pin(async move {
                IntoThread::into(BT::new(ctx).await)
            }))
        );
    }
}
