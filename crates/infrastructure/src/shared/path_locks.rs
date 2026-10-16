//! The installation-wide path coordinator shared by download publication and library
//! mutations. Lock identity is the physical location (registered root directory plus the
//! lowercased relative path), so overlapping roots and aliases cannot bypass coordination.
use application::shared::path_locks::{
    DestinationReservations, ModelPathLease, ModelPathLocks, PathLease, PathLockError,
    PathLocks as PathLocksPort,
};
use async_trait::async_trait;
use domain::models::library::ModelLocation;
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex, OnceLock, Weak},
};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

#[derive(Default)]
pub struct PathLocks {
    locks: Mutex<HashMap<String, Weak<AsyncMutex<()>>>>,
    /// Registered root id -> physical directory (lowercase, forward slashes).
    roots: Mutex<HashMap<String, String>>,
    reservations: OnceLock<Arc<dyn DestinationReservations>>,
}
struct Lease {
    _guards: Vec<OwnedMutexGuard<()>>,
}
impl ModelPathLease for Lease {}
impl PathLease for Lease {}

fn physical_form(path: &Path) -> String {
    let text = path.to_string_lossy();
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
    text.replace('\\', "/").trim_end_matches('/').to_lowercase()
}

impl PathLocks {
    /// Registers (or replaces) the physical directory of a root id.
    pub fn register_root(&self, root_id: &str, directory: &Path) {
        self.roots
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(root_id.to_owned(), physical_form(directory));
    }
    /// Installs the durable destination reservations once. Later calls are ignored.
    pub fn set_reservations(&self, reservations: Arc<dyn DestinationReservations>) {
        let _ = self.reservations.set(reservations);
    }

    /// The lock key of `root_id:relative` (relative already lowercase).
    fn key(&self, root_id: &str, relative: &str) -> String {
        let roots = self.roots.lock().unwrap_or_else(|p| p.into_inner());
        match roots.get(root_id) {
            Some(physical) if relative.is_empty() => physical.clone(),
            Some(physical) => format!("{physical}/{relative}"),
            None => format!("{root_id}:{relative}"),
        }
    }

    /// Every download-style reservation key that could address the same physical file.
    fn reservation_keys(&self, location: &ModelLocation) -> Vec<String> {
        let relative = location.path.as_string().to_lowercase();
        let physical = self.key(&location.root_id, &relative);
        let roots = self.roots.lock().unwrap_or_else(|p| p.into_inner());
        let mut keys = vec![location.key()];
        for (id, root) in roots.iter() {
            if let Some(rest) = physical.strip_prefix(&format!("{root}/")) {
                keys.push(format!("{id}:{rest}"));
            }
        }
        keys
    }

    async fn lock_key(&self, key: String) -> OwnedMutexGuard<()> {
        let lock = {
            let mut locks = self.locks.lock().unwrap_or_else(|p| p.into_inner());
            locks.retain(|_, v| v.strong_count() > 0);
            let entry = locks.entry(key).or_default();
            match entry.upgrade() {
                Some(v) => v,
                None => {
                    let v = Arc::new(AsyncMutex::new(()));
                    *entry = Arc::downgrade(&v);
                    v
                }
            }
        };
        lock.lock_owned().await
    }
}

#[async_trait]
impl ModelPathLocks for PathLocks {
    async fn acquire(&self, key: &str) -> Box<dyn ModelPathLease> {
        let key = key.to_lowercase();
        let physical = match key.split_once(':') {
            Some((root_id, relative)) => self.key(root_id, relative),
            None => key,
        };
        Box::new(Lease {
            _guards: vec![self.lock_key(physical).await],
        })
    }
}

#[async_trait]
impl PathLocksPort for PathLocks {
    async fn acquire_many(
        &self,
        locations: Vec<ModelLocation>,
    ) -> Result<Box<dyn PathLease>, PathLockError> {
        if let Some(reservations) = self.reservations.get() {
            for location in &locations {
                for key in self.reservation_keys(location) {
                    if reservations.is_reserved(&key).await {
                        return Err(PathLockError::BusyReserved);
                    }
                }
            }
        }
        // Deduplicate and order the physical keys so concurrent callers cannot deadlock.
        let mut keys: Vec<String> = locations
            .iter()
            .map(|l| self.key(&l.root_id, &l.path.as_string().to_lowercase()))
            .collect();
        keys.sort();
        keys.dedup();
        let mut guards = Vec::with_capacity(keys.len());
        for key in keys {
            guards.push(self.lock_key(key).await);
        }
        Ok(Box::new(Lease { _guards: guards }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::shared::model_values::RelativeModelPath;
    use std::time::Duration;

    fn loc(root: &str, path: &str) -> ModelLocation {
        ModelLocation::new(root, RelativeModelPath::new(path).unwrap())
    }

    struct Reserved(&'static str);
    #[async_trait]
    impl DestinationReservations for Reserved {
        async fn is_reserved(&self, key: &str) -> bool {
            key == self.0
        }
    }

    #[tokio::test]
    async fn overlapping_roots_share_one_lock_for_the_same_physical_file() {
        let directory = tempfile::tempdir().unwrap();
        let locks = Arc::new(PathLocks::default());
        locks.register_root("models", directory.path());
        locks.register_root("extra-a", &directory.path().join("checkpoints"));
        let held = locks
            .acquire_many(vec![loc("models", "checkpoints/X.pt")])
            .await
            .unwrap();
        let contender = {
            let locks = locks.clone();
            tokio::spawn(async move {
                locks
                    .acquire_many(vec![loc("extra-a", "x.PT")])
                    .await
                    .map(|_| ())
            })
        };
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            !contender.is_finished(),
            "the alias must wait for the owner"
        );
        drop(held);
        contender.await.unwrap().unwrap();
        // The download-style single key addresses the same physical file.
        let held = locks.acquire("models:checkpoints/x.pt").await;
        let waiting = {
            let locks = locks.clone();
            tokio::spawn(async move {
                locks
                    .acquire_many(vec![loc("extra-a", "x.pt")])
                    .await
                    .map(|_| ())
            })
        };
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!waiting.is_finished());
        drop(held);
        waiting.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn crossed_multi_path_requests_do_not_deadlock() {
        let locks = Arc::new(PathLocks::default());
        let mut tasks = Vec::new();
        for round in 0..20 {
            let locks = locks.clone();
            tasks.push(tokio::spawn(async move {
                let (a, b) = (loc("models", "c/a.pt"), loc("models", "c/b.pt"));
                let request = if round % 2 == 0 {
                    vec![a, b]
                } else {
                    vec![b, a]
                };
                let lease = locks.acquire_many(request).await.unwrap();
                tokio::task::yield_now().await;
                drop(lease);
            }));
        }
        let all = async {
            for task in tasks {
                task.await.unwrap();
            }
        };
        tokio::time::timeout(Duration::from_secs(10), all)
            .await
            .expect("no deadlock");
    }

    #[tokio::test]
    async fn a_durable_download_reservation_reports_busy_reserved() {
        let directory = tempfile::tempdir().unwrap();
        let locks = PathLocks::default();
        locks.register_root("models", directory.path());
        locks.set_reservations(Arc::new(Reserved("models:checkpoints/busy.pt")));
        assert_eq!(
            locks
                .acquire_many(vec![loc("models", "checkpoints/Busy.pt")])
                .await
                .err(),
            Some(PathLockError::BusyReserved)
        );
        locks
            .acquire_many(vec![loc("models", "checkpoints/free.pt")])
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn an_overlapping_extra_root_still_sees_the_default_roots_reservation() {
        let directory = tempfile::tempdir().unwrap();
        let locks = PathLocks::default();
        locks.register_root("models", directory.path());
        locks.register_root("extra-a", &directory.path().join("checkpoints"));
        locks.set_reservations(Arc::new(Reserved("models:checkpoints/busy.pt")));
        assert_eq!(
            locks
                .acquire_many(vec![loc("extra-a", "busy.pt")])
                .await
                .err(),
            Some(PathLockError::BusyReserved)
        );
    }
}
