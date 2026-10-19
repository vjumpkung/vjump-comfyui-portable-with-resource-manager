use std::sync::Arc;

use application::shared::{GateError, MaintenanceGate, MaintenanceLease, Retained};
use async_trait::async_trait;
use tokio::sync::{Mutex, OwnedMutexGuard};

/// The maintenance gate: one Tokio mutex shared by every maintenance workflow. A lease owns
/// the guard, so the gate stays held until the lease is dropped.
#[derive(Clone, Default)]
pub struct TokioMaintenanceGate {
    mutex: Arc<Mutex<()>>,
}

impl TokioMaintenanceGate {
    pub fn new() -> Self {
        Self::default()
    }
}

struct TokioLease {
    guard: Arc<OwnedMutexGuard<()>>,
}

impl MaintenanceLease for TokioLease {
    fn retained(&self) -> Retained {
        Retained::new(self.guard.clone())
    }
}

#[async_trait]
impl MaintenanceGate for TokioMaintenanceGate {
    async fn acquire(&self) -> Result<Box<dyn MaintenanceLease>, GateError> {
        let guard = self.mutex.clone().lock_owned().await;
        Ok(Box::new(TokioLease {
            guard: Arc::new(guard),
        }))
    }

    async fn try_acquire(&self) -> Result<Box<dyn MaintenanceLease>, GateError> {
        match self.mutex.clone().try_lock_owned() {
            Ok(guard) => Ok(Box::new(TokioLease {
                guard: Arc::new(guard),
            })),
            Err(_) => Err(GateError::Busy),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn the_gate_is_exclusive_until_the_lease_is_dropped() {
        let gate = TokioMaintenanceGate::new();
        let lease = gate.acquire().await.unwrap();
        assert_eq!(gate.try_acquire().await.err(), Some(GateError::Busy));

        let waiter = {
            let gate = gate.clone();
            tokio::spawn(async move { gate.acquire().await.map(|_| ()) })
        };
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            !waiter.is_finished(),
            "acquire waits while the lease is held"
        );
        drop(lease);
        waiter.await.unwrap().unwrap();
        assert!(gate.try_acquire().await.is_ok());
    }
}
