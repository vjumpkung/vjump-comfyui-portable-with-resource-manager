//! File-system watching. The watcher callback only records a bounded invalidation (a set of
//! root ids), so a flood of events can never grow memory; the supervisor debounces it into scans.
use application::models::library::WatcherState;
use notify::{Config, PollWatcher, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};
use tokio::sync::Notify;

/// Time without new events before a scan starts.
pub const QUIET: Duration = Duration::from_millis(250);
/// The longest a continuous stream of events may delay a scan.
pub const MAX_DELAY: Duration = Duration::from_secs(1);
/// Polling interval when native watching is unavailable.
pub const POLL_INTERVAL: Duration = Duration::from_secs(30);

/// When a pending invalidation becomes due: the earlier of quiet time and the maximum delay.
pub fn due_at(first: Instant, last: Instant) -> Instant {
    (last + QUIET).min(first + MAX_DELAY)
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Pending {
    pub all: bool,
    pub roots: HashSet<String>,
}
#[derive(Default)]
struct DirtyState {
    pending: Pending,
    first: Option<Instant>,
    last: Option<Instant>,
    immediate: bool,
}

/// Roots that need a rescan. Marking is cheap and callable from any thread.
#[derive(Default)]
pub struct DirtyRoots {
    state: Mutex<DirtyState>,
    notify: Notify,
}
impl DirtyRoots {
    fn touch(&self, change: impl FnOnce(&mut DirtyState)) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let now = Instant::now();
        state.first.get_or_insert(now);
        state.last = Some(now);
        change(&mut state);
        drop(state);
        self.notify.notify_one();
    }
    pub fn mark(&self, root_id: &str) {
        self.touch(|s| {
            s.pending.roots.insert(root_id.to_owned());
        });
    }
    pub fn mark_all(&self) {
        self.touch(|s| s.pending.all = true);
    }
    /// A manual rescan: no debounce.
    pub fn mark_all_now(&self) {
        self.touch(|s| {
            s.pending.all = true;
            s.immediate = true;
        });
    }
    /// Waits for pending invalidations to become due and takes them.
    pub async fn take_due(&self) -> Pending {
        loop {
            let wait = {
                let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
                match (state.first, state.last) {
                    (Some(first), Some(last)) => {
                        let due = if state.immediate {
                            Instant::now()
                        } else {
                            due_at(first, last)
                        };
                        let now = Instant::now();
                        if due <= now {
                            state.first = None;
                            state.last = None;
                            state.immediate = false;
                            return std::mem::take(&mut state.pending);
                        }
                        Some(due - now)
                    }
                    _ => None,
                }
            };
            match wait {
                Some(delay) => {
                    tokio::select! {
                        () = tokio::time::sleep(delay) => {}
                        () = self.notify.notified() => {}
                    }
                }
                None => self.notify.notified().await,
            }
        }
    }
}

/// Keeps the native or polling watcher alive.
pub struct RootWatcher {
    _native: Option<RecommendedWatcher>,
    _poll: Option<PollWatcher>,
    pub state: WatcherState,
}

fn handler(
    roots: Vec<(String, PathBuf)>,
    dirty: std::sync::Arc<DirtyRoots>,
) -> impl FnMut(notify::Result<notify::Event>) + Send + 'static {
    move |event| match event {
        // Reads (including our own scans and hashing) must not trigger more scans.
        Ok(event) if matches!(event.kind, notify::EventKind::Access(_)) => {}
        Ok(event) => {
            if event.paths.is_empty() {
                dirty.mark_all();
            }
            for path in &event.paths {
                match roots.iter().find(|(_, root)| path.starts_with(root)) {
                    Some((id, _)) => dirty.mark(id),
                    None => dirty.mark_all(),
                }
            }
        }
        // Overflow or any watcher error: the affected state is unknown, so rescan everything.
        Err(_) => dirty.mark_all(),
    }
}

/// Starts watching every root recursively. Native watching is preferred; a failure falls back to
/// a 30 second poll, and total failure is reported as unavailable (manual rescan still works).
pub fn start(roots: &[(String, PathBuf)], dirty: &std::sync::Arc<DirtyRoots>) -> RootWatcher {
    if let Ok(mut native) =
        RecommendedWatcher::new(handler(roots.to_vec(), dirty.clone()), Config::default())
        && roots
            .iter()
            .all(|(_, path)| native.watch(path, RecursiveMode::Recursive).is_ok())
    {
        return RootWatcher {
            _native: Some(native),
            _poll: None,
            state: WatcherState::Native,
        };
    }
    let config = Config::default().with_poll_interval(POLL_INTERVAL);
    if let Ok(mut poll) = PollWatcher::new(handler(roots.to_vec(), dirty.clone()), config)
        && roots
            .iter()
            .all(|(_, path)| poll.watch(path, RecursiveMode::Recursive).is_ok())
    {
        return RootWatcher {
            _native: None,
            _poll: Some(poll),
            state: WatcherState::Polling,
        };
    }
    RootWatcher {
        _native: None,
        _poll: None,
        state: WatcherState::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scan_is_due_after_quiet_time_but_never_later_than_the_maximum_delay() {
        let start = Instant::now();
        // Quiet: one event, due after 250 ms.
        assert_eq!(due_at(start, start), start + QUIET);
        // Continuous activity: the last event keeps moving, the cap does not.
        let last = start + Duration::from_millis(900);
        assert_eq!(due_at(start, last), start + MAX_DELAY);
    }

    #[tokio::test]
    async fn bursts_collapse_into_one_pending_set_and_a_manual_rescan_is_immediate() {
        let dirty = std::sync::Arc::new(DirtyRoots::default());
        for _ in 0..10_000 {
            dirty.mark("models");
        }
        dirty.mark("extra-a");
        let waiting = {
            let dirty = dirty.clone();
            tokio::spawn(async move { dirty.take_due().await })
        };
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!waiting.is_finished(), "debounce window still open");
        tokio::time::sleep(Duration::from_millis(350)).await;
        let pending = waiting.await.unwrap();
        assert!(!pending.all);
        assert_eq!(pending.roots.len(), 2);

        dirty.mark_all_now();
        let pending = tokio::time::timeout(Duration::from_millis(100), dirty.take_due())
            .await
            .expect("manual rescans skip the debounce");
        assert!(pending.all);
    }

    #[test]
    fn watching_a_real_directory_starts_without_error() {
        let directory = tempfile::tempdir().unwrap();
        let dirty = std::sync::Arc::new(DirtyRoots::default());
        let watcher = start(&[("models".into(), directory.path().to_owned())], &dirty);
        assert_ne!(watcher.state, WatcherState::Unavailable);
        // A path that cannot be watched falls back and ends up reported, not panicking.
        let missing = directory.path().join("missing");
        let degraded = start(&[("gone".into(), missing)], &dirty);
        assert_ne!(degraded.state, WatcherState::Native);
    }
}
