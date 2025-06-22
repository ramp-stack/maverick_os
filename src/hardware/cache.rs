use std::sync::Arc;
use std::path::PathBuf;
use std::fmt::Debug;

use serde::{Serialize, Deserialize};

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
        let storage_path = PathBuf::from("./");
        std::fs::create_dir_all(&storage_path).unwrap();
        let path = storage_path.join("cache.db");
        let db = rusqlite::Connection::open(path).unwrap();
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
        let db = self.0.lock().await;
        let mut stmt = db.prepare(
            &format!("SELECT value FROM kvs where key = \'{}\'", key),
        ).unwrap();
        let result = stmt.query_and_then([], |row| {
            let item: String = row.get(0).unwrap();
            Ok(hex::decode(item).unwrap())
        }).unwrap().collect::<Result<Vec<Vec<u8>>, rusqlite::Error>>().unwrap();
        result.first().and_then(|b| serde_json::from_slice(b).ok()).unwrap_or_default()
    }
}
