use std::sync::Arc;
use std::path::PathBuf;
use std::fmt::Debug;

#[cfg(target_os = "android")]
use winit_crate::platform::android::activity::AndroidApp;

#[cfg(not(target_arch = "wasm32"))]
use tokio::sync::Mutex;

use crate::state::Field;

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

    pub async fn set<F: Field + 'static>(&self, item: &F) {
        self.0.lock().await.execute(
            "INSERT INTO kvs(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value;",
            [F::ident(), hex::encode(item.to_bytes())]
        ).unwrap();
    }
    pub async fn get<F: Field + 'static>(&self) -> F {
        let db = self.0.lock().await;
        let mut stmt = db.prepare(&format!(
            "SELECT value FROM kvs where key = \'{}\'",
            F::ident()
        )).unwrap();
        let result = stmt.query_and_then([], |row| {
            let item: String = row.get(0).unwrap();
            Ok(hex::decode(item).unwrap())
        }).unwrap().collect::<Result<Vec<Vec<u8>>, rusqlite::Error>>().unwrap();
        result.first().map(|b| F::from_bytes(b)).unwrap_or_default()
    }
}
