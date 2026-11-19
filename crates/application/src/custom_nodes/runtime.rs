use super::{NodeAppError, NodeService};
use crate::{
    comfy_runtime::{NodeLifecycle, RuntimeError},
    shared::MaintenanceLease,
};
use async_trait::async_trait;

#[async_trait]
impl NodeLifecycle for NodeService {
    async fn before_start(&self, lease: &mut dyn MaintenanceLease) -> Result<(), RuntimeError> {
        self.reconcile_held(lease).await.map_err(runtime_error)
    }

    async fn reconcile_after_stop(
        &self,
        lease: &mut dyn MaintenanceLease,
    ) -> Result<(), RuntimeError> {
        self.reconcile_held(lease).await.map_err(runtime_error)
    }

    fn note_start(&self) {
        self.scans().note_start();
    }

    fn start_finished(&self) {
        self.scans().start_finished();
    }

    /// The service cannot spawn work (the application layer has no executor); the desktop wrapper
    /// schedules the scan.
    fn request_scan(&self) {}

    fn scans_held_off(&self) -> bool {
        self.scans().closing()
    }

    fn scan_settling(&self) -> bool {
        self.scans().settling()
    }
}

fn runtime_error(error: NodeAppError) -> RuntimeError {
    match error {
        NodeAppError::RuntimeRunning => RuntimeError::NeedsStop,
        NodeAppError::Busy => RuntimeError::Busy,
        _ => RuntimeError::NodeRecoveryRequired,
    }
}
