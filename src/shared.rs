use arc_swap::{ArcSwap, Guard};
use arc_swap::access::{Access, Constant, Map};
use serde::{Serialize, de::DeserializeOwned};
use tokio::sync::{OnceCell, broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;

use std::any::{Any, TypeId, type_name};
use std::collections::HashMap;
use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::Deref;
use std::sync::{Arc, LazyLock, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::runtime::RUNTIME;

static NEXT_SUBSCRIBER_ID: AtomicU64 = AtomicU64::new(0);
static SHARED: LazyLock<Mutex<HashMap<(TypeId, Key), SharedEntry>>> = LazyLock::new(Mutex::default);
static SERVICES: LazyLock<Mutex<HashMap<Key, JoinHandle<()>>>> = LazyLock::new(Mutex::default);

type Request<S> = (u64, <S as Sharable>::Request, oneshot::Sender<<S as Sharable>::Response>);
type Notification<S> = (u64, u64, Arc<<S as Sharable>::Response>);
type SharedData<S> = (
    Arc<ArcSwap<(u64, S)>>, mpsc::UnboundedSender<Request<S>>, broadcast::WeakSender<Notification<S>>,
);
type SharedEntry = Arc<OnceCell<Box<dyn Any + Send + Sync>>>;

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct Key(pub TypeId, pub u64);
impl Key {
    pub fn new<T: 'static>(init: &impl Hash) -> Self {
        let mut hasher = DefaultHasher::new();
        init.hash(&mut hasher);
        Self(TypeId::of::<T>(), hasher.finish())
    }
}

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

    /// Persistence namespace; adapters preserve their wrapped type's existing cache keys.
    fn name() -> &'static str {type_name::<Self>()}
    fn persistent() -> bool {true}
    fn capacity() -> usize {1024}
    fn key(init: &Self::Init) -> Key {Key::new::<Self>(init)}
    fn cache_key(init: &Self::Init) -> Option<String> {
        Self::persistent().then(|| format!("{}:{:016x}", Self::name(), Self::key(init).1))
    }
    fn restore(_init: &Self::Init, bytes: &[u8]) -> Self {
        postcard::from_bytes(bytes).expect("Invalid cached shared state")
    }
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
    key: Option<(TypeId, Key)>,
    value: Arc<ArcSwap<(u64, S)>>,
    requests: mpsc::UnboundedSender<Request<S>>,
    sender: broadcast::WeakSender<Notification<S>>,
    subscriber: u64,
    responses: Mutex<(u64, broadcast::Receiver<Notification<S>>)>,
}
impl<S: Sharable> Shared<S> {
    pub fn new(init: S::Init) -> Self {Self::open(init, true)}
    pub fn temporary(init: S::Init) -> Self {Self::open(init, false)}

    fn open(init: S::Init, registered: bool) -> Self {
        let runtime = RUNTIME.get().expect("Runtime must be started");
        let key = registered.then(|| (TypeId::of::<S>(), S::key(&init)));
        let once = match key {
            Some(key) => SHARED.lock().unwrap().entry(key).or_default().clone(),
            None => Arc::new(OnceCell::new()),
        };
        let shared = runtime.block_on(once.get_or_init(async || {
            let cache = if registered {S::cache_key(&init)} else {None};
            let cache = cache.map(|id| (runtime.cache(), id));
            let mut object = match cache.as_ref().and_then(|(cache, id)| cache.load_bytes(id).unwrap()) {
                Some(bytes) => S::restore(&init, &bytes),
                None => {
                    let object = S::init(init).await;
                    if let Some((cache, id)) = &cache {cache.save(id, &object).unwrap();}
                    object
                }
            };
            let value = Arc::new(ArcSwap::from_pointee((0_u64, object.clone())));
            let (requests, mut receiver) = mpsc::unbounded_channel::<Request<S>>();
            let (responses, _) = broadcast::channel(S::capacity());
            let snapshot = value.clone();
            let sender = responses.downgrade();
            runtime.spawn(Box::pin(async move {
                let mut version = 0;
                while let Some((origin, request, reply)) = receiver.recv().await {
                    let response = object.handle(request).await;
                    if let Some((cache, id)) = &cache {cache.save(id, &object).unwrap();}
                    version += 1;
                    value.store(Arc::new((version, object.clone())));
                    let _ = responses.send((version, origin, Arc::new(response.clone())));
                    reply.send(response).unwrap_or_else(|_| panic!("Shared reply receiver dropped"));
                }
            }));
            Box::new((snapshot, requests, sender)) as Box<dyn Any + Send + Sync>
        }));
        let (value, requests, sender) = shared.downcast_ref::<SharedData<S>>().unwrap();
        let responses = sender.upgrade().unwrap().subscribe();
        let version = value.as_ref().load().0;
        Self {
            key, value: value.clone(), requests: requests.clone(), sender: sender.clone(),
            subscriber: NEXT_SUBSCRIBER_ID.fetch_add(1, Ordering::Relaxed),
            responses: Mutex::new((version, responses)),
        }
    }

    /// Releases the registry entry; remaining handles stay usable until they are dropped.
    pub fn shutdown(&self) {
        if let Some(key) = self.key {
            let mut shared = SHARED.lock().unwrap();
            let entry = shared.get(&key).and_then(|entry| entry.get());
            let matches = entry.and_then(|entry| entry.downcast_ref::<SharedData<S>>())
                .is_some_and(|(value, _, _)| Arc::ptr_eq(value, &self.value));
            let entry = matches.then(|| shared.remove(&key));
            drop(shared);
            drop(entry);
        }
    }

    pub fn update(&self, request: S::Request) -> S::Response {
        let (reply, response) = oneshot::channel();
        self.requests.send((self.subscriber, request, reply))
            .unwrap_or_else(|_| panic!("Shared worker has stopped"));
        RUNTIME.get().unwrap().block_on(response).expect("Shared worker stopped before replying")
    }

    pub fn load(&self) -> Ref<'_, S> {
        let mut responses = self.responses.lock().unwrap();
        // Subscribe before loading so updates after this snapshot cannot be missed.
        let current = self.sender.upgrade().expect("Shared worker has stopped").subscribe();
        let snapshot = self.value.as_ref().load();
        *responses = (snapshot.0, current);
        Ref::new(snapshot).map(|snapshot| &snapshot.1)
    }

    pub async fn next(&mut self) -> S::Response {
        let (seen, responses) = self.responses.get_mut().unwrap();
        loop {
            match responses.recv().await {
                Ok((version, origin, response)) => {
                    if version <= *seen {continue}
                    assert_eq!(version, *seen + 1, "Shared notifications skipped unseen updates");
                    *seen = version;
                    if origin != self.subscriber {return (*response).clone()}
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(error) => panic!("Shared notification failed: {error}"),
            }
        }
    }

    pub fn try_next(&mut self) -> Option<S::Response> {
        let (seen, responses) = self.responses.get_mut().unwrap();
        loop {
            match responses.try_recv() {
                Ok((version, origin, response)) => {
                    if version <= *seen {continue}
                    assert_eq!(version, *seen + 1, "Shared notifications skipped unseen updates");
                    *seen = version;
                    if origin != self.subscriber {return Some((*response).clone())}
                }
                Err(broadcast::error::TryRecvError::Empty) => return None,
                Err(broadcast::error::TryRecvError::Lagged(_)) => continue,
                Err(error) => panic!("Shared notification failed: {error}"),
            }
        }
    }
}
impl<S: Sharable> Clone for Shared<S> {
    fn clone(&self) -> Self {
        let responses = self.sender.upgrade().expect("Shared worker has stopped").subscribe();
        let version = self.value.as_ref().load().0;
        Self {
            key: self.key, value: self.value.clone(),
            requests: self.requests.clone(), sender: self.sender.clone(),
            subscriber: NEXT_SUBSCRIBER_ID.fetch_add(1, Ordering::Relaxed),
            responses: Mutex::new((version, responses)),
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
        Self::spawn(Key::new::<S>(&init), async move {
            let mut service = S::init(init).await;
            loop {
                let predicate = service.predicate().await;
                service.handle(predicate).await;
                tokio::task::yield_now().await;
            }
        });
    }
    pub fn shutdown<S: Service>(init: &S::Init) {Self::stop(Key::new::<S>(init))}
    pub fn spawn(key: Key, task: impl Future<Output = ()> + Send + 'static) {
        let runtime = RUNTIME.get().expect("Runtime must be started");
        let mut services = SERVICES.lock().unwrap();
        if services.get(&key).is_some_and(|task| !task.is_finished()) {return}
        services.insert(key, runtime.spawn(Box::pin(task)));
    }
    pub fn stop(key: Key) {
        let task = SERVICES.lock().unwrap().remove(&key);
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
