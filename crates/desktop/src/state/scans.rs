//! Background custom-node scans: the startup scan, and the scan requested after ComfyUI stopped,
//! restarted or updated. A scan never blocks Stop, and delays Start, Restart or Update only for a
//! few seconds while it settles interrupted node operations, so this scheduler is the only place
//! that waits for a whole scan. Requests while a scan runs queue exactly one more run, and a scan
//! that a node operation cancelled runs again once the operation is done.

use std::sync::{Arc, Mutex};

use application::custom_nodes::{NodeAppError, NodeService};
use infrastructure::custom_nodes::stack::{NodeEvent, NodeHub};
use tokio::sync::watch;

#[derive(Default)]
struct Queue {
    /// A run task exists (queued for the node lock, or scanning).
    running: bool,
    /// A request arrived while a run existed; one more scan follows it.
    again: bool,
}

pub(crate) struct ScanScheduler {
    service: NodeService,
    hub: Arc<NodeHub>,
    /// True from the moment the startup scan is armed until a scan run has ended, including the
    /// time it waits for the node lock. The page shows its rows as not yet complete meanwhile.
    pub(super) startup: watch::Sender<bool>,
    /// A background scan is queued or running; rows stay visible and only a quiet hint shows.
    pub(super) refreshing: watch::Sender<bool>,
    queue: Mutex<Queue>,
    /// How many scans the runs started, for tests.
    #[cfg(test)]
    pub(super) scans_started: std::sync::atomic::AtomicUsize,
}

/// Ends a scan run however its task ends (completion, error, panic or cancellation): clears the
/// flags first, then tells subscribers to refetch, and starts the scan that was queued meanwhile.
struct ScanRun(Arc<ScanScheduler>);

impl Drop for ScanRun {
    fn drop(&mut self) {
        let scheduler = &self.0;
        let again = {
            let mut queue = scheduler.queue.lock().unwrap_or_else(|p| p.into_inner());
            queue.running = false;
            // Cleared while the queue is locked: a request that creates the next run sets the
            // flags under the same lock, so this can never wipe the flags of that run.
            scheduler.startup.send_replace(false);
            scheduler.refreshing.send_replace(false);
            std::mem::take(&mut queue.again)
        };
        scheduler.hub.publish(NodeEvent::Changed);
        if again {
            scheduler.request();
        }
    }
}

impl ScanScheduler {
    pub(crate) fn new(service: NodeService, hub: Arc<NodeHub>) -> Arc<Self> {
        Arc::new(Self {
            service,
            hub,
            startup: watch::Sender::new(false),
            refreshing: watch::Sender::new(false),
            queue: Mutex::default(),
            #[cfg(test)]
            scans_started: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    /// Marks the startup scan as scheduled; `NodeServices::start_scan` runs it once the state is
    /// published.
    pub(crate) fn arm_startup(&self) {
        self.startup.send_replace(true);
    }

    /// Whether the startup scan is scheduled or running.
    pub(crate) fn scanning(&self) -> bool {
        *self.startup.borrow()
    }

    /// Whether a background scan is queued or running.
    pub(crate) fn refreshing(&self) -> bool {
        *self.refreshing.borrow()
    }

    /// Asks for a scan and returns at once. While a scan is queued or running, the request only
    /// queues one more run. Nothing is scheduled while the app closes.
    pub(crate) fn request(self: &Arc<Self>) {
        if self.service.scans().closing() {
            // What the scan would have seen is not recorded; say so, and do not leave the
            // flags set for a run that will never exist.
            self.service.scans().mark_incomplete();
            let idle = {
                let queue = self.queue.lock().unwrap_or_else(|p| p.into_inner());
                let idle = !queue.running;
                if idle {
                    self.startup.send_replace(false);
                    self.refreshing.send_replace(false);
                }
                idle
            };
            if idle {
                self.hub.publish(NodeEvent::Changed);
            }
            return;
        }
        {
            let mut queue = self.queue.lock().unwrap_or_else(|p| p.into_inner());
            if queue.running {
                queue.again = true;
                return;
            }
            queue.running = true;
            // Set under the queue lock, like the clearing in `ScanRun::drop`.
            self.refreshing.send_replace(true);
        }
        self.hub.publish(NodeEvent::Changed);
        // Created before the task exists, so even a task that is dropped unpolled ends the run.
        let run = ScanRun(self.clone());
        tauri::async_runtime::spawn(async move { Self::run(run).await });
    }

    async fn run(guard: ScanRun) {
        let this = guard.0.clone();
        loop {
            #[cfg(test)]
            this.scans_started
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let result = this.service.rescan_when_free().await;
            match &result {
                Ok(_) => tracing::info!("custom node scan finished"),
                Err(NodeAppError::Cancelled) => tracing::info!(
                    "custom node scan stopped early (the app is closing, or a node operation needed the node lock)"
                ),
                Err(error) => tracing::error!(%error, "custom node scan needs attention"),
            }
            let again =
                std::mem::take(&mut this.queue.lock().unwrap_or_else(|p| p.into_inner()).again);
            let yielded = matches!(result, Err(NodeAppError::Cancelled));
            if !(again || yielded) || this.service.scans().closing() {
                return;
            }
            if yielded {
                // The operation that cancelled the scan queues for the lock before the next try.
                tokio::task::yield_now().await;
            }
        }
    }

    /// Waits until no startup scan is scheduled or running. Only tests use it.
    #[cfg(test)]
    pub(crate) async fn wait_for_scan(&self) {
        let mut scanning = self.startup.subscribe();
        // The returned guard is dropped at once; it must not live across another await.
        let _ = scanning.wait_for(|running| !*running).await;
    }

    /// Waits until no background scan is queued or running. Only tests use it.
    #[cfg(test)]
    pub(crate) async fn wait_for_refresh(&self) {
        let mut refreshing = self.refreshing.subscribe();
        let _ = refreshing.wait_for(|running| !*running).await;
    }
}
#[cfg(test)]
mod tests {
    use std::{
        sync::atomic::Ordering::SeqCst,
        time::{Duration, Instant},
    };

    use super::*;
    use crate::state::{
        AppState,
        recovery_tests::{WAIT, compose_without_services, root_with_manual_node},
    };

    async fn composed(directory: &tempfile::TempDir) -> AppState {
        compose_without_services(&root_with_manual_node(directory)).await
    }

    /// Waits for `condition` without a fixed sleep for the happy path.
    async fn eventually(condition: impl Fn() -> bool) {
        let start = Instant::now();
        while !condition() {
            assert!(start.elapsed() < WAIT, "the condition never became true");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    #[tokio::test]
    async fn requests_while_a_scan_runs_queue_exactly_one_more() {
        let directory = tempfile::tempdir().unwrap();
        let state = composed(&directory).await;
        let nodes = &state.services().unwrap().nodes;
        let scheduler = nodes.scheduler.clone();
        let mut events = nodes.stack.hub.subscribe();
        // The node lock is taken, so the first scan queues and every later request finds it.
        let lock = nodes.stack.deps.node_lock.acquire().await.unwrap();

        for _ in 0..4 {
            scheduler.request();
        }

        assert!(scheduler.refreshing());
        // The refresh is announced once, when the first request creates the run.
        assert_eq!(events.recv().await.unwrap().1, NodeEvent::Changed);
        assert!(events.try_recv().is_err());
        drop(lock);
        tokio::time::timeout(WAIT, scheduler.wait_for_refresh())
            .await
            .unwrap();
        assert_eq!(scheduler.scans_started.load(SeqCst), 2);
        assert!(!scheduler.refreshing());
        assert_eq!(nodes.service.list().await.unwrap().len(), 1);
        // Nothing stays queued: a later request starts exactly one scan again.
        scheduler.request();
        tokio::time::timeout(WAIT, scheduler.wait_for_refresh())
            .await
            .unwrap();
        assert_eq!(scheduler.scans_started.load(SeqCst), 3);
    }

    #[tokio::test]
    async fn a_scan_that_yielded_runs_again_and_closing_stops_it() {
        let directory = tempfile::tempdir().unwrap();
        let state = composed(&directory).await;
        let nodes = &state.services().unwrap().nodes;
        let scheduler = nodes.scheduler.clone();
        let scans = nodes.service.scans().clone();

        // A node operation waits for the node lock: every scan that gets in is cancelled at once
        // and tried again behind it. The yield below is deliberately an orphan that never takes
        // the lock, so the retries are forced; with the real (FIFO) tokio mutex the operation
        // that cancelled the scan is served before the next try and nothing spins.
        let operation = scans.yield_to_operation();
        scheduler.request();
        eventually(|| scheduler.scans_started.load(SeqCst) >= 2).await;
        assert!(scheduler.refreshing());
        assert!(scans.incomplete());
        drop(operation);
        tokio::time::timeout(WAIT, scheduler.wait_for_refresh())
            .await
            .unwrap();
        assert_eq!(nodes.service.list().await.unwrap().len(), 1);
        assert!(!scans.incomplete());

        // Closing ends the retries for good, and nothing new is scheduled meanwhile.
        let operation = scans.yield_to_operation();
        scheduler.request();
        let before = scheduler.scans_started.load(SeqCst);
        eventually(|| scheduler.scans_started.load(SeqCst) >= before + 2).await;
        let pause = scans.pause();
        tokio::time::timeout(WAIT, scheduler.wait_for_refresh())
            .await
            .unwrap();
        let started = scheduler.scans_started.load(SeqCst);
        scheduler.request();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(scheduler.scans_started.load(SeqCst), started);
        assert!(!scheduler.refreshing());
        assert!(scans.incomplete());
        drop((operation, pause));
    }

    #[tokio::test]
    async fn the_startup_flag_clears_when_a_request_finds_the_app_closing() {
        let directory = tempfile::tempdir().unwrap();
        let state = composed(&directory).await;
        let nodes = &state.services().unwrap().nodes;
        let scheduler = nodes.scheduler.clone();
        let mut events = nodes.stack.hub.subscribe();
        assert!(scheduler.scanning());
        let pause = nodes.service.scans().pause();

        scheduler.request();

        assert!(!scheduler.scanning() && !scheduler.refreshing());
        assert!(nodes.service.scans().incomplete());
        assert_eq!(events.try_recv().unwrap().1, NodeEvent::Changed);
        assert_eq!(scheduler.scans_started.load(SeqCst), 0);
        drop(pause);
    }

    /// A run that already exists, with its flags set, and an observer that reads the flags at
    /// the moment `Changed` arrives.
    async fn started_run(
        scheduler: &Arc<ScanScheduler>,
        hub: &NodeHub,
    ) -> (ScanRun, tokio::task::JoinHandle<(NodeEvent, bool, bool)>) {
        let mut events = hub.subscribe();
        let startup = scheduler.startup.subscribe();
        let refreshing = scheduler.refreshing.subscribe();
        let observer = tokio::spawn(async move {
            let (_, event) = events.recv().await.unwrap();
            (event, *startup.borrow(), *refreshing.borrow())
        });
        scheduler.queue.lock().unwrap().running = true;
        scheduler.startup.send_replace(true);
        scheduler.refreshing.send_replace(true);
        (ScanRun(scheduler.clone()), observer)
    }

    #[tokio::test]
    async fn flags_clear_however_the_run_ends() {
        let directory = tempfile::tempdir().unwrap();
        let state = composed(&directory).await;
        let nodes = &state.services().unwrap().nodes;
        let scheduler = nodes.scheduler.clone();
        let hub = nodes.stack.hub.clone();
        let cleared = |scheduler: &ScanScheduler| {
            !scheduler.scanning()
                && !scheduler.refreshing()
                && !scheduler.queue.lock().unwrap().running
        };

        // It ends normally.
        let (run, observer) = started_run(&scheduler, &hub).await;
        drop(run);
        assert_eq!(observer.await.unwrap(), (NodeEvent::Changed, false, false));
        assert!(cleared(&scheduler));

        // Its task panics.
        let (run, observer) = started_run(&scheduler, &hub).await;
        let task = tokio::spawn(async move {
            let _run = run;
            panic!("the scan panicked");
        });
        assert!(task.await.unwrap_err().is_panic());
        assert_eq!(observer.await.unwrap(), (NodeEvent::Changed, false, false));
        assert!(cleared(&scheduler));

        // Its task is cancelled.
        let (run, observer) = started_run(&scheduler, &hub).await;
        let task = tokio::spawn(async move {
            let _run = run;
            std::future::pending::<()>().await;
        });
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert_eq!(observer.await.unwrap(), (NodeEvent::Changed, false, false));
        assert!(cleared(&scheduler));

        // Its future is dropped before it was ever polled.
        let (run, observer) = started_run(&scheduler, &hub).await;
        let future = async move {
            let _run = run;
        };
        drop(future);
        assert_eq!(observer.await.unwrap(), (NodeEvent::Changed, false, false));
        assert!(cleared(&scheduler));
    }

    #[tokio::test]
    async fn a_request_that_arrives_as_a_run_ends_still_runs() {
        let directory = tempfile::tempdir().unwrap();
        let state = composed(&directory).await;
        let nodes = &state.services().unwrap().nodes;
        let scheduler = nodes.scheduler.clone();
        let hub = nodes.stack.hub.clone();
        let (run, _observer) = started_run(&scheduler, &hub).await;
        // A request found the run and queued behind it, but the run never saw it (it panicked).
        scheduler.request();
        assert_eq!(scheduler.scans_started.load(SeqCst), 0);

        drop(run);

        tokio::time::timeout(WAIT, scheduler.wait_for_refresh())
            .await
            .unwrap();
        assert_eq!(scheduler.scans_started.load(SeqCst), 1);
        assert_eq!(nodes.service.list().await.unwrap().len(), 1);
    }
}
