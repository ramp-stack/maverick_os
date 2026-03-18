use std::sync::Arc;
use std::time::Duration;
use std::fmt::Debug;
use std::future::Future;

use tokio::sync::Mutex;

pub use rusqlite::Connection;

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
pub use android::OsApplicationSupport;

#[cfg(any(target_os = "ios", target_os = "macos"))]
mod apple;
#[cfg(any(target_os = "ios", target_os = "macos"))]
use apple::OsApplicationSupport;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux::OsApplicationSupport;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows::OsApplicationSupport;

#[derive(Debug, Clone)]
pub struct Cache(
    Arc<Mutex<Connection>>
);

impl Cache {
    pub(crate) fn new(
        #[cfg(target_os = "android")]
        vm: &jni::JavaVM
    ) -> Self {
        #[cfg(target_os = "android")]
        OsApplicationSupport::init_android(vm);
        
        let storage_path = OsApplicationSupport::get().unwrap(); 
        std::fs::create_dir_all(&storage_path).unwrap();
        let path = storage_path.join("cache.db");
        
        let db = Connection::open(&path).unwrap();
        db.busy_timeout(Duration::ZERO).unwrap();
        Cache(Arc::new(Mutex::new(db)))
    }

    pub async fn lock(&mut self) -> tokio::sync::MutexGuard<'_, Connection> {
        self.0.lock().await
    }
}
