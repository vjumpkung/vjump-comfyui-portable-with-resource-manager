//! Path coordination shared by model acquisition and library mutations (spec section 3.3.3).
use async_trait::async_trait;
use domain::models::library::ModelLocation;
use thiserror::Error;

/// Shared by acquisition and library mutations. Dropping the lease releases it.
pub trait ModelPathLease: Send + Sync {}

/// Single-key lease used by download publication. Keys use the form `root_id:lowercase/path`.
#[async_trait]
pub trait ModelPathLocks: Send + Sync {
    async fn acquire(&self, key: &str) -> Box<dyn ModelPathLease>;
}

/// Proof that every requested location is held. Dropping the lease releases all of them.
pub trait PathLease: Send {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PathLockError {
    /// A durable destination reservation of another job owns one of the locations.
    #[error("another operation or download owns one of the paths")]
    BusyReserved,
    #[error("path coordination is unavailable")]
    Unavailable,
}

/// Acquires all locations at once: aliases resolve to one physical key, keys are deduplicated
/// and taken in a fixed order, so concurrent callers cannot deadlock.
#[async_trait]
pub trait PathLocks: Send + Sync {
    async fn acquire_many(
        &self,
        locations: Vec<ModelLocation>,
    ) -> Result<Box<dyn PathLease>, PathLockError>;
}

/// The durable destination reservations of unfinished download jobs.
#[async_trait]
pub trait DestinationReservations: Send + Sync {
    /// `key` is a download destination reservation key (`root_id:lowercase/path`).
    async fn is_reserved(&self, key: &str) -> bool;
}
