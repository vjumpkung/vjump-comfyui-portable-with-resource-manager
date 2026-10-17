use super::{NodeAppError, NodeService};
use crate::{
    comfy_runtime::{NodeLifecycle, RuntimeError},
    shared::MaintenanceLease,
};
use async_trait::async_trait;

#[async_trait]
impl NodeLifecycle for NodeService {
    async fn before_start(&self, lease: &mut dyn MaintenanceLease) -> Result<(), RuntimeError> {
        self.reconcile_held(lease)
            .await
            .map_err(before_start_error)?;
        self.ensure_reconciled().await.map_err(before_start_error)
    }

    async fn after_stop(&self, lease: &mut dyn MaintenanceLease) -> Result<(), RuntimeError> {
        self.reconcile_held(lease).await.map_err(after_stop_error)?;
        self.scan_held(lease)
            .await
            .map(|_| ())
            .map_err(after_stop_error)
    }
}

fn before_start_error(error: NodeAppError) -> RuntimeError {
    match error {
        NodeAppError::RuntimeRunning => RuntimeError::NeedsStop,
        _ => RuntimeError::NodeRecoveryRequired,
    }
}

fn after_stop_error(error: NodeAppError) -> RuntimeError {
    match error {
        NodeAppError::RuntimeRunning => RuntimeError::NeedsStop,
        _ => RuntimeError::NodeRescanFailed,
    }
}
