//! The environment context: setup profiles, hardware observations, Python environments and
//! the pin set that protects the PyTorch stack.

mod eligibility;
mod env;
mod error;
mod hardware;
mod pins;
mod profile;
mod version;

pub use eligibility::{DiskVolume, Eligibility, IneligibleReason, evaluate};
pub use env::{
    EnvId, EnvState, NewEnvironment, Ownership, PythonEnvironment, PythonSourceKind, SetupMode,
    check_activation,
};
pub use error::EnvironmentError;
pub use hardware::{GpuObservation, GpuProbe, HardwareObservation};
pub use pins::{DistName, PinChange, PinSet};
pub use profile::{
    AcceleratorSource, AcceleratorSpec, DiskBudget, PROFILE_SCHEMA_VERSION, ProfileStatus,
    SetupProfile, TorchBuild, TritonSpec, VerifyExpectations,
};
pub use version::{ComputeCapability, CudaVariant, CudaVersion, OsKind, PythonVersion};
