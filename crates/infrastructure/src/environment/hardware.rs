//! Hardware detection: NVIDIA GPUs through NVML (falling back to `nvidia-smi`), free disk space
//! of the relevant volumes, the OS version and, on Windows, whether long paths are enabled.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use application::environment::HardwareProbe;
use async_trait::async_trait;
use domain::environment::{
    ComputeCapability, CudaVersion, GpuObservation, GpuProbe, HardwareObservation, OsKind,
};
use sysinfo::Disks;

use super::{
    exec::run_quiet,
    layout::{Layout, Tool, command},
    python::normalize_path,
};
use crate::shared::{fs::nearest_existing, process::RunLimits};

pub struct SystemHardware;

impl SystemHardware {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SystemHardware {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GpuReading {
    pub driver_version: Option<String>,
    pub driver_cuda: Option<CudaVersion>,
    pub gpus: Vec<GpuObservation>,
}

/// `nvidia-smi --query-gpu=index,name,driver_version,compute_cap,memory.total
/// --format=csv,noheader,nounits` lines.
pub fn parse_gpu_csv(text: &str) -> GpuReading {
    let mut reading = GpuReading::default();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let fields: Vec<&str> = line.split(',').map(str::trim).collect();
        if fields.len() < 5 {
            continue;
        }
        let Ok(index) = fields[0].parse::<u32>() else {
            continue;
        };
        reading
            .driver_version
            .get_or_insert_with(|| fields[2].to_owned());
        reading.gpus.push(GpuObservation {
            index,
            name: fields[1].to_owned(),
            compute_capability: fields[3].parse::<ComputeCapability>().ok(),
            vram_mib: fields[4].parse().ok(),
        });
    }
    reading
}

/// The `CUDA Version: 13.1` field of the plain `nvidia-smi` header.
pub fn parse_cuda_header(text: &str) -> Option<CudaVersion> {
    let after = text.split("CUDA Version:").nth(1)?;
    after
        .split_whitespace()
        .next()
        .and_then(|version| version.parse().ok())
}

fn read_nvml() -> Result<GpuReading, nvml_wrapper::error::NvmlError> {
    let nvml = nvml_wrapper::Nvml::init()?;
    let driver_version = nvml.sys_driver_version().ok();
    let driver_cuda = nvml.sys_cuda_driver_version().ok().and_then(|version| {
        let major = nvml_wrapper::cuda_driver_version_major(version);
        let minor = nvml_wrapper::cuda_driver_version_minor(version);
        Some(CudaVersion::new(
            u16::try_from(major).ok()?,
            u16::try_from(minor).ok()?,
        ))
    });
    let count = nvml.device_count()?;
    let mut gpus = Vec::new();
    for index in 0..count {
        let device = nvml.device_by_index(index)?;
        gpus.push(GpuObservation {
            index,
            name: device.name().unwrap_or_else(|_| format!("GPU {index}")),
            compute_capability: device
                .cuda_compute_capability()
                .ok()
                .and_then(|capability| {
                    Some(ComputeCapability::new(
                        u8::try_from(capability.major).ok()?,
                        u8::try_from(capability.minor).ok()?,
                    ))
                }),
            vram_mib: device.memory_info().ok().map(|memory| memory.total >> 20),
        });
    }
    Ok(GpuReading {
        driver_version,
        driver_cuda,
        gpus,
    })
}

fn smi_candidates() -> Vec<PathBuf> {
    if cfg!(windows) {
        let root = std::env::var_os("SystemRoot")
            .map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
        let mut candidates = vec![root.join(r"System32\nvidia-smi.exe")];
        if let Some(program_files) = std::env::var_os("ProgramFiles") {
            candidates.push(
                PathBuf::from(program_files).join(r"NVIDIA Corporation\NVSMI\nvidia-smi.exe"),
            );
        }
        candidates
    } else {
        [
            "/usr/bin/nvidia-smi",
            "/usr/local/bin/nvidia-smi",
            "/bin/nvidia-smi",
        ]
        .iter()
        .map(PathBuf::from)
        .collect()
    }
}

enum SmiOutcome {
    Reading(GpuReading),
    /// `nvidia-smi` does not exist: no NVIDIA driver is installed.
    Missing,
    /// It exists but did not give a usable answer.
    Failed,
}

async fn read_smi(layout: &Layout) -> SmiOutcome {
    let Some(program) = smi_candidates().into_iter().find(|path| path.is_file()) else {
        return SmiOutcome::Missing;
    };
    let cancel = application::shared::CancelToken::new();
    let limits = RunLimits {
        timeout: Some(Duration::from_secs(20)),
        ..RunLimits::default()
    };
    let query = command(layout, Tool::Plain, &program).args([
        "--query-gpu=index,name,driver_version,compute_cap,memory.total",
        "--format=csv,noheader,nounits",
    ]);
    let Ok(output) = run_quiet(&query, &cancel, &limits).await else {
        return SmiOutcome::Failed;
    };
    let mut reading = parse_gpu_csv(&output.stdout);
    if let Ok(header) = run_quiet(&command(layout, Tool::Plain, &program), &cancel, &limits).await {
        reading.driver_cuda = parse_cuda_header(&header.stdout);
    }
    SmiOutcome::Reading(reading)
}

/// One volume: its mount point and the space available to the current user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Volume {
    pub mount: PathBuf,
    pub available: u64,
}

fn comparable(path: &Path) -> Vec<String> {
    path.components()
        .map(|component| {
            let text = component.as_os_str().to_string_lossy().into_owned();
            if cfg!(windows) {
                text.to_lowercase()
            } else {
                text
            }
        })
        .collect()
}

/// The volume with the longest mount point that contains `path`.
pub fn volume_of<'a>(path: &Path, volumes: &'a [Volume]) -> Option<&'a Volume> {
    let wanted = comparable(path);
    volumes
        .iter()
        .filter(|volume| {
            let mount = comparable(&volume.mount);
            mount.len() <= wanted.len() && wanted[..mount.len()] == mount[..]
        })
        .max_by_key(|volume| comparable(&volume.mount).len())
}

fn system_volumes() -> Vec<Volume> {
    Disks::new_with_refreshed_list()
        .list()
        .iter()
        .map(|disk| Volume {
            mount: disk.mount_point().to_owned(),
            available: disk.available_space(),
        })
        .collect()
}

/// The volume that will hold `path`: its nearest existing ancestor decides.
fn volume_for(path: &Path, volumes: &[Volume]) -> Option<Volume> {
    let anchor = normalize_path(nearest_existing(path).unwrap_or(path));
    volume_of(&anchor, volumes).cloned()
}

#[cfg(windows)]
fn long_paths_enabled() -> Option<bool> {
    use windows_sys::Win32::System::Registry::{
        HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RegGetValueW,
    };
    let key: Vec<u16> = r"SYSTEM\CurrentControlSet\Control\FileSystem"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let name: Vec<u16> = "LongPathsEnabled".encode_utf16().chain(Some(0)).collect();
    let mut value: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: both strings are NUL terminated, and `value`/`size` outlive the call.
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&raw mut value).cast(),
            &raw mut size,
        )
    };
    match status {
        0 => Some(value != 0),
        // A missing value means the default: long paths disabled.
        2 => Some(false),
        _ => None,
    }
}

#[cfg(not(windows))]
fn long_paths_enabled() -> Option<bool> {
    None
}

fn os_version() -> String {
    sysinfo::System::long_os_version()
        .or_else(sysinfo::System::os_version)
        .unwrap_or_else(|| "unknown".to_owned())
}

#[async_trait]
impl HardwareProbe for SystemHardware {
    async fn probe(&self, install_root: &Path, env_path: Option<&Path>) -> HardwareObservation {
        let layout = Layout::new(install_root);
        let nvml = tokio::task::spawn_blocking(read_nvml).await.ok();
        let (reading, gpus_state) = match nvml {
            Some(Ok(reading)) => {
                let state = if reading.gpus.is_empty() {
                    GpuState::None
                } else {
                    GpuState::Found
                };
                (reading, state)
            }
            _ => match read_smi(&layout).await {
                SmiOutcome::Reading(reading) if !reading.gpus.is_empty() => {
                    (reading, GpuState::Found)
                }
                SmiOutcome::Reading(reading) => (reading, GpuState::None),
                SmiOutcome::Missing => (GpuReading::default(), GpuState::None),
                SmiOutcome::Failed => (GpuReading::default(), GpuState::Unavailable),
            },
        };
        let gpus = match gpus_state {
            GpuState::Found => GpuProbe::Detected(reading.gpus.clone()),
            GpuState::None => GpuProbe::NoNvidiaGpu,
            GpuState::Unavailable => GpuProbe::Unavailable,
        };

        let install_root = install_root.to_owned();
        let env_path = env_path.map(Path::to_owned);
        let (install_free, env_free, separate) = tokio::task::spawn_blocking(move || {
            let volumes = system_volumes();
            let install = volume_for(&install_root, &volumes);
            let env = env_path
                .as_deref()
                .and_then(|path| volume_for(path, &volumes));
            let separate = match (&install, &env) {
                (Some(install), Some(env)) => install.mount != env.mount,
                (None, Some(_)) => true,
                _ => false,
            };
            (
                install.map(|volume| volume.available),
                env.filter(|_| separate).map(|volume| volume.available),
                separate,
            )
        })
        .await
        .unwrap_or((None, None, false));

        HardwareObservation {
            gpus,
            driver_version: reading.driver_version,
            driver_cuda: reading.driver_cuda,
            os: if cfg!(windows) {
                OsKind::Windows
            } else {
                OsKind::Linux
            },
            os_version: os_version(),
            install_volume_free: install_free,
            env_volume_free: env_free,
            env_on_separate_volume: separate,
            long_paths_enabled: long_paths_enabled(),
        }
    }
}

enum GpuState {
    Found,
    None,
    Unavailable,
}

#[cfg(test)]
mod tests;
