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
}
