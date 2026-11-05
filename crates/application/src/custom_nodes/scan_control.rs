use super::NodeAppError;
use crate::shared::{CancelToken, MaintenanceLease};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct State {
    paused: usize,
    active: Option<CancelToken>,
    incomplete: bool,
}

/// Process-local coordination for cooperative inventory observation.
#[derive(Default)]
pub struct ScanControl(Mutex<State>);

impl ScanControl {
    pub fn begin(self: &Arc<Self>, _lease: &dyn MaintenanceLease) -> Option<ScanActivity> {
        let mut state = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if state.paused > 0 {
            return None;
        }
        debug_assert!(
            state.active.is_none(),
            "scan already active under maintenance lease"
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
}
