use std::fmt::Debug;
use std::borrow::Borrow;
use std::ops::Index;
use std::hash::{Hash, Hasher};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use std::any::{Any, TypeId};
use std::marker::PhantomData;
use std::collections::HashSet;
use std::sync::{MutexGuard, Mutex};

use crate::Runtime;
use crate::Cache;

use air::Id;
use arc_swap::ArcSwap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use crossfire::{MTx, AsyncTx, AsyncRx, Rx, spsc, mpsc};

#[allow(clippy::type_complexity)]
static SHARED: LazyLock<Arc<Mutex<HashMap<String, Arc<Box<dyn Fn() -> Box<dyn Any + Send + Sync> + Send + Sync>>>>>> = LazyLock::new(|| {
    Arc::new(Mutex::new(HashMap::default()))
});

type Request<S> = (<S as Sharable>::Request, AsyncTx<spsc::One<Box<<S as Sharable>::Response>>>);
type Queue<S> = Vec<(AsyncTx<spsc::One<Box<<S as Sharable>::Response>>>, Option<<S as Sharable>::Response>)>;
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

pub struct Responder<'a, R>(&'a mut Option<R>);
impl<'a, R> Responder<'a, R> {
    pub fn respond(self, response: R) {let _ = self.0.insert(response);}
}

pub struct Receiver<S: Sharable>(AsyncRx<mpsc::List<Request<S>>>, Queue<S>);
impl<S: Sharable> Receiver<S> {
    pub async fn receive(&mut self) -> (S::Request, Responder<'_, S::Response>) {
        let (request, responder) = self.0.recv().await.unwrap();
        (request, Responder(&mut self.1.push_mut((responder, None)).1))
    }

    async fn flush(&mut self) {
        for (tx, r) in self.1.drain(..) {
            if let Some(r) = r {
                let _ = tx.send(Box::new(r)).await;
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct Requester<S: Sharable>(MTx<mpsc::List<Request<S>>>, Runtime);
impl<S: Sharable> Requester<S> {
    pub fn request(&self, request: S::Request) -> S::Response {
        self.1.block_on(async {
            let (tx, rx): (_, AsyncRx<_>) = spsc::build(spsc::One::new());
            self.0.send((request, tx)).unwrap();
            *rx.recv().await.unwrap()
        })
    }
}

pub(crate) trait Sharable: Serialize + for<'a> Deserialize<'a> + Debug + Clone + Send + Sync + 'static {
    type Init: Hash;
    type Request: Send + 'static;
    type Response: Send + 'static;
    type Memory: Send + 'static;

    fn init(init: Self::Init) -> impl Future<Output = Self>;

    fn init_memory(&mut self) -> impl Future<Output = Self::Memory>;

    fn run(&mut self, memory: &mut Self::Memory, recevier: &mut Receiver<Self>) -> impl Future<Output = bool> + Send;
}

#[derive(Debug)]
pub struct Shared<S: Sharable>(Arc<ArcSwap<S>>, Requester<S>);
impl<S: Sharable> Shared<S> {
    pub fn new(runtime: Runtime, init: S::Init) -> Self {
        let name = std::any::type_name::<S>();
        let id = Id::hash(&(std::any::type_name::<S>(), &init)).to_string();
        *SHARED.lock().unwrap().entry(id.clone()).or_insert_with(|| {
            let mut cache = Cache::new(id.clone()).unwrap();
            let mut shared = cache.get::<S>("shared").unwrap().unwrap_or(runtime.block_on(S::init(init)));
            let mut memory = runtime.block_on(S::init_memory(&mut shared));
            let arc = Arc::new(ArcSwap::from(Arc::new(shared.clone())));
            let (tx, rx) = mpsc::build(mpsc::List::new());
            let mut rx = Receiver(rx, Vec::new());
            let a = arc.clone();
            runtime.spawn(Box::pin(async move { loop {
                if S::run(&mut shared, &mut memory, &mut rx).await {
                    cache.insert("shared", &shared).unwrap();
                    arc.store(Arc::new(shared.clone()));
                }
                rx.flush().await;
            }}));
            let shared = Shared(a, Requester(tx, runtime));
            Arc::new(Box::new(move || Box::new(shared.clone())))
        })().downcast::<Shared<S>>().unwrap()
    }

    pub fn as_ref(&self) -> Ref<'_, S> {Ref::new(self.0.load())}

    pub fn requester(&self) -> &Requester<S> {&self.1}

    pub fn request(&self, request: S::Request) -> S::Response {
        self.1.request(request)
    }
}
impl<S: Sharable> Clone for Shared<S> {fn clone(&self) -> Self {Shared(self.0.clone(), self.1.clone())}}

#[derive(Default)]
struct HashReader(Vec<u8>);
impl core::hash::Hasher for HashReader {
    fn finish(&self) -> u64 {panic!("NOOP");}
    fn write(&mut self, bytes: &[u8]) {self.0.extend(bytes);}
}
impl HashReader {
    pub fn read<H: Hash + ?Sized>(h: &H) -> Vec<u8> {
        let mut hasher = HashReader::default();
        h.hash(&mut hasher);
        hasher.0
    }
}
