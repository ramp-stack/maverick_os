use std::sync::Arc;
use std::path::PathBuf;
use std::time::Duration;
use std::fmt::Debug;
use std::ops::Add;

use serde::{Serialize, Deserialize};

use crate::ApplicationSupport;
use rusqlite::Connection;

#[cfg(target_os = "android")]
use winit::platform::android::activity::AndroidApp;

#[cfg(not(target_arch = "wasm32"))]
use tokio::sync::{Mutex, MutexGuard, MappedMutexGuard};

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
        let db = rusqlite::Connection::open(path).unwrap();
        db.busy_timeout(Duration::ZERO).unwrap();
        db.execute(
            "CREATE TABLE if not exists kvs(key TEXT NOT NULL UNIQUE, value TEXT);", []
        ).unwrap();
        Cache(Arc::new(Mutex::new(db)))
    }

    pub async fn set<
        V: Serialize + for<'a> Deserialize <'a> + Default,
    >(&self, key: &str, value: &V) {
        self.0.lock().await.execute(
            "INSERT INTO kvs(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value;",
            [key, &hex::encode(serde_json::to_vec(value).unwrap())]
        ).unwrap();
    }

    pub async fn get<
        V: Serialize + for<'a> Deserialize <'a> + Default,
    >(&self, key: &str) -> V {
        self.0.lock().await.prepare(
            &format!("SELECT value FROM kvs where key = \'{}\'", key),
        ).unwrap().query_and_then([], |row| {
            let item: String = row.get(0).unwrap();
            Ok(hex::decode(item).unwrap())
        }).unwrap().collect::<Result<Vec<Vec<u8>>, rusqlite::Error>>().unwrap()
        .first().and_then(|b| serde_json::from_slice(b).ok()).unwrap_or_default()
    }

    pub async fn lock(&mut self, callback: impl FnOnce(&Connection)) {
        let mut guard = self.0.lock().await;
        let tx = guard.transaction_with_behavior(rusqlite::TransactionBehavior::Exclusive).unwrap();
        callback(&tx)
    }
}
