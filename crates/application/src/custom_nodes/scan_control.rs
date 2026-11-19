use super::NodeAppError;
use crate::shared::{CancelToken, MaintenanceLease};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct State {
    paused: usize,
    /// Node operations that cancelled the active scan and wait for the node lock.
    yielding: usize,
    /// Seqlock-style start counter: [`ScanControl::note_start`] makes it odd (a start is in
    /// progress) and [`ScanControl::start_finished`] makes it even again. It only ever grows, so a
    /// scan that saw it odd or changed must not trust a stopped reading taken before.
    starts: u64,
    /// Scans that are about to settle, or are settling, interrupted node operations.
    settling: usize,
    active: Option<CancelToken>,
    incomplete: bool,
}

/// Process-local coordination for cooperative inventory observation.
#[derive(Default)]
pub struct ScanControl(Mutex<State>);

impl ScanControl {
    /// Starts a scan. The caller holds the node lock, so at most one scan is active. None while
    /// closing or while a node operation waits for the node lock.
    pub fn begin(self: &Arc<Self>, _nodes: &dyn MaintenanceLease) -> Option<ScanActivity> {
        let mut state = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if state.paused > 0 || state.yielding > 0 {
            return None;
        }
        debug_assert!(
            state.active.is_none(),
            "scan already active under the node lock"
        );
        if state.active.is_some() {
            return None;
        }
        let token = CancelToken::new();
        state.active = Some(token.clone());
        Some(ScanActivity {
            scans: self.clone(),
            token,
            finalized: false,
        })
    }

    pub fn mark_incomplete(&self) {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).incomplete = true;
    }

    pub fn incomplete(&self) -> bool {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).incomplete
    }

    pub fn pause(self: &Arc<Self>) -> ScanPause {
        let mut state = self.0.lock().unwrap_or_else(|p| p.into_inner());
        state.paused += 1;
        let stopped_scan = if let Some(token) = &state.active {
            token.cancel();
            true
        } else {
            false
        };
        ScanPause {
            scans: self.clone(),
            stopped_scan,
            kept: false,
        }
    }

    pub fn closing(&self) -> bool {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).paused > 0
    }

    /// A node operation (or a setup run) is about to wait for the node lock: cancels the active
    /// scan and holds new ones off until the returned guard is dropped.
    pub fn yield_to_operation(self: &Arc<Self>) -> ScanYield {
        let mut state = self.0.lock().unwrap_or_else(|p| p.into_inner());
        state.yielding += 1;
        if let Some(token) = &state.active {
            token.cancel();
        }
        ScanYield {
            scans: self.clone(),
        }
    }

    /// Records that a start of ComfyUI began: the counter turns odd until
    /// [`Self::start_finished`].
    pub fn note_start(&self) {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).starts += 1;
    }

    /// Records that the start recorded by [`Self::note_start`] ended, however it ended: the
    /// counter turns even again.
    pub fn start_finished(&self) {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).starts += 1;
    }

    /// The start counter: odd while a start is in progress, and it changes whenever one began or
    /// ended, so equal readings mean no start happened in between.
    pub fn starts(&self) -> u64 {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).starts
    }

    /// Marks a scan as settling interrupted node operations until the guard is dropped. Created
    /// before the scan tries the maintenance gate, so Start can tell a scan that holds it briefly
    /// from another holder.
    pub fn enter_settling(self: &Arc<Self>) -> ScanSettling {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).settling += 1;
        ScanSettling {
            scans: self.clone(),
        }
    }

    /// Whether a scan is settling interrupted node operations right now.
    pub fn settling(&self) -> bool {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).settling > 0
    }
}

pub struct ScanActivity {
    scans: Arc<ScanControl>,
    token: CancelToken,
    finalized: bool,
}

impl ScanActivity {
    pub fn token(&self) -> &CancelToken {
        &self.token
    }

    pub fn finish<T>(mut self, result: Result<T, NodeAppError>) -> Result<T, NodeAppError> {
        let mut state = self.scans.0.lock().unwrap_or_else(|p| p.into_inner());
        let cancelled = self.token.is_cancelled();
        state.active = None;
        self.finalized = true;
        match result {
            Ok(value) => {
                state.incomplete = false;
                if cancelled {
                    Err(NodeAppError::Cancelled)
                } else {
                    Ok(value)
                }
            }
            Err(NodeAppError::RuntimeRunning) => Err(NodeAppError::RuntimeRunning),
            Err(error) => {
                state.incomplete = true;
                Err(error)
            }
        }
    }
}

impl Drop for ScanActivity {
    fn drop(&mut self) {
        if !self.finalized {
            self.scans
                .0
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .active = None;
        }
    }
}

pub struct ScanPause {
    scans: Arc<ScanControl>,
    stopped_scan: bool,
    kept: bool,
}

impl ScanPause {
    pub fn stopped_scan(&self) -> bool {
        self.stopped_scan
    }

    pub fn keep(mut self) {
        self.kept = true;
    }
}

impl Drop for ScanPause {
    fn drop(&mut self) {
        if !self.kept {
            self.scans
                .0
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .paused -= 1;
        }
    }
}

/// A scan that is settling interrupted node operations; see [`ScanControl::enter_settling`].
pub struct ScanSettling {
    scans: Arc<ScanControl>,
}

impl Drop for ScanSettling {
    fn drop(&mut self) {
        self.scans
            .0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .settling -= 1;
    }
}

/// Holds new scans off while a node operation waits for the node lock.
pub struct ScanYield {
    scans: Arc<ScanControl>,
}

impl Drop for ScanYield {
    fn drop(&mut self) {
        self.scans
            .0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .yielding -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::Retained;

    struct Lease;
    impl MaintenanceLease for Lease {
        fn retained(&self) -> Retained {
            Retained::default()
        }
    }

    #[test]
    fn pause_cancels_and_resume_uses_a_fresh_token() {
        let scans = Arc::new(ScanControl::default());
        let activity = scans.begin(&Lease).unwrap();
        let old = activity.token().clone();
        let pause = scans.pause();
        assert!(pause.stopped_scan());
        assert!(old.is_cancelled());
        assert!(scans.begin(&Lease).is_none());
        drop(activity);
        drop(pause);
        let next = scans.begin(&Lease).unwrap();
        assert!(!next.token().is_cancelled());
        assert!(old.is_cancelled());
    }

    #[test]
    fn pauses_are_counted_and_keep_leaves_scans_paused() {
        let scans = Arc::new(ScanControl::default());
        let first = scans.pause();
        let second = scans.pause();
        assert!(!first.stopped_scan());
        drop(first);
        assert!(scans.closing());
        assert!(scans.begin(&Lease).is_none());
        drop(second);
        assert!(!scans.closing());
        scans.pause().keep();
        assert!(scans.closing());
        assert!(scans.begin(&Lease).is_none());
    }

    #[test]
    fn drop_clears_active_even_during_unwinding() {
        let scans = Arc::new(ScanControl::default());
        drop(scans.begin(&Lease).unwrap());
        assert!(scans.begin(&Lease).is_some());
        let unwound = std::panic::catch_unwind(|| {
            let _activity = scans.begin(&Lease).unwrap();
            panic!("scan panicked");
        });
        assert!(unwound.is_err());
        assert!(scans.begin(&Lease).is_some());
    }

    #[test]
    fn finish_records_failures_and_clears_incomplete_on_success() {
        let scans = Arc::new(ScanControl::default());
        assert_eq!(
            scans
                .begin(&Lease)
                .unwrap()
                .finish::<()>(Err(NodeAppError::Filesystem)),
            Err(NodeAppError::Filesystem)
        );
        assert!(scans.incomplete());
        assert_eq!(scans.begin(&Lease).unwrap().finish(Ok(42)), Ok(42));
        assert!(!scans.incomplete());
        for incomplete in [false, true] {
            if incomplete {
                scans.mark_incomplete();
            }
            assert_eq!(
                scans
                    .begin(&Lease)
                    .unwrap()
                    .finish::<()>(Err(NodeAppError::RuntimeRunning)),
                Err(NodeAppError::RuntimeRunning)
            );
            assert_eq!(scans.incomplete(), incomplete);
        }
    }

    #[test]
    fn successful_commit_with_late_cancellation_stays_complete() {
        let scans = Arc::new(ScanControl::default());
        scans.mark_incomplete();
        let activity = scans.begin(&Lease).unwrap();
        let committed = true;
        let pause = scans.pause();
        assert_eq!(activity.finish(Ok(committed)), Err(NodeAppError::Cancelled));
        assert!(committed);
        assert!(!scans.incomplete());
        drop(pause);
        assert!(scans.begin(&Lease).is_some());
    }

    #[test]
    fn a_yield_cancels_the_active_scan_and_holds_new_ones_off_until_dropped() {
        let scans = Arc::new(ScanControl::default());
        let idle = scans.yield_to_operation();
        assert!(!scans.closing());
        assert!(scans.begin(&Lease).is_none());
        drop(idle);
        let activity = scans.begin(&Lease).unwrap();
        let token = activity.token().clone();
        let first = scans.yield_to_operation();
        let second = scans.yield_to_operation();
        assert!(token.is_cancelled());
        // A yield is not a pause: closing stays false, and the scan ends as cancelled.
        assert!(!scans.closing());
        assert!(scans.begin(&Lease).is_none());
        assert_eq!(activity.finish(Ok(())), Err(NodeAppError::Cancelled));
        drop(first);
        assert!(scans.begin(&Lease).is_none());
        drop(second);
        let next = scans.begin(&Lease).unwrap();
        assert!(!next.token().is_cancelled());
    }

    #[test]
    fn the_start_counter_is_odd_while_a_start_is_in_progress_and_only_grows() {
        let scans = ScanControl::default();
        assert_eq!(scans.starts(), 0);
        scans.note_start();
        assert_eq!(scans.starts(), 1);
        scans.start_finished();
        assert_eq!(scans.starts(), 2);
        scans.note_start();
        assert_eq!(scans.starts() % 2, 1);
        scans.start_finished();
        assert_eq!(scans.starts(), 4);
    }

    #[test]
    fn settling_is_reported_while_a_guard_is_held() {
        let scans = Arc::new(ScanControl::default());
        assert!(!scans.settling());
        let first = scans.enter_settling();
        let second = scans.enter_settling();
        assert!(scans.settling());
        drop(first);
        assert!(scans.settling());
        drop(second);
        assert!(!scans.settling());
    }
}
