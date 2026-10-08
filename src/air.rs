pub use air::{Secret, Name, Id};
pub use air::contract::{Contract, Metadata};

use air::contract::{Location, Instance as _Instance, Output};
use air::names::DefaultResolver;
use air::inbox::{Inbox, RequestType};
use air::storage::Response;
use air::client::Client;

use crate::runtime::RUNTIME;
use crate::shared::{Shared, Sharable, Ref, Service, Services, Pending};

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
pub struct Instance<C: Contract>(Shared<_Instance<C>>, Shared<Inbox>);
impl<C: Contract> Instance<C> {
    pub fn send(&mut self, msg: C::Message) -> Pending<C::Result> {
        if let Update::Pending(p) = self.0.request(msg) {p} else {panic!("-_-");}
    }
    pub fn share(&self, name: Name) {self.1.request((name, *self.0.as_ref().location()));}
    pub fn pending(&self) -> Ref<'_, C> {self.0.as_ref().map(|i| i.pending())}
    pub fn confirmed(&self) -> Option<Ref<'_, C>> {
        let r = self.0.as_ref();
        r.confirmed().is_some().then(|| r.map(|i| i.confirmed().unwrap()))
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(bound = "C: Contract")]
pub struct _Instances<C: Contract>(#[serde(skip)] HashMap<Id, Instance<C>>, Secret);
impl<C: Contract> Sharable for _Instances<C> {
    type Init = Secret;
    type Request = C::Init;
    type Response = Instance<C>;

    fn services() -> Services<Self> {Services::default().add::<InstancesRequest>()}

    async fn init(init: Self::Init) -> Self {Self(HashMap::new(), init)}

    async fn handle(&mut self, request: Self::Request) -> Self::Response {
        let location = _Instance::<C>::generate_location(&self.1, &request).unwrap();
        self.0.entry(Id::hash(&location)).or_insert_with(||
            Instance(Shared::new(InstanceInit{secret: self.1.clone(), location, init: Some(request)}), Shared::new(self.1.clone()))
        ).clone()
    }
}


pub struct InstancesRequest(Shared<Inbox>);
impl<C: Contract> Service<_Instances<C>> for InstancesRequest {
    type Predicate = Location;

    async fn init(instances: &mut _Instances<C>) -> Self {Self(Shared::new(instances.1.clone()))}

    async fn predicate(&mut self) -> Self::Predicate {loop {
        let location = self.0.next().await;
        if location.contract == C::id() {
            break location;
        }
    }}

    async fn run(&mut self, instances: &mut _Instances<C>, location: Self::Predicate) -> Option<Instance<C>> {
        let hash = Id::hash(&location);
        if !instances.0.contains_key(&hash) {
            let instance = Instance(Shared::new(InstanceInit{secret: instances.1.clone(), location, init: None}), Shared::new(instances.1.clone()));
            instances.0.insert(hash, instance.clone());
            Some(instance)
        } else {None}
    }

    async fn update(&mut self, _instances: &mut _Instances<C>, _update: Option<Instance<C>>) {}
}

pub struct InstanceInit<I> {secret: Secret, location: Location, init: Option<I> }
impl<I> Hash for InstanceInit<I> {fn hash<H: Hasher>(&self, hasher: &mut H) {self.secret.hash(hasher); self.location.hash(hasher);}}

#[derive(Debug, Clone)]
pub enum Update<R: Clone + Send + Sync + 'static> {Init, Pending(Pending<R>), Confirmed(R)}

impl<C: Contract> Sharable for _Instance<C> {
    type Init = InstanceInit<C::Init>;
    type Request = C::Message;
    type Response = Update<C::Result>;

    fn services() -> Services<Self> {Services::default().add::<InstanceRequest<C>>()}

    async fn init(sinit: Self::Init) -> Self {
        match sinit.init {
            Some(init) => _Instance::new(sinit.secret, init).unwrap(),
            None => _Instance::receive(sinit.secret, sinit.location).unwrap()
        }
    }

    async fn handle(&mut self, request: Self::Request) -> Self::Response {
        Update::Pending(Pending::new(self.send(request)))
    }
}

struct InstanceRequest<C: Contract>(AsyncRx<spsc::List<Response>>, VecDeque<Pending<C::Result>>);
impl<C: Contract> Service<_Instance<C>> for InstanceRequest<C> {
    type Predicate = Response;

    async fn init(instance: &mut _Instance<C>) -> Self {
        Self(CLIENT.get().unwrap().send(instance.start()).await, VecDeque::new())
    }

    async fn predicate(&mut self) -> Self::Predicate {self.0.recv().await.unwrap()}

    async fn run(&mut self, instance: &mut _Instance<C>, predicate: Self::Predicate) -> Option<Update<C::Result>> {
        let (pending, update) = match instance.response(&mut *RESOLVER.lock().await, predicate).await {
            Output::Init(pending) => {
                (pending, Update::Init)
            },
            Output::Created(confirmed, pending) | Output::Read(confirmed, pending) => {
                self.1.pop_front().unwrap().update(confirmed.clone(), true).await;
                (pending, Update::Confirmed(confirmed))
            },
            Output::Subscribed | Output::Garbage => {return None;}
        };
        for (p, u) in pending.into_iter().zip(self.1.iter_mut()) { u.update(p, false).await }
        Some(update)
    }

    async fn update(&mut self, instance: &mut _Instance<C>, update: Option<Update<C::Result>>) {
        if let Some(Update::Pending(pending)) = update { self.1.push_back(pending); }
        if let Some(request) = instance.request() {
            self.0 = CLIENT.get().unwrap().send(request).await;
        }
    }
}

impl Sharable for Inbox {
    type Init = Secret;
    type Request = (Name, Location);
    type Response = Location;

    fn services() -> Services<Self> {Services::default().add::<InboxRequests>()}

    async fn init(init: Self::Init) -> Self {Inbox::new(init).unwrap()}

    async fn handle(&mut self, request: Self::Request) -> Self::Response {
        self.send(&mut *RESOLVER.lock().await, request.0, request.1).await;
        request.1
    }
}

struct InboxRequests(HashMap<RequestType, AsyncRx<spsc::List<Response>>>);
impl Service<Inbox> for InboxRequests {
    type Predicate = (RequestType, Response);

    async fn init(inbox: &mut Inbox) -> Self {
        let mut requests = HashMap::new();
        for (ty, request) in inbox.start() {
            requests.insert(ty, CLIENT.get().unwrap().send(request).await);
        }
        InboxRequests(requests)
    }

    async fn predicate(&mut self) -> Self::Predicate {
        if self.0.is_empty() { pending().await } else {
            FuturesUnordered::from_iter(self.0.iter().map(|(ty, rx)| {
                let fut = rx.recv();
                async move { (*ty, fut.await.unwrap()) }
            })).next().await.unwrap()
        }
    }

    async fn run(&mut self, inbox: &mut Inbox, predicate: Self::Predicate) -> Option<Location> {
        inbox.response(&mut *RESOLVER.lock().await, predicate.0, predicate.1).await.map(|r| r.1)
    }

    async fn update(&mut self, inbox: &mut Inbox, _update: Option<Location>) {
        for (ty, req) in inbox.requests() {
            self.0.insert(ty, CLIENT.get().unwrap().send(req).await);
        }
    }
}










//  #[derive(Serialize, Deserialize, Default, Debug, Clone)]
//  struct Identities(HashMap<Name, Arc<Identity>>, DefaultResolver);
//  impl Sharable for DefaultResolver {
//      type Init = ();
//      type Request = (Name, Option<u64>);
//      type Response = Arc<Identity>;

//      fn services() -> Services<Self> {Services::default()}

//      async fn init(init: Self::Init) -> Self {Self::default()}
//      async fn handle(&mut self, request: Self::Request) -> Self::Response {
//          let identity = self.1.resolve(request.0, request.1).await;
//          self.0.insert(request.0, identity.clone());
//          identity
//      }
//  }

//  #[derive(Clone, Debug)]
//  pub(crate) struct MResolver(Shared<Identities>);
//  impl MResolver {pub fn new() -> Self {MResolver(Shared::new(()))}}
//  impl Resolver for MResolver {
//      async fn resolve(&mut self, name: Name, time: Option<u64>) -> Arc<Identity> {
//          self.0.as_ref().0.get(&name).cloned().unwrap_or_else(|| self.0.request((name, time)))
//      }
//  }




//  type Receive = Box<dyn Fn(Location) -> Bass + Send + Sync>;
//  type Create = Box<dyn FnOnce() -> (Bass, Location) + Send + Sync>;
//  type Bass = Box<dyn Any + Send + Sync>;

//  enum Request { Create(Id, Id, Create), Register(Id, Receive) }

//  #[derive(Serialize, Deserialize, Clone, Debug)]
//  struct Manager{
//      inbox: Shared<Inbox>, 
//      secret: Secret,
//      constructors: HashMap<Id, Receive>,
//      pub instances: HashMap<Id, HashMap<Id, Arc<Bass>>>,
//  }
//  impl Manager {
//      fn register(&mut self, contract: Id, receive: Receive) {
//          if !self.constructors.contains_key(&contract) {
//              self.instances.insert(contract, self.inbox.as_ref().list(&contract).into_iter().map(|loc| (loc.instance, Arc::new((receive)(loc)))).collect());
//              self.constructors.insert(contract, receive);
//          }
//      }

//      fn receive(&mut self, location: Location) -> Arc<Bass> {
//          let mut instances = self.instances.entry(location.contract).or_default();
//          self.constructors.get(&location.contract).map(|constructor|
//              instances.entry(location.instance).or_insert_with(|| Arc::new((constructor)(location))).clone()
//          )
//      }
//  }
//  impl Sharable for Manager {
//      type Init = Secret;
//      type Request = Request;
//      type Response = Arc<Bass>; 

//      fn services() -> Service<Self> {Service::default().add::<ManagerRequest>()}

//      async fn init(init: Self::Init) -> Self {
//          Manager{
//              inbox: INBOX.get().unwrap().clone(),
//              secret: init,
//              constructors: HashMap::new(),
//              instances: HashMap::new()
//          }
//      }

//      async fn handle(&mut self, request: Self::Request) -> Self::Response {
//          match request {
//              Request::Create(contract, instance, create) => {
//                  let instances = self.instances.entry(contract).or_default();
//                  let loc = if let std::collections::hash_map::Entry::Vacant(e) = instances.entry(instance) {
//                      let (bass, loc) = (create)();
//                      e.insert(Arc::new(bass)); Some(loc)
//                  } else { None };
//                  let bass = instances.get(&instance).unwrap().clone();
//                  if let Some(location) = location {
//                      self.share.entry(self.secret.name()).or_default().insert(location);
//                  }
//                  Some(bass)
//              },
//              Request::Register(contract, receive) => {
//                  self.register(contract, receive);
//                  None
//              }
//          }
//      }
//  }

//  struct ManagerRequest(Shared<Inbox>);
//  impl Service<Manager> for ManagerRequest {
//      type Predicate = Location;
//      async fn init(inbox: &mut Inbox) -> Self {Self(INBOX.get().unwrap().clone())}
//      async fn predicate(&mut self) -> Self::Predicate {self.0.next().await}
//      async fn run(&mut self, manager: &mut Manager, predicate: Self::Predicate) -> Inbox::Response {manager.receive(predicate)}
//      async fn update(&mut self, manager: &mut Manager, update: Manager::Response) {}
//  }
