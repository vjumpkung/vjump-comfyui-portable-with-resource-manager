use super::version::{ComputeCapability, CudaVersion, OsKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuObservation {
    pub index: u32,
    pub name: String,
    /// `None` when the probe could not read the compute capability.
    pub compute_capability: Option<ComputeCapability>,
    pub vram_mib: Option<u64>,
}

/// The outcome of the GPU probe. A failed probe is not the same as "no NVIDIA GPU".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpuProbe {
    Detected(Vec<GpuObservation>),
    NoNvidiaGpu,
    Unavailable,
}

/// An immutable observation of the machine, taken before profiles are offered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HardwareObservation {
    pub gpus: GpuProbe,
    pub driver_version: Option<String>,
    /// The highest CUDA version the installed driver supports.
    pub driver_cuda: Option<CudaVersion>,
    pub os: OsKind,
    pub os_version: String,
    /// Free bytes on the volume that holds the install root.
    pub install_volume_free: Option<u64>,
    /// Free bytes on a separate environment volume; `None` means the same volume as the install root.
    pub env_volume_free: Option<u64>,
    /// True when the environment lives on a different volume than the install root.
    pub env_on_separate_volume: bool,
    /// Windows long-path support; `None` on Linux or when unreadable.
    pub long_paths_enabled: Option<bool>,
}
