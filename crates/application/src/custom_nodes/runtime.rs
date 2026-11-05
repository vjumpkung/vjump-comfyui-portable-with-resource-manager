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
        let activity = self.scans().begin(lease);
        let reconciled = self.reconcile_held(lease).await;
        let Some(activity) = activity else {
            reconciled.map_err(after_stop_error)?;
            self.scans().mark_incomplete();
            return Err(RuntimeError::Cancelled);
        };
        let result = async {
            reconciled?;
            self.commit_observation(lease, activity.token()).await
        }
        .await;
        activity
            .finish(result)
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
        NodeAppError::Cancelled => RuntimeError::Cancelled,
        NodeAppError::RuntimeRunning => RuntimeError::NeedsStop,
        _ => RuntimeError::NodeRescanFailed,
    }
}
