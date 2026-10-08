use std::fmt::Debug;
use std::hash::Hash;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use std::any::Any;
use std::marker::PhantomData;
use std::collections::HashSet;
use std::sync::Mutex;
use std::sync::OnceLock;

use crate::{Runtime, RUNTIME};
use crate::Cache;

use air::Id;
use arc_swap::ArcSwap;
use serde::{Deserialize, Serialize};
use crossfire::{MTx, AsyncTx, AsyncRx, spsc, mpsc};
use postage::broadcast::{channel, Sender, Receiver as PReceiver};
use postage::prelude::{Stream, Sink};

#[allow(clippy::type_complexity)]
static SHARED: LazyLock<Arc<Mutex<HashMap<String, Arc<OnceLock<Arc<Box<dyn Fn() -> Box<dyn Any + Send + Sync> + Send + Sync>>>>>>>> = LazyLock::new(|| {
    Arc::new(Mutex::new(HashMap::default()))
});

type Request<S> = (<S as Sharable>::Request, AsyncTx<spsc::One<(Id, Box<<S as Sharable>::Response>)>>);
type Guard<T> = arc_swap::Guard<Arc<T>, arc_swap::DefaultStrategy>;

pub enum Ref<'a, T> {
    Arc(Guard<T>, PhantomData::<&'a ()>),
    #[allow(clippy::type_complexity)]
    Map(Arc<Box<dyn for<'b> Fn(&'b ()) -> &'b T + Send + Sync>>, PhantomData::<&'a ()>)
}
impl<'a, T: Send + Sync + 'static> Ref<'a, T> {
    pub fn new(arc: Guard<T>) -> Self {Ref::Arc(arc, PhantomData::<&'a ()>)}
    pub fn map<R>(self, access: impl for<'b> Fn(&'b T) -> &'b R + Sync + Send + 'static) -> Ref<'a, R> {match self {
        Ref::Arc(a, p) => Ref::Map(Arc::new(Box::new(move |_: &()| {
            let r: &R = access(a.as_ref());
            unsafe { &*(r as *const R) }
        })), p),
        Ref::Map(f, p) => Ref::Map(Arc::new(Box::new(move |t: &()| {
            let r: &R = access(f(t));
            unsafe { &*(r as *const R) }
        })), p),
    }}
}
impl<'a, T> AsRef<T> for Ref<'a, T> {fn as_ref(&self) -> &T {self}}
impl<'a, T: Debug> Debug for Ref<'a, T> {fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {(**self).fmt(f)}}
impl<'a, T> std::ops::Deref for Ref<'a, T> {type Target = T; fn deref(&self) -> &T {match self {Self::Arc(arc, _) => arc.as_ref(), Self::Map(f, _) => f(&())}}}
impl<'a, T> Clone for Ref<'a, T> {fn clone(&self) -> Self {match self {Ref::Arc(a, p) => Ref::Arc(Guard::from_inner(Arc::clone(a)), *p), Ref::Map(f, p) => Ref::Map(f.clone(), *p)}}}

#[derive(Debug, Clone)]
pub struct Requester<S: Sharable>(MTx<mpsc::List<Request<S>>>, Runtime, Arc<tokio::sync::Mutex<HashSet<Id>>>);
impl<S: Sharable> Requester<S> {
    pub fn new(runtime: Runtime) -> (AsyncRx<mpsc::List<Request<S>>>, Self) {
        let (tx, rx): (_, AsyncRx<_>) = mpsc::build(mpsc::List::new());
        (rx, Requester(tx, runtime, Arc::new(tokio::sync::Mutex::new(HashSet::new()))))
    }

    pub fn request(&self, request: S::Request) -> S::Response {
        self.1.block_on(async {
            let (tx, rx): (_, AsyncRx<_>) = spsc::build(spsc::One::new());
            self.0.send((request, tx)).unwrap();
            let (id, b) = rx.recv().await.unwrap();
            self.2.lock().await.insert(id);
            *b
        })
    }

    pub fn remove(&self, id: &Id) -> bool {RUNTIME.get().unwrap().block_on(self.2.lock()).remove(id)}
    pub fn clear(&self) {self.2.blocking_lock().clear()}
}

#[derive(Debug)]
pub struct Broadcaster<U>(Arc<tokio::sync::Mutex<PReceiver<U>>>, Sender<U>);
impl<U: Debug + Clone> Broadcaster<U> {
    pub fn new() -> (Sender<U>, Self) {
        let (bx, lx) = channel(10_000);
        (bx.clone(), Broadcaster(Arc::new(tokio::sync::Mutex::new(lx)), bx))
    }
    pub async fn send(&mut self, update: U) {self.1.send(update).await.unwrap()}
    pub async fn next(&self) -> U {self.0.lock().await.recv().await.unwrap()}
    pub fn try_next(&self) -> Option<U> {self.0.blocking_lock().try_recv().ok()}
    pub fn clear(&self) {*self.0.blocking_lock() = self.1.subscribe();}
}
impl<U: Clone> Clone for Broadcaster<U> {fn clone(&self) -> Self {
    Self(Arc::new(tokio::sync::Mutex::new(RUNTIME.get().unwrap().block_on(self.0.lock()).clone())), self.1.clone())
}}

#[derive(Clone, Debug)]
pub struct Pending<R: Clone + Send + Sync + 'static>(Arc<ArcSwap<(R, bool)>>, Broadcaster<bool>);
impl<R: Clone + Send + Sync + 'static> Pending<R> {
    pub(crate) fn new(pending: R) -> Self {
        let (_, br) = Broadcaster::new();
        Self(Arc::new(ArcSwap::from(Arc::new((pending, false)))), br)
    }
    pub(crate) async fn update(&mut self, update: R, confirmed: bool) {self.0.store((update, confirmed).into()); self.1.send(confirmed).await;}

    pub fn pending(&self) -> Ref<'_, R> {
        self.1.clear();
        Ref::new(self.0.load()).map(|r| &r.0)
    }
    pub fn try_confirmed(&self) -> Option<R> {
        let r = self.0.load(); r.1.then(|| r.0.clone())
    }
    pub async fn confirmed(self) -> R {loop {if self.1.next().await {return self.0.load().0.clone();}}}

    pub fn try_next(&self) -> Option<bool> {self.1.try_next()}
    pub async fn next(&self) -> bool {self.1.next().await}
}

pub trait Sharable: Serialize + for<'a> Deserialize<'a> + Debug + Clone + Send + Sync + 'static {
    type Init: Hash;
    type Request: Send + 'static;
    type Response: Debug + Clone + Send + 'static;
    type Memory: Send + 'static;
    type Predicate: Send + 'static;

    fn init(init: Self::Init) -> impl Future<Output = Self>;
    fn init_memory(&mut self) -> impl Future<Output = Self::Memory>;

    fn predicate(&mut self, memory: &mut Self::Memory) -> impl Future<Output = Self::Predicate> + Send;

    fn handle_predicate(&mut self, predicate: Self::Predicate) -> impl Future<Output = Option<Self::Response>> + Send;

    fn handle_request(&mut self, request: Self::Request) -> impl Future<Output = Self::Response> + Send;

    fn update(&mut self, _memory: &mut Self::Memory) -> impl Future<Output = ()> + Send {async {}}
}

#[derive(Debug, Clone)]
pub struct Shared<S: Sharable>(Arc<ArcSwap<S>>, Requester<S>, Broadcaster<(Id, S::Response)>);
impl<S: Sharable> Shared<S> {
    pub fn new(init: S::Init) -> Self {
        let runtime = RUNTIME.get().expect("Runtime must be started").clone();
        let id = Id::hash(&(std::any::type_name::<S>(), &init)).to_string();
        let once: Arc<OnceLock<_>> = SHARED.lock().unwrap().entry(id.clone()).or_insert_with(|| Arc::new(OnceLock::new())).clone();
        *once.get_or_init(|| {
            let mut cache = Cache::new(id.clone()).unwrap();
            let mut sharable = cache.get::<S>("sharable").unwrap().unwrap_or(runtime.block_on(S::init(init)));
            let mut memory = runtime.block_on(sharable.init_memory());

            let arc = Arc::new(ArcSwap::from(Arc::new(sharable.clone())));
            let (rx, rr) = Requester::new(runtime.clone());
            let (mut bx, br) = Broadcaster::new();
            let a = arc.clone();
            runtime.spawn(Box::pin(async move { loop {
                let (responder, response) = tokio::select! {
                    biased; 
                    r = rx.recv() => {
                        let (request, responder): (_, AsyncTx<_>) = r.unwrap();
                        let response = sharable.handle_request(request).await;
                        (Some(responder), Some(response))
                    },
                    predicate = sharable.predicate(&mut memory) => {
                        let r = sharable.handle_predicate(predicate).await;
                        (None, r)
                    },
                };
                sharable.update(&mut memory).await;
                cache.insert("sharable", &sharable).unwrap();
                arc.store(Arc::new(sharable.clone()));
                if let Some(response) = response {
                    let id = Id::random();
                    if let Some(responder) = responder { 
                        let _ = responder.send((id, Box::new(response.clone()))).await; 
                    }
                    bx.send((id, response.clone())).await.unwrap();
                }
            }}));
            let shared = Shared(a, rr, br);
            Arc::new(Box::new(move || Box::new(shared.clone())))
        })().downcast::<Shared<S>>().unwrap()
    }

    pub async fn next(&mut self) -> S::Response {loop {
        let (id, r) = self.2.next().await;
        if !self.1.remove(&id) {
            return r;
        }
    }}

    pub fn try_next(&mut self) -> Option<S::Response> {loop {
        let (id, r) = self.2.try_next()?;
        if !self.1.remove(&id) {
            return Some(r);
        }
    }}

    pub fn as_ref(&self) -> Ref<'_, S> {
        self.2.clear();
        self.1.clear();
        Ref::new(self.0.load())
    }

    pub fn request(&self, request: S::Request) -> S::Response {self.1.request(request)}
}
