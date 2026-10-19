use std::any::{Any, TypeId, type_name};
use std::collections::HashMap;
use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::Deref;
use std::sync::{Arc, LazyLock, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};
use arc_swap::{ArcSwap, Guard};
use arc_swap::access::{Access, Constant, Map};
use serde::{Serialize, de::DeserializeOwned};
use tokio::sync::{OnceCell, broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;
use crate::RUNTIME;
use crate::hardware::Cache;

pub trait Service: Send + Sized + 'static {
    type Init: Hash + Send + 'static;
    type Predicate: Send;

    fn init(init: Self::Init) -> impl Future<Output = Self> + Send;
    fn predicate(&mut self) -> impl Future<Output = Self::Predicate> + Send;
    fn handle(&mut self, predicate: Self::Predicate) -> impl Future<Output = ()> + Send;
}

pub trait Sharable: Serialize + DeserializeOwned + Clone + Send + Sync + 'static {
    type Init: Hash + Send + 'static;
    type Request: Send + 'static;
    type Response: Clone + Send + Sync + 'static;

    fn persistent() -> bool {true}
    fn init(init: Self::Init) -> impl Future<Output = Self> + Send;
    fn handle(&mut self, request: Self::Request) -> impl Future<Output = Self::Response> + Send;
}

pub enum Ref<'a, T> {
    Shared(Guard<Arc<T>>),
    Mapped(Arc<dyn Deref<Target = T> + Send + Sync + 'a>),
}
impl<'a, T: Send + Sync + 'a> Ref<'a, T> {
    pub fn new(value: Guard<Arc<T>>) -> Self {Self::Shared(value)}
    pub fn map<R: 'a>(self, access: impl Fn(&T) -> &R + Clone + Send + Sync + 'a) -> Ref<'a, R> {
        Ref::Mapped(Arc::new(Map::new(Constant(self), move |value: &Ref<'a, T>| {
            access(value)
        }).load()))
    }
}
impl<T> AsRef<T> for Ref<'_, T> {fn as_ref(&self) -> &T {self}}
impl<T> Clone for Ref<'_, T> {
    fn clone(&self) -> Self {
        match self {
            Self::Shared(value) => Self::Shared(Guard::from_inner(Arc::clone(value))),
            Self::Mapped(value) => Self::Mapped(value.clone()),
        }
    }
}
impl<T> Deref for Ref<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        match self {Self::Shared(value) => value, Self::Mapped(value) => value}
    }
}
impl<T: Debug> Debug for Ref<'_, T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {(**self).fmt(f)}
}

pub struct Shared<S: Sharable> {
    key: Key,
    value: Arc<ArcSwap<S>>,
    requests: mpsc::UnboundedSender<Request<S>>,
    sender: broadcast::WeakSender<Notification<S>>,
    subscriber: u64,
    responses: Mutex<broadcast::Receiver<Notification<S>>>,
}
impl<S: Sharable> Shared<S> {
    pub fn new(init: S::Init) -> Self {
        let runtime = RUNTIME.get().expect("Runtime must be started");
        let key = Key::new::<S>(&init);
        let once = SHARED.lock().unwrap().entry(key).or_default().clone();
        let shared = runtime.block_on(once.get_or_init(async || {
            let cache = S::persistent().then(Cache::new);
            let id = format!("{}:{:016x}", type_name::<S>(), key.1);
            let mut object = match cache.as_ref().and_then(|cache| cache.load(&id)) {
                Some(object) => object,
                None => {
                    let object = S::init(init).await;
                    if let Some(cache) = &cache {cache.save(&id, &object);}
                    object
                }
            };
            let value = Arc::new(ArcSwap::from_pointee(object.clone()));
            let (requests, mut receiver) = mpsc::unbounded_channel::<Request<S>>();
            let (responses, _) = broadcast::channel(1024);
            let snapshot = value.clone();
            let sender = responses.downgrade();
            let task = runtime.spawn(Box::pin(async move {
                loop {
                    let (origin, request, reply) = receiver.recv().await.unwrap();
                    let response = object.handle(request).await;
                    if let Some(cache) = &cache {cache.save(&id, &object);}
                    value.store(Arc::new(object.clone()));
                    let _ = responses.send((origin, Arc::new(response.clone())));
                    reply.send(response).unwrap_or_else(|_| panic!("Shared reply receiver dropped"));
                }
            }));
            Box::new((snapshot, requests, sender, Mutex::new(Some(task)))) as Box<dyn Any + Send + Sync>
        }));
        let (value, requests, sender, _) = shared.downcast_ref::<SharedData<S>>().unwrap();
        let responses = sender.upgrade().unwrap().subscribe();
        Self {
            key, value: value.clone(), requests: requests.clone(), sender: sender.clone(),
            subscriber: NEXT_SUBSCRIBER_ID.fetch_add(1, Ordering::Relaxed),
            responses: Mutex::new(responses),
        }
    }

    pub fn shutdown(&self) {
        let worker = {
            let shared = SHARED.lock().unwrap();
            let Some(entry) = shared.get(&self.key).and_then(|entry| entry.get()) else {return};
            let (value, _, _, worker) = entry.downcast_ref::<SharedData<S>>().unwrap();
            if !Arc::ptr_eq(value, &self.value) {return}
            let worker = worker.lock().unwrap().take();
            worker
        };
        if let Some(worker) = worker {
            worker.abort();
            if let Err(error) = RUNTIME.get().unwrap().block_on(worker) {
                assert!(error.is_cancelled(), "Shared worker failed: {error}");
            }
            SHARED.lock().unwrap().remove(&self.key);
        }
    }

    pub fn update(&self, request: S::Request) -> S::Response {
        let (reply, response) = oneshot::channel();
        self.requests.send((self.subscriber, request, reply))
            .unwrap_or_else(|_| panic!("Shared worker has stopped"));
        RUNTIME.get().unwrap().block_on(response).expect("Shared worker stopped before replying")
    }

    pub fn load(&self) -> Ref<'_, S> {
        let current = self.sender.upgrade().expect("Shared worker has stopped").subscribe();
        *self.responses.lock().unwrap() = current;
        Ref::new(self.value.as_ref().load())
    }

    pub async fn next(&mut self) -> S::Response {
        let receiver = self.responses.get_mut().unwrap();
        loop {
            let (origin, response) = receiver.recv().await.unwrap();
            if origin != self.subscriber {return (*response).clone()}
        }
    }

    pub fn try_next(&mut self) -> Option<S::Response> {
        let receiver = self.responses.get_mut().unwrap();
        loop {
            match receiver.try_recv() {
                Ok((origin, response)) if origin != self.subscriber => return Some((*response).clone()),
                Ok(_) => {}
                Err(broadcast::error::TryRecvError::Empty) => return None,
                Err(error) => panic!("{error}"),
            }
        }
    }
}
impl<S: Sharable> Clone for Shared<S> {
    fn clone(&self) -> Self {
        let responses = self.sender.upgrade().expect("Shared worker has stopped").subscribe();
        Self {
            key: self.key,
            value: self.value.clone(), requests: self.requests.clone(), sender: self.sender.clone(),
            subscriber: NEXT_SUBSCRIBER_ID.fetch_add(1, Ordering::Relaxed),
            responses: Mutex::new(responses),
        }
    }
}
impl<S: Sharable> Debug for Shared<S> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.debug_struct("Shared").field("type", &type_name::<S>()).finish_non_exhaustive()
    }
}

pub struct Services;
impl Services {
    pub fn start<S: Service>(init: S::Init) {
        let runtime = RUNTIME.get().expect("Runtime must be started");
        let key = Key::new::<S>(&init);
        let mut services = SERVICES.lock().unwrap();
        if services.get(&key).is_some_and(|task| !task.is_finished()) {return}
        let task = runtime.spawn(Box::pin(async move {
            let mut service = S::init(init).await;
            loop {
                let predicate = service.predicate().await;
                service.handle(predicate).await;
                tokio::task::yield_now().await;
            }
        }));
        services.insert(key, task);
    }
    pub fn shutdown<S: Service>(init: &S::Init) {
        let task = SERVICES.lock().unwrap().remove(&Key::new::<S>(init));
        if let Some(task) = task {
            task.abort();
            if let Err(error) = RUNTIME.get().unwrap().block_on(task) {
                assert!(error.is_cancelled(), "Service failed: {error}");
            }
        }
    }
    pub fn shutdown_all() {
        loop {
            let tasks: Vec<_> = SERVICES.lock().unwrap().drain().map(|(_, task)| task).collect();
            if tasks.is_empty() {break}
            for task in &tasks {task.abort();}
            RUNTIME.get().unwrap().block_on(async {
                for task in tasks {
                    if let Err(error) = task.await {
                        assert!(error.is_cancelled(), "Service failed: {error}");
                    }
                }
            });
        }
    }
}

type Request<S> = (u64, <S as Sharable>::Request, oneshot::Sender<<S as Sharable>::Response>);
type Notification<S> = (u64, Arc<<S as Sharable>::Response>);
type SharedData<S> = (
    Arc<ArcSwap<S>>, mpsc::UnboundedSender<Request<S>>, broadcast::WeakSender<Notification<S>>,
    Mutex<Option<JoinHandle<()>>>,
);
type SharedEntry = Arc<OnceCell<Box<dyn Any + Send + Sync>>>;

static NEXT_SUBSCRIBER_ID: AtomicU64 = AtomicU64::new(0);
static SHARED: LazyLock<Mutex<HashMap<Key, SharedEntry>>> = LazyLock::new(Mutex::default);
static SERVICES: LazyLock<Mutex<HashMap<Key, JoinHandle<()>>>> = LazyLock::new(Mutex::default);

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
struct Key(TypeId, u64);
impl Key {
    fn new<T: 'static>(init: &impl Hash) -> Self {
        let mut hasher = DefaultHasher::new();
        init.hash(&mut hasher);
        Self(TypeId::of::<T>(), hasher.finish())
    }
}
