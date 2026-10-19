//! The environment context: setup and re-setup workflows, environment queries and package
//! management. Ports describe everything that touches the machine; adapters live in
//! `infrastructure`.

mod activation;
mod deps;
mod error;
mod execute;
mod model;
mod packages;
mod ports;
mod queries;
mod relocation;
mod setup;

pub use activation::{ReconcileReport, ReconcileStartup};
pub use deps::{ActiveRuns, EnvironmentDeps, PlanStore, same_location};
pub use error::EnvironmentAppError;
pub use execute::{ExecuteSetup, RunOutcome};
pub use model::*;
pub use packages::{
    ApplyOutcome, ApplyPackageChange, PackageOperation, PackagePlan, PlanId, PlanResolution,
    PreviewPackageChange,
};
pub use ports::*;
pub use queries::{
    DeleteEnvironment, DetectHardware, EnvironmentInfo, GetEnvironmentInfo, GetSetupStatus,
    ListPackages, ListProfiles, ProfileOffer, RecoveryReason, SelectInstallRoot, SetupPhase,
    SetupStatus, ValidatePython, summarize,
};
pub use relocation::*;
pub use setup::{CancelSetup, SetupAction, StartSetup};

#[cfg(test)]
pub(crate) mod tests;
