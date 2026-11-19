use async_trait::async_trait;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum GateError {
    #[error("another maintenance operation is running")]
    Busy,
    #[error("the maintenance gate is unavailable")]
    Unavailable,
}

/// Proof that the holder owns the maintenance gate. Dropping the lease releases the gate, so
/// owners keep it alive through cancellation, process cleanup and filesystem cleanup.
pub trait MaintenanceLease: Send + Sync {
    fn retained(&self) -> super::Retained;
}

/// The single scheduler that serializes maintenance operations (spec section 3.3.3).
/// Composed workflows acquire it once; there is no recursive locking.
#[async_trait]
pub trait MaintenanceGate: Send + Sync {
    /// Waits until the gate is free.
    async fn acquire(&self) -> Result<Box<dyn MaintenanceLease>, GateError>;
    /// Fails with [`GateError::Busy`] instead of waiting.
    async fn try_acquire(&self) -> Result<Box<dyn MaintenanceLease>, GateError>;
    /// Waits at most `wait` for the gate, then fails with [`GateError::Busy`].
    async fn acquire_within(
        &self,
        wait: std::time::Duration,
    ) -> Result<Box<dyn MaintenanceLease>, GateError>;
}

/// The maintenance lease plus the node lock; subprocess survivors retain both.
///
/// Lock order: the maintenance gate comes first, then the node lock. Anything that holds the
/// node lock only ever `try_acquire`s the maintenance gate. Blocking or bounded node-lock
/// acquisitions happen only while holding the maintenance gate, or in scans that hold nothing
/// else, so no wait cycle can form (not even with a fair mutex).
pub struct WithNodeLock<'a> {
    pub lease: &'a mut dyn MaintenanceLease,
    pub nodes: Box<dyn MaintenanceLease>,
}

impl MaintenanceLease for WithNodeLock<'_> {
    fn retained(&self) -> super::Retained {
        self.lease.retained().merge(self.nodes.retained())
    }
}
