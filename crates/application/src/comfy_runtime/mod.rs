//! Runtime workflows and machine-facing ports. The M1 maintenance contract remains intact.

mod close;
mod model;
mod ports;
mod update_workflow;
mod use_cases;
pub use close::*;
pub use model::*;
pub use ports::*;
pub use update_workflow::*;
pub use use_cases::*;

#[cfg(test)]
mod tests;

use std::path::Path;

use async_trait::async_trait;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum RuntimeBlock {
    #[error("ComfyUI from this installation is running")]
    Running,
    #[error("the state of ComfyUI could not be determined")]
    CannotAssess,
}

/// Refuses maintenance while the installation's ComfyUI may be running. M2 adds the
/// owned-runtime stop implementation behind the same port.
#[async_trait]
pub trait RuntimeMaintenance: Send + Sync {
    async fn ensure_stopped(&self, install_root: &Path) -> Result<(), RuntimeBlock>;
}
