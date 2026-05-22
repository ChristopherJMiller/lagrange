use crate::config::Settings;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, OwnedMutexGuard};

#[derive(Clone)]
pub struct AppState {
    pub settings: Arc<Settings>,
    pub db: SqlitePool,
    vm_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

impl AppState {
    pub fn new(settings: Settings, db: SqlitePool) -> Self {
        Self {
            settings: Arc::new(settings),
            db,
            vm_locks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Serialize lifecycle operations on a single VM. Held for the duration
    /// of the returned guard.
    pub async fn lock_vm(&self, name: &str) -> OwnedMutexGuard<()> {
        let mut map = self.vm_locks.lock().await;
        let m = map
            .entry(name.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        drop(map);
        m.lock_owned().await
    }
}
