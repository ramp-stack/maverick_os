use std::sync::Arc;
use std::time::Duration;
use std::fmt::Debug;

use serde::{Serialize, Deserialize};

use crate::ApplicationSupport;
pub use rusqlite::Connection;

use std::future::Future;
use active_rusqlite::{ActiveRecord, ActiveRusqlite};
use rusqlite::Error;

#[cfg(target_os = "android")]
use winit::platform::android::activity::AndroidApp;

#[cfg(not(target_arch = "wasm32"))]
use tokio::sync::Mutex;

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone)]
pub struct Cache(
    Arc<Mutex<rusqlite::Connection>>
);

#[cfg(not(target_arch = "wasm32"))]
impl Cache {
    pub(crate) fn new() -> Self {
        let storage_path = ApplicationSupport::get().unwrap(); 
        std::fs::create_dir_all(&storage_path).unwrap();
        let path = storage_path.join("cache.db");
        // if path.exists() { std::fs::remove_file(&path).expect("Failed to delete file"); }   
        let mut db = rusqlite::Connection::open(&path).unwrap();
        db.busy_timeout(Duration::ZERO).unwrap();
        db.execute(
            "CREATE TABLE if not exists kvs(key TEXT NOT NULL UNIQUE, value TEXT);", []
        ).unwrap();
        if db.get::<Option<String>>("v3").is_none() {
            drop(db);
            std::fs::remove_file(&path).expect("Failed to delete file");
            db = rusqlite::Connection::open(path).unwrap();
            db.execute(
                "CREATE TABLE if not exists kvs(key TEXT NOT NULL UNIQUE, value TEXT);", []
            ).unwrap();
            db.set("v3", &"".to_string());
        }
        Cache(Arc::new(Mutex::new(db)))
    }

    pub async fn set<
        V: Serialize + for<'a> Deserialize <'a> + Default,
    >(&self, key: &str, value: &V) {
        self.0.lock().await.set(key, value)
    }

    pub async fn get<
        V: Serialize + for<'a> Deserialize <'a> + Default,
    >(&self, key: &str) -> V {
        self.0.lock().await.get(key)
    }

    pub async fn lock<T>(&mut self, callback: impl FnOnce(&Connection) -> T) -> Result<T, rusqlite::Error> {
        let mut guard = self.0.lock().await;
        let tx = guard.transaction_with_behavior(rusqlite::TransactionBehavior::Exclusive).unwrap();
        let result = callback(&tx);
        tx.commit()?;
        Ok(result)
    }
}

pub trait RustSqlite {
    fn set<V: Serialize + for<'a> Deserialize <'a> + Default>(&self, key: &str, value: &V);
    fn get<V: Serialize + for<'a> Deserialize <'a> + Default>(&self, key: &str) -> V;
}

impl RustSqlite for Connection {
    fn set<V: Serialize + for<'a> Deserialize <'a> + Default>(&self, key: &str, value: &V) {
        self.execute(
            "INSERT INTO kvs(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value;",
            [key, &hex::encode(serde_json::to_vec(value).unwrap())]
        ).unwrap();
    }

    fn get<V: Serialize + for<'a> Deserialize <'a> + Default>(&self, key: &str) -> V {
        self.prepare(
            &format!("SELECT value FROM kvs where key = \'{}\'", key),
        ).unwrap().query_and_then([], |row| {
            let item: String = row.get(0).unwrap();
            Ok(hex::decode(item).unwrap())
        }).unwrap().collect::<Result<Vec<Vec<u8>>, rusqlite::Error>>().unwrap()
        .first().and_then(|b| serde_json::from_slice(b).ok()).unwrap_or_default()
    }
}

pub trait ActiveCache {
    fn create(&self, cache: &mut Cache) -> impl Future<Output = Result<(), Error>>;
    fn read(cache: &mut Cache) -> impl Future<Output = Result<Option<Self>, Error>> where Self: Sized;
    fn update(&mut self, cache: &mut Cache) -> impl Future<Output = Result<(), Error>>;
    fn delete(cache: &mut Cache) -> impl Future<Output = Result<(), Error>>;

    fn read_or(cache: &mut Cache, or: impl FnOnce() -> Self) -> impl Future<Output = Result<Self, Error>> where Self: Sized;

    fn create_sub<T: ActiveRecord>(cache: &mut Cache, path: &[&str], record: &T) -> impl Future<Output = Result<(), Error>>;
    fn read_sub<T: ActiveRecord>(cache: &mut Cache, path: &[&str]) -> impl Future<Output = Result<Option<T>, Error>>;
    fn update_sub<T: ActiveRecord>(cache: &mut Cache,  path: &[&str], record: &mut T) -> impl Future<Output = Result<(), Error>>;
    fn delete_sub(cache: &mut Cache, path: &[&str]) -> impl Future<Output = Result<(), Error>>;
}

impl<A: ActiveRusqlite> ActiveCache for A {
    async fn create(&self, cache: &mut Cache) -> Result<(), Error> {
        cache.lock(|c: &Connection| self.create(c)).await?
    }
    async fn read(cache: &mut Cache) -> Result<Option<Self>, Error> {
        cache.lock(|c: &Connection| Self::read(c)).await?
    }
    async fn update(&mut self, cache: &mut Cache) -> Result<(), Error> {
        cache.lock(|c: &Connection| self.update(c)).await?
    }
    async fn delete(cache: &mut Cache) -> Result<(), Error> {
        cache.lock(|c: &Connection| Self::delete(c)).await?
    }

    async fn read_or(cache: &mut Cache, or: impl FnOnce() -> Self) -> Result<Self, Error> {
        cache.lock(|c: &Connection| Self::read(c).transpose().unwrap_or_else(|| {
            let t = or();
            t.create(c)?;
            Ok(t)
        })).await?
    }

    async fn create_sub<T: ActiveRecord>(cache: &mut Cache, path: &[&str], record: &T) -> Result<(), Error> {
        cache.lock(|c: &Connection| Self::create_sub(c, path, record)).await?
    }
    async fn read_sub<T: ActiveRecord>(cache: &mut Cache, path: &[&str]) -> Result<Option<T>, Error> {
        cache.lock(|c: &Connection| Self::read_sub(c, path)).await?
    }
    async fn update_sub<T: ActiveRecord>(cache: &mut Cache,  path: &[&str], record: &mut T) -> Result<(), Error> {
        cache.lock(|c: &Connection| Self::update_sub(c, path, record)).await?
    }
    async fn delete_sub(cache: &mut Cache, path: &[&str]) -> Result<(), Error> {
        cache.lock(|c: &Connection| Self::delete_sub(c, path)).await?
    }
}
