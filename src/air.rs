use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arc_swap::ArcSwap;

pub use air::contract::{Contract, Contracts, Substance, Reactants, Reactant, from, into, Error, Beaker, RequestBuilder, Request};
pub use air::names::{Name, Id};

use air::names::Secret;
use air::contract::Manager;

use serde::{Serialize, Deserialize};

use crossfire::{MTx, Rx, mpsc::{Array, bounded_blocking}};

use rusqlite::{OptionalExtension, Connection};

use crate::hardware;


#[derive(Debug, Clone)]
pub struct Context {
    builder: RequestBuilder,
    value: Arc<ArcSwap<BTreeMap<Id, BTreeMap<Id, Substance>>>>,
    tx: MTx<Array<Request>>,
}
impl Context {
    pub fn name(&self) -> Name {self.builder.name()}
    pub fn builder(&self) -> &RequestBuilder {&self.builder}
    pub fn request(&self, request: Request) {
        let _ = self.tx.send(request);
    }

    pub fn query(&self, id: &Id, iid: &Id, path: PathBuf) -> Option<Substance> {
        self.value.load().get(id)?.get(iid)?.query(path).ok()
    }

    pub fn get<C: Contract>(&self, iid: &Id) -> Option<Substance> {
        Some(self.value.load().get(&C::id())?.get(iid)?.clone())
    }

    pub fn list(&self, c_id: &Id) -> Vec<Id> {
        match self.value.load().get(c_id) {
            Some(instances) => instances.keys().map(|k| *k).collect(),
            None => vec![]
        }
    }

    pub fn create<C: Contract>(&self, contract: C) -> Result<Id, Error> {
        let (id, request) = self.builder.create(contract)?;
        let _ = self.tx.send(request);
        Ok(id)
    }

    pub fn share<C: Contract>(&self, iid: Id, name: Name) -> Result<(), Error> {
        let request = self.builder.share::<C>(iid, name)?;
        let _ = self.tx.send(request);
        Ok(())
    }

    pub fn send<P: AsRef<Path>, R: Reactant + 'static>(&self, id: Id, path: P, reactant: R) -> Result<Result<(), R::Error>, Error> {
        let request = self.builder.send(id, path, reactant)?;
        let _ = self.tx.send(request); 
        Ok(Ok(()))
    }
}

pub(crate) struct Air {
    cache: Connection,
    manager: Manager,
    value: Arc<ArcSwap<BTreeMap<Id, BTreeMap<Id, Substance>>>>,
    rx: Rx<Array<Request>>
}

impl Air {
    pub fn start(_hardware: &hardware::Context, contracts: Contracts) -> Result<(Context, Self), rusqlite::Error> {
      //let secret = hardware.cloud().get("secret").and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_else(|| {
      //    let secret = Secret::new();
      //    hardware.cloud().save("secret", &serde_json::to_string(&secret).unwrap());
      //    secret
      //});
        let cache = Connection::open("./air_cache.db")?;
        init(&cache)?;
        let mut manager = get(&cache, "manager")?.unwrap_or_else(|| Manager::new(Secret::new()));
        //let mut manager = Manager::new(Secret::new());
        manager.init(contracts);
        let builder = manager.request_builder();

        let value = Arc::new(ArcSwap::from(Arc::new(manager.get())));

        let (tx, rx) = bounded_blocking(100);

        Ok((Context{builder, value: value.clone(), tx}, Air{
            cache,
            manager,
            value,
            rx,
        }))
    }

    pub async fn run(mut self) {
        loop {
            //let mut request = Vec::new();
            //while let Ok(r) = self.rx.try_recv() {request.push(r);}
            self.manager.tick(self.rx.try_recv().ok()).await;
            self.value.store(Arc::new(self.manager.get()));
            insert(&self.cache, "manager", &self.manager).unwrap();
        }
    }
}


fn init(connection: &Connection) -> Result<(), rusqlite::Error> {
    connection.execute("CREATE TABLE if not exists Cache(
        key TEXT NOT NULL PRIMARY KEY,
        value BLOB NOT NULL
    );", [])?;
    Ok(())
}
fn get<T: for<'a> Deserialize<'a>>(connection: &Connection, key: &str) -> Result<Option<T>, rusqlite::Error> {
    Ok(connection.query_row(
        &format!("SELECT value FROM Cache WHERE key='{key}'"),
        [], |r| Ok(serde_json::from_slice(&r.get::<_, Vec<u8>>(0)?).ok()),
    ).optional()?.flatten())
}

fn insert<T: Serialize>(connection: &Connection, key: &str, value: &T) -> Result<(), rusqlite::Error> {
    connection.execute(
        &format!("INSERT INTO Cache(key, value) VALUES ('{key}', ?1) ON CONFLICT DO UPDATE SET value=excluded.value;"),
        [serde_json::to_vec(value).unwrap()],
    )?;
    Ok(())
}
