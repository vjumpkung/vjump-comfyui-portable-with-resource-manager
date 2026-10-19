//! The installation-scoped library supervisor: debounced scans, the watcher, scan status and the
//! change signal subscribers listen to. Traversal runs on blocking threads; the UI never waits.
use super::{
    roots::LibraryRoots,
    scan::{RootScan, scan_root},
    sqlite::SqliteCatalog,
    watch::{self, DirtyRoots, RootWatcher},
};
use crate::shared::clock::SystemClock;
use application::{
    models::library::*,
    shared::{Clock, IdGenerator},
};
use async_trait::async_trait;
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio::sync::{oneshot, watch as watch_channel};

struct Inner {
    warnings: Vec<ScanWarning>,
    scanning: bool,
    watcher: WatcherState,
    last_revision: Option<u64>,
}

/// The roots a watcher was started for, and the watcher itself.
type WatchedRoots = (Vec<(String, PathBuf)>, RootWatcher);

pub struct LibrarySupervisor {
    catalog: Arc<SqliteCatalog>,
    roots: Arc<LibraryRoots>,
    pub dirty: Arc<DirtyRoots>,
    inner: Mutex<Inner>,
    changes: watch_channel::Sender<u64>,
    waiters: Mutex<Vec<oneshot::Sender<()>>>,
    watcher: Mutex<Option<WatchedRoots>>,
}

impl LibrarySupervisor {
    pub fn new(catalog: Arc<SqliteCatalog>, roots: Arc<LibraryRoots>) -> Arc<Self> {
        Arc::new(Self {
            catalog,
            roots,
            dirty: Arc::default(),
            inner: Mutex::new(Inner {
                warnings: Vec::new(),
                scanning: false,
                watcher: WatcherState::Unavailable,
                last_revision: None,
            }),
            changes: watch_channel::channel(0).0,
            waiters: Mutex::default(),
            watcher: Mutex::default(),
        })
    }

    /// Receivers see every change as a new sequence number; intermediate values coalesce, so a
    /// slow subscriber can never overflow a queue and always ends on the latest state.
    pub fn subscribe(&self) -> watch_channel::Receiver<u64> {
        self.changes.subscribe()
    }

    /// The scan loop. The first pass scans everything.
    pub async fn run(self: Arc<Self>) {
        let (_shutdown, stopped) = oneshot::channel();
        self.run_until(stopped).await;
    }

    /// Stops between scans so blocking traversal and catalog writes are never detached.
    /// Each run owns a fresh stop channel and recreates its watcher on the first scan.
    pub async fn run_until(self: Arc<Self>, mut stopped: oneshot::Receiver<()>) {
        self.dirty.mark_all_now();
        loop {
            let pending = tokio::select! {
                biased;
                _ = &mut stopped => break,
                pending = self.dirty.take_due() => pending,
            };
            let waiters =
                std::mem::take(&mut *self.waiters.lock().unwrap_or_else(|p| p.into_inner()));
            let only = (!pending.all).then_some(pending.roots);
            if self.scan_now(only).await.is_err() {
                // The next trigger tries again; a manual rescan reports the failure.
                self.dirty.mark_all();
            }
            for waiter in waiters {
                let _ = waiter.send(());
            }
        }
        self.watcher
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        self.inner.lock().unwrap_or_else(|p| p.into_inner()).watcher = WatcherState::Unavailable;
        self.waiters
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
        self.bump();
    }

    /// One scan with compare-and-apply: observations from before a catalog change are discarded
    /// and the roots are traversed again instead of overwriting newer state.
    pub async fn scan_now(&self, only: Option<HashSet<String>>) -> Result<()> {
        self.set_scanning(true);
        let outcome = self.scan_inner(only).await;
        self.set_scanning(false);
        outcome
    }

    async fn scan_inner(&self, only: Option<HashSet<String>>) -> Result<()> {
        let mut warnings = self.roots.refresh().await;
        self.sync_watcher();
        let specs: Vec<_> = self
            .roots
            .specs()
            .into_iter()
            .filter(|s| only.as_ref().is_none_or(|set| set.contains(&s.id)))
            .collect();
        let mut applied = false;
        for _ in 0..3 {
            let revision = self.catalog.revision().await?;
            let mut scans: Vec<RootScan> = Vec::new();
            for spec in &specs {
                let known = self.catalog.known_files(&spec.id).await?;
                let spec = spec.clone();
                scans.push(
                    tokio::task::spawn_blocking(move || scan_root(&spec, &known))
                        .await
                        .map_err(|_| LibraryError(LibraryErrorCode::Io))?,
                );
            }
            let mut attempt_warnings = Vec::new();
            for scan in &mut scans {
                attempt_warnings.append(&mut scan.warnings);
            }
            if self
                .catalog
                .apply_scan(revision, scans, SystemClock.now())
                .await?
            {
                warnings.extend(attempt_warnings);
                applied = true;
                break;
            }
        }
        if !applied {
            // Still racing with writers: leave the catalog alone and scan again soon.
            self.dirty.mark_all();
        }
        let revision = self.catalog.revision().await?;
        let notify = {
            let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
            let changed = inner.last_revision != Some(revision) || inner.warnings != warnings;
            inner.warnings = warnings;
            inner.last_revision = Some(revision);
            changed
        };
        if notify {
            self.bump();
        }
        Ok(())
    }

    fn set_scanning(&self, scanning: bool) {
        self.inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .scanning = scanning;
        self.bump();
    }

    fn bump(&self) {
        self.changes.send_modify(|n| *n += 1);
    }

    /// (Re)starts watching when the set of roots changed.
    fn sync_watcher(&self) {
        let wanted: Vec<(String, PathBuf)> = self
            .roots
            .specs()
            .into_iter()
            .map(|s| (s.id, s.path))
            .collect();
        let mut watcher = self.watcher.lock().unwrap_or_else(|p| p.into_inner());
        if watcher.as_ref().is_some_and(|(roots, _)| *roots == wanted) {
            return;
        }
        let started = watch::start(&wanted, &self.dirty);
        self.inner.lock().unwrap_or_else(|p| p.into_inner()).watcher = started.state;
        *watcher = Some((wanted, started));
    }
}

#[async_trait]
impl LibraryControl for LibrarySupervisor {
    async fn rescan(&self) -> Result<()> {
        let (sender, receiver) = oneshot::channel();
        self.waiters
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(sender);
        self.dirty.mark_all_now();
        receiver
            .await
            .map_err(|_| LibraryError(LibraryErrorCode::Io))
    }

    fn status(&self) -> LibraryStatus {
        let inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        LibraryStatus {
            sequence: *self.changes.borrow(),
            roots: self.roots.infos(),
            warnings: inner.warnings.clone(),
            scanning: inner.scanning,
            watcher: inner.watcher,
        }
    }

    fn changed(&self) {
        self.bump();
    }
}

/// Shared id generator for library operations.
pub fn ids() -> Arc<dyn IdGenerator> {
    Arc::new(crate::shared::clock::RandomIds::default())
}
