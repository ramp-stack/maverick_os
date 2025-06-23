use std::time::Duration;

use crate::runtime::{self, Service as ThreadService, ThreadContext, Services, ServiceList};
use crate::hardware;

use air::orange_name::OrangeResolver;
use air::server::{Purser, Request as AirRequest};
use air::storage::records::{self, Cache, PathedKey};
use air::storage;
use serde::{Serialize, Deserialize};

pub use air::{DateTime, Id};
pub use air::orange_name::{OrangeName, OrangeSecret};
pub use air::storage::{PublicItem, Filter, Op};
pub use air::storage::records::{RecordPath, Permissions, Protocol, Error, Record, Header, HeaderInfo, ValidationError, Validation, ChildrenValidation};

enum Client {
    Public(Box<storage::Client>),
    Private(Box<records::Client>)
}

impl From<storage::Client> for Client {fn from(c: storage::Client) -> Self {Client::Public(Box::new(c))}}
impl From<records::Client> for Client {fn from(c: records::Client) -> Self {Client::Private(Box::new(c))}}

impl Client {
    pub fn build_request(&self) -> AirRequest {match self {
        Self::Public(client) => client.build_request(),
        Self::Private(client) => client.build_request(),
    }}
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Request {
    CreatePublic(PublicItem),
    ReadPublic(Filter),
    UpdatePublic(Id, PublicItem),
    Discover(RecordPath, u32, Vec<Protocol>),
    CreatePrivate(RecordPath, Protocol, u32, Permissions, Vec<u8>),
    CreatePointer(RecordPath, RecordPath, u32),
    ReadPrivate(RecordPath),
    UpdatePrivate(RecordPath, Permissions, Vec<u8>),
    DeletePrivate(RecordPath),
    Share(OrangeName, Permissions, RecordPath),
    Receive(DateTime)
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Response {
    CreatePublic(Id),
    ReadPublic(Vec<(Id, OrangeName, PublicItem, DateTime)>),
    CreatePrivate(RecordPath, Option<(Result<Record, ValidationError>, DateTime)>),
    ReadPrivate(Option<(Record, DateTime)>),
    UpdatePrivate(bool),
    DeletePrivate(bool),
    Discover(Option<RecordPath>, Option<DateTime>),
    Receive(Vec<(OrangeName, RecordPath)>),
    Empty
}

pub struct Service{
    resolver: OrangeResolver,
    secret: OrangeSecret,
    purser: Purser,
    cache: Cache
}

impl Service {
    pub async fn create_private<
        S: Serialize + for<'a> Deserialize <'a> + Send + 'static,
        R: Serialize + for<'a> Deserialize <'a> + Send + 'static,
    >(ctx: &mut ThreadContext<S, R>, path: RecordPath, protocol: Protocol, index: u32, perms: Permissions, payload: Vec<u8>) -> Result<(RecordPath, Option<(Result<Record, ValidationError>, DateTime)>), Error> {
        match ctx.blocking_request::<Service>(Request::CreatePrivate(path, protocol, index, perms, payload)).await? {
            Response::CreatePrivate(path, result) => Ok((path, result)),
            r => Err(Error::MaliciousResponse(format!("{:?}", r))),
        }
    }

    pub async fn create_public<
        S: Serialize + for<'a> Deserialize <'a> + Send + 'static,
        R: Serialize + for<'a> Deserialize <'a> + Send + 'static,
    >(ctx: &mut ThreadContext<S, R>, item: PublicItem) -> Result<Id, Error> {
        match ctx.blocking_request::<Service>(Request::CreatePublic(item)).await? {
            Response::CreatePublic(id) => Ok(id),
            r => Err(Error::MaliciousResponse(format!("{:?}", r))),
        }
    }

    pub async fn read_public<
        S: Serialize + for<'a> Deserialize <'a> + Send + 'static,
        R: Serialize + for<'a> Deserialize <'a> + Send + 'static,
    >(ctx: &mut ThreadContext<S, R>, filter: Filter) -> Result<Vec<(Id, OrangeName, PublicItem, DateTime)>, Error> {
        match ctx.blocking_request::<Service>(Request::ReadPublic(filter)).await? {
            Response::ReadPublic(results) => Ok(results),
            r => Err(Error::MaliciousResponse(format!("{:?}", r))),
        }
    }

    pub async fn discover<
        S: Serialize + for<'a> Deserialize <'a> + Send + 'static,
        R: Serialize + for<'a> Deserialize <'a> + Send + 'static,
    >(ctx: &mut ThreadContext<S, R>, path: RecordPath, index: u32, protocols: Vec<Protocol>) -> Result<(Option<RecordPath>, Option<DateTime>), Error> {
        match ctx.blocking_request::<Service>(Request::Discover(path, index, protocols)).await? {
            Response::Discover(result, date) => Ok((result, date)),
            r => Err(Error::MaliciousResponse(format!("{:?}", r)))
        }
    }
}

#[async_trait::async_trait]
impl ThreadService for Service {
    type Send = Result<Response, Error>;
    type Receive = Request;

    async fn new(hardware: &mut hardware::Context) -> Self {
        //TODO: check cloud too
        let secret = if let Some(s) = hardware.cache.get("OrangeSecret").await {s} else {
            let sec = OrangeSecret::new();
            hardware.cache.set("OrangeSecret", &sec.clone()).await;
            sec
        };
        hardware.cache.set("OrangeName", &Some(secret.name())).await;
        let cache = match hardware.cache.get("Cache").await {
            Some(cache) => cache,
            None => {
                Cache::new(PathedKey::new(RecordPath::root(), OrangeResolver.secret_key(&secret, None, None).await.unwrap()))
            }
        };
        Service{
            resolver: OrangeResolver,
            secret,
            purser: Purser::new(),
            cache
        }
    }

    ///This service does not have any repeating tasks and accepts no messages from main
    async fn run(&mut self, ctx: &mut ThreadContext<Self::Send, Self::Receive>) -> Result<Option<Duration>, runtime::Error> {
        let mut clients = Vec::new();
        let mut requests = Vec::new();

        while let Some((id, request)) = ctx.get_request() {
            let client: Client = match request {
                Request::CreatePublic(item) => storage::Client::create_public(&mut self.resolver, &self.secret, item).await?.into(),
                Request::ReadPublic(filter) => storage::Client::read_public(filter).into(),
                Request::UpdatePublic(id, item) => storage::Client::update_public(&mut self.resolver, &self.secret, id, item).await?.into(),
                Request::Discover(path, index, protocols) => records::Client::discover(&mut self.cache, &path, index, protocols)?.into(),
                Request::CreatePrivate(parent, protocol, index, perms, payload) => records::Client::create(&mut self.cache, &parent, protocol, index, &perms, payload)?.into(),
                Request::CreatePointer(parent, path, index) => records::Client::create_pointer(&mut self.cache, &parent, &path, index)?.into(),
                Request::ReadPrivate(path) => records::Client::read(&mut self.cache, &path)?.into(),
                Request::UpdatePrivate(path, perms, payload) => records::Client::update(&mut self.cache, &path, &perms, payload)?.into(),
                Request::DeletePrivate(path) => records::Client::delete(&mut self.cache, &path)?.into(),
                Request::Share(name, perms, path) => records::Client::share(&mut self.cache, &mut self.resolver, &self.secret, &name, &perms, &path).await?.into(),
                Request::Receive(since) => records::Client::receive(&mut self.resolver, &self.secret, since).await?.into(),
            };
            requests.push(client.build_request());
            clients.push((client, id));
        }
        let batch = AirRequest::batch(requests);
        let endpoint = self.resolver.endpoint(&self.secret.name(), None, None).await?;
        let res = self.purser.send(&mut self.resolver, &endpoint, batch).await?;
        for (response, (client, id)) in res.batch()?.into_iter().zip(clients) {
            ctx.respond(id, match client {
                Client::Public(client) => match client.process_response(&mut self.resolver, response).await {
                    Ok(storage::Processed::CreatePublic(id)) => Ok(Response::CreatePublic(id)),
                    Ok(storage::Processed::ReadPublic(results)) => Ok(Response::ReadPublic(results)),
                    Ok(storage::Processed::Empty) => Ok(Response::Empty),
                    Ok(r) => Err(Error::MaliciousResponse(format!("{:?}", r))),
                    Err(e) => Err(e.into())
                },
                Client::Private(client) => client.process_response(&mut self.cache, &mut self.resolver, response).await.map(|r| match r {
                    records::Processed::Discover(record, date) => Response::Discover(record, date),
                    records::Processed::Create(path, conflict) => Response::CreatePrivate(path, conflict),
                    records::Processed::Read(record) => Response::ReadPrivate(record),
                    records::Processed::Update(s) => Response::UpdatePrivate(s),
                    records::Processed::Delete(s) => Response::DeletePrivate(s),
                    records::Processed::Receive(records) => Response::Receive(records),
                    records::Processed::Empty => Response::Empty,
                }),
            })
        }
        ctx.hardware.cache.set("Cache", &Some(self.cache.clone())).await;
        Ok(Some(Duration::from_millis(100)))
    }
}

impl Services for Service {
    fn services() -> ServiceList {ServiceList::default()}
}
