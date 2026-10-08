pub use air::{Secret, Name, Id};
pub use air::contract::{Contract, Metadata};

use air::contract::{Location, Instance as _Instance, Output};
use air::names::DefaultResolver;
use air::inbox::{Inbox, RequestType};
use air::storage::Response;
use air::client::Client;

use crate::runtime::RUNTIME;
use crate::shared::{Shared, Sharable, Ref, Pending};

use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::future::pending;
use std::sync::LazyLock;
use std::sync::OnceLock;
use std::sync::Arc;

use crossfire::{AsyncRx, spsc};
use serde::{Deserialize, Serialize};
use futures::stream::{FuturesUnordered, StreamExt};
use tokio::sync::Mutex;

static CLIENT: OnceLock<Client> = OnceLock::new();
static RESOLVER: LazyLock<Arc<Mutex<DefaultResolver>>> = LazyLock::new(|| Arc::new(Mutex::new(DefaultResolver::default())));

#[derive(Clone)]
pub struct Context{
    secret: Secret,
}
impl Context {
    pub fn new(secret: Secret) -> Self {
        let client = RUNTIME.get().unwrap().block_on(Client::new(DefaultResolver::default()));
        let _ = CLIENT.set(client);
        Context{secret}
    }

    pub fn me(&self) -> Name {self.secret.name()}

    pub fn instances<C: Contract>(&self) -> Instances<C> {
        Instances(Shared::new(self.secret.clone()))
    }
}

#[derive(Clone, Debug)]
pub struct Instances<C: Contract>(Shared<_Instances<C>>);
impl<C: Contract> Instances<C> {
    pub fn create(&self, init: C::Init) -> Instance<C> {self.0.request(init)}
    pub fn list(&self) -> Ref<'_, HashMap<Id, Instance<C>>> {self.0.as_ref().map(|i| &i.0)}
    pub async fn next(&mut self) -> Instance<C> {self.0.next().await}
    pub fn try_next(&mut self) -> Option<Instance<C>> {self.0.try_next()}
}

#[derive(Clone, Debug)]
pub struct Instance<C: Contract>(Shared<RunningInstance<C>>, Shared<Inbox>);
impl<C: Contract> Instance<C> {
    pub fn send(&mut self, msg: C::Message) -> Pending<C::Result> {
        if let Update::Pending(p) = self.0.request(msg) {p} else {panic!("-_-");}
    }
    pub fn share(&self, name: Name) {self.1.request((name, *self.0.as_ref().0.location()));}
    pub fn pending(&self) -> Ref<'_, C> {self.0.as_ref().map(|i| i.0.pending())}
    pub fn confirmed(&self) -> Option<Ref<'_, C>> {
        let r = self.0.as_ref();
        r.0.confirmed().is_some().then(|| r.map(|i| i.0.confirmed().unwrap()))
    }

    pub async fn next(&mut self) -> Update<C::Result> {self.0.next().await}
    pub fn try_next(&mut self) -> Option<Update<C::Result>> {self.0.try_next()}
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(bound = "C: Contract")]
pub struct _Instances<C: Contract>(#[serde(skip)] HashMap<Id, Instance<C>>, Secret);
impl<C: Contract> Sharable for _Instances<C> {
    type Init = Secret;
    type Request = C::Init;
    type Response = Instance<C>;
    type Memory = Shared<Inbox>;
    type Predicate = Location;

    async fn init(init: Self::Init) -> Self {Self(HashMap::new(), init)}
    async fn init_memory(&mut self) -> Self::Memory {Shared::new(self.1.clone())}

    async fn predicate(&mut self, memory: &mut Self::Memory) -> Self::Predicate {loop {
        let location = memory.next().await;
        if location.contract == C::id() {
            break location;
        }
    }}

    async fn handle_predicate(&mut self, location: Self::Predicate) -> Option<Instance<C>> {
        let hash = Id::hash(&location);
        if !self.0.contains_key(&hash) {
            let instance = Instance(Shared::new(InstanceInit{secret: self.1.clone(), location, init: None}), Shared::new(self.1.clone()));
            self.0.insert(hash, instance.clone());
            Some(instance)
        } else {None}
    }

    async fn handle_request(&mut self, request: Self::Request) -> Self::Response {
        let location = _Instance::<C>::generate_location(&self.1, &request).unwrap();
        self.0.entry(Id::hash(&location)).or_insert_with(||
            Instance(Shared::new(InstanceInit{secret: self.1.clone(), location, init: Some(request)}), Shared::new(self.1.clone()))
        ).clone()
    }
}


pub struct InstanceInit<I> {secret: Secret, location: Location, init: Option<I> }
impl<I> Hash for InstanceInit<I> {fn hash<H: Hasher>(&self, hasher: &mut H) {self.secret.hash(hasher); self.location.hash(hasher);}}

#[derive(Debug, Clone)]
pub enum Update<R: Clone + Send + Sync + 'static> {Init, Pending(Pending<R>), Confirmed(R)}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(bound = "C: Contract")]
pub struct RunningInstance<C: Contract>(_Instance<C>, #[serde(skip)] VecDeque<Pending<C::Result>>);
impl<C: Contract> Sharable for RunningInstance<C> {
    type Init = InstanceInit<C::Init>;
    type Request = C::Message;
    type Response = Update<C::Result>;
    type Memory = AsyncRx<spsc::List<Response>>;
    type Predicate = Response;

    async fn init(sinit: Self::Init) -> Self {
        Self(match sinit.init {
            Some(init) => _Instance::new(sinit.secret, init).unwrap(),
            None => _Instance::receive(sinit.secret, sinit.location).unwrap()
        }, VecDeque::new())
    }

    async fn init_memory(&mut self) -> Self::Memory {
        CLIENT.get().unwrap().send(self.0.start()).await
    }

    async fn predicate(&mut self, memory: &mut Self::Memory) -> Self::Predicate {memory.recv().await.unwrap()}

    async fn handle_predicate(&mut self, predicate: Self::Predicate) -> Option<Self::Response> {
        let (pending, update) = match self.0.response(&mut *RESOLVER.lock().await, predicate).await {
            Output::Init(pending) => {
                (pending, Update::Init)
            },
            Output::Created(confirmed, pending) => {
                if let Some(mut pending) = self.1.pop_front() {
                    pending.update(confirmed.clone(), true).await;
                }
                (pending, Update::Confirmed(confirmed))
            },
            Output::Read(confirmed, pending) => (pending, Update::Confirmed(confirmed)),
            Output::Subscribed | Output::Garbage => {return None;}
        };
        for (p, u) in pending.into_iter().zip(self.1.iter_mut()) { u.update(p, false).await }
        Some(update)
    }

    async fn handle_request(&mut self, request: Self::Request) -> Self::Response {
        println!("request");
        let pending = Pending::new(self.0.send(request));
        self.1.push_back(pending.clone());
        Update::Pending(pending)
    }

    async fn update(&mut self, memory: &mut Self::Memory) {
        if let Some(request) = self.0.request() {
            *memory = CLIENT.get().unwrap().send(request).await;
        }
    }
}

impl Sharable for Inbox {
    type Init = Secret;
    type Request = (Name, Location);
    type Response = Location;
    type Memory = HashMap<RequestType, AsyncRx<spsc::List<Response>>>;
    type Predicate = (RequestType, Response);

    async fn init(init: Self::Init) -> Self {Inbox::new(init).unwrap()}

    async fn init_memory(&mut self) -> Self::Memory {
        let mut requests = HashMap::new();
        for (ty, request) in self.start() {
            requests.insert(ty, CLIENT.get().unwrap().send(request).await);
        }
        requests
    }

    async fn predicate(&mut self, memory: &mut Self::Memory) -> Self::Predicate {
        if memory.is_empty() { pending().await } else {
            FuturesUnordered::from_iter(memory.iter().map(|(ty, rx)| {
                let fut = rx.recv();
                async move { (*ty, fut.await.unwrap()) }
            })).next().await.unwrap()
        }
    }

    async fn handle_predicate(&mut self, predicate: Self::Predicate) -> Option<Self::Response> {
        self.response(&mut *RESOLVER.lock().await, predicate.0, predicate.1).await.map(|r| r.1)
    }

    async fn handle_request(&mut self, request: Self::Request) -> Self::Response {
        self.send(&mut *RESOLVER.lock().await, request.0, request.1).await;
        request.1
    }

    async fn update(&mut self, memory: &mut Self::Memory) {
        for (ty, req) in self.requests() {
            memory.insert(ty, CLIENT.get().unwrap().send(req).await);
        }
    }
}
