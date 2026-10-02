pub use air::{Secret, Name, Id};
pub use air::contract::{Contract, Metadata};


use air::channel::{Channel};
use air::contract::{Location, Instance as _Instance};
use air::names::{DefaultResolver, Resolver, Identity};
use air::inbox::{Inbox, RequestType};
use air::storage::Response;
use air::client::Client;


use crate::runtime::Runtime;
use crate::shared::{Shared, Sharable, Receiver, Responder, Ref};

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::future::pending;
use std::sync::OnceLock;
use std::sync::Arc;
use std::any::Any;

use crossfire::{AsyncRx, spsc};
use serde::{Deserialize, Serialize};
use futures::stream::{FuturesUnordered, StreamExt};

static CLIENT: OnceLock<Client> = OnceLock::new();
static RESOLVER: OnceLock<MResolver> = OnceLock::new();

#[derive(Clone)]
pub struct Context{
    runtime: Runtime,
    secret: Secret,
    manager: Shared<Manager>,
}
impl Context {
    pub fn new(runtime: Runtime, secret: Secret) -> Self {
        let resolver = MResolver::new(runtime.clone());
        let client = runtime.block_on(Client::new(resolver.clone()));
        let _ = CLIENT.set(client);
        let _ = RESOLVER.set(resolver);
        let manager = Shared::new(runtime.clone(), secret.clone());
        Context{runtime, secret, manager}
    }

    pub fn create<C: Contract>(&self, init: C::Init) -> Instance<C> {
        let location = _Instance::<C>::generate_location(&self.secret, &init).unwrap();
        let secret = self.secret.clone();
        let runtime = self.runtime.clone();
        let manager = self.manager.clone();
        let _secret = self.secret.clone();
        let _runtime = self.runtime.clone();
        let _manager = self.manager.clone();
        if let Instances::Single(single) = self.manager.request(Request::Create(C::id(), Id::hash(&location),
            Box::new(move || (Box::new(Instance::<C>(Shared::new(_runtime, InstanceInit{
                secret: _secret,
                location,
                init: Some(init)
            }), _manager)) as _, location)),
            Box::new(move |location| Box::new(Instance::<C>(Shared::new(runtime.clone(),
                InstanceInit{secret: secret.clone(), location, init: None}
            ), manager.clone())) as _)
        )).unwrap() {single.downcast_ref::<Instance<C>>().unwrap().clone()} else {panic!("-_-");}
    }

    pub fn list<C: Contract>(&self) -> HashMap<Id, Instance<C>> {
        let secret = self.secret.clone();
        let runtime = self.runtime.clone();
        let manager = self.manager.clone();
        if let Instances::Map(map) = self.manager.request(Request::List(C::id(),
            Box::new(move |location| Box::new(Instance::<C>(Shared::new(runtime.clone(),
                InstanceInit{secret: secret.clone(), location, init: None}
            ), manager.clone())) as _)
        )).unwrap() {map.into_iter().map(|(id, bass)| (id, bass.downcast_ref::<Instance<C>>().unwrap().clone())).collect()} else {panic!("-_-");}
    }
}


#[derive(Clone, Debug)]
pub struct Instance<C: Contract>(Shared<_Instance<C>>, Shared<Manager>);
impl<C: Contract> Instance<C> {
    pub fn send(&mut self, msg: C::Message) -> C::Result {self.0.request(msg)}
    pub fn share(&self, name: Name) {self.1.request(Request::Share(name, *self.0.as_ref().location()));}
    pub fn pending(&self) -> Ref<'_, C> {self.0.as_ref().map(|i| i.pending())}
    pub fn confirmed(&self) -> Option<Ref<'_, C>> {
        let r = self.0.as_ref();
        r.confirmed().is_some().then(|| r.map(|i| i.confirmed().unwrap()))
    }
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
struct Identities(HashMap<Name, Arc<Identity>>);
impl Sharable for Identities {
    type Init = ();
    type Memory = DefaultResolver;
    type Request = (Name, Option<u64>);
    type Response = Arc<Identity>;

    async fn init(init: Self::Init) -> Self {
        Identities::default()
    }

    async fn init_memory(&mut self) -> Self::Memory {
        DefaultResolver::start()
    }

    async fn run(&mut self, memory: &mut Self::Memory, receiver: &mut Receiver<Self>) -> bool {
        let ((name, time), responder) = receiver.receive().await;
        let identity = memory.resolve(name, time).await;
        responder.respond(identity.clone());
        self.0.insert(name, identity);
        true
    }
}

#[derive(Clone, Debug)]
pub(crate) struct MResolver(Shared<Identities>);
impl MResolver {
    pub fn new(runtime: Runtime) -> Self {
        MResolver(Shared::new(runtime, ()))
    }
}
impl Resolver for MResolver {
    async fn resolve(&mut self, name: Name, time: Option<u64>) -> Arc<Identity> {
        self.0.as_ref().0.get(&name).cloned().unwrap_or_else(|| {
            self.0.request((name, time))
        })
    }
}

pub(crate) struct InstanceInit<I> {
    secret: Secret,
    location: Location,
    init: Option<I>
}
impl<I> Hash for InstanceInit<I> {fn hash<H: Hasher>(&self, hasher: &mut H) {self.secret.hash(hasher); self.location.hash(hasher);}}

impl<C: Contract> Sharable for _Instance<C> {
    type Init = InstanceInit<C::Init>;
    type Memory = (MResolver, AsyncRx<spsc::List<Response>>);
    type Request = C::Message;
    type Response = C::Result;

    async fn init(sinit: Self::Init) -> Self {
        match sinit.init {
            Some(init) => _Instance::new(sinit.secret, init).unwrap(),
            None => _Instance::receive(sinit.secret, sinit.location).unwrap(),
        }
    }

    async fn init_memory(&mut self) -> Self::Memory {
        let request = self.start();
        (RESOLVER.get().unwrap().clone(), CLIENT.get().unwrap().send(request).await)
    }

    async fn run(&mut self, memory: &mut Self::Memory, receiver: &mut Receiver<Self>) -> bool {
        tokio::select!{
            biased;
            response = memory.1.recv() => {
                self.response(&mut memory.0, response.unwrap()).await;
            },
            (message, responder) = receiver.receive() => {
                responder.respond(self.send(message));
                
            }
        }
        if let Some(request) = self.request() {
            memory.1 = CLIENT.get().unwrap().send(request).await;
        }
        true
    }
}

type Receive = Box<dyn Fn(Location) -> Bass + Send + Sync> ;
type Create = Box<dyn FnOnce() -> (Bass, Location) + Send + Sync>;
type Bass = Box<dyn Any + Send + Sync>;

enum Request {
    Share(Name, Location),
    Create(Id, Id, Create, Receive),
    List(Id, Receive)
}

struct ManagerMemory {
    requests: HashMap<RequestType, AsyncRx<spsc::List<Response>>>,
    resolver: MResolver,
    constructors: HashMap<Id, Receive>,
    instances: HashMap<Id, HashMap<Id, Arc<Bass>>>
}
impl ManagerMemory {
    async fn poll(&mut self) -> (RequestType, Response) {
        if self.requests.is_empty() {
            pending().await
        } else {
            let mut waiting = FuturesUnordered::new();
            for (ty, rx) in &self.requests {
                let fut = rx.recv();
                waiting.push(async move { (*ty, fut.await.unwrap()) });
            }
            waiting.next().await.unwrap()
        }
    }

    fn receiver(&mut self, inbox: &Inbox, contract: Id, receive: Receive) {
        if !self.constructors.contains_key(&contract) {
            self.instances.insert(contract, inbox.list(&contract).into_iter().map(|loc| (loc.instance, Arc::new((receive)(loc)))).collect());
            self.constructors.insert(contract, receive);
        }
    }

    fn receive(&mut self, location: Location) -> Option<Arc<Bass>> {
        let mut instances = self.instances.entry(location.contract).or_default();
        self.constructors.get(&location.contract).map(|constructor|
            instances.entry(location.instance).or_insert_with(|| Arc::new((constructor)(location))).clone()
        )
    }

    fn create(&mut self, contract: Id, instance: Id, create: Create) -> (Arc<Bass>, Option<Location>) {
        let instances = self.instances.entry(contract).or_default();

        let loc = if let std::collections::hash_map::Entry::Vacant(e) = instances.entry(instance) {
            let (bass, loc) = (create)();
            e.insert(Arc::new(bass)); Some(loc)
        } else { None };
        (instances.get(&instance).unwrap().clone(), loc)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct Manager(Inbox, Secret);

enum Instances { Single(Arc<Bass>), Map(HashMap<Id, Arc<Bass>>)}

impl Sharable for Manager {
    type Init = Secret;
    type Memory = ManagerMemory;
    type Request = Request;
    type Response = Option<Instances>; 

    async fn init(init: Self::Init) -> Self {
        Manager(Inbox::new(init.clone()).unwrap(), init)
    }

    async fn init_memory(&mut self) -> Self::Memory {
        let mut requests = HashMap::new();
        for (ty, request) in self.0.start() {
            requests.insert(ty, CLIENT.get().unwrap().send(request).await);
        }
        let resolver = RESOLVER.get().unwrap().clone();
        ManagerMemory{requests, resolver, constructors: HashMap::new(), instances: HashMap::new()}
    }

    async fn run(&mut self, memory: &mut Self::Memory, receiver: &mut Receiver<Self>) -> bool {
        tokio::select!{
            biased;
            (ty, response) = memory.poll() => {
                if let Some((_name, location)) = self.0.response(&mut memory.resolver, ty, response).await {
                    memory.receive(location);
                }
            },
            (request, responder) = receiver.receive() => match request {
                Request::Create(contract, instance, create, receive) => {
                    memory.receiver(&self.0, contract, receive);
                    let (bass, location) = memory.create(contract, instance, create);
                    if let Some(location) = location {
                        self.0.send(&mut memory.resolver, self.1.name(), location).await;
                    }
                    responder.respond(Some(Instances::Single(bass)));
                },
                Request::Share(name, location) => {
                    self.0.send(&mut memory.resolver, name, location).await;
                },
                Request::List(contract, receive) => {
                    memory.receiver(&self.0, contract, receive);
                    responder.respond(Some(Instances::Map(memory.instances.get(&contract).unwrap().clone())));
                }
            }
        }
        for (ty, req) in self.0.requests() {
            memory.requests.insert(ty, CLIENT.get().unwrap().send(req).await);
        }
        true
    }
}
