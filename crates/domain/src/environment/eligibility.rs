use thiserror::Error;

use super::{
    hardware::{GpuProbe, HardwareObservation},
    profile::SetupProfile,
    version::{ComputeCapability, CudaVariant, CudaVersion, OsKind},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskVolume {
    Install,
    Environment,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum IneligibleReason {
    #[error("this profile is for a different operating system")]
    WrongOs { profile: OsKind, host: OsKind },
    #[error("no NVIDIA GPU was detected")]
    NoNvidiaGpu,
    #[error("the GPU probe failed, so compatibility cannot be confirmed")]
    GpuProbeFailed,
    #[error("the NVIDIA driver version could not be read")]
    DriverUnknown,
    #[error("the driver supports CUDA {has} but this profile needs {needs}")]
    DriverTooOld {
        needs: CudaVersion,
        has: CudaVersion,
    },
    #[error("the compute capability of GPU {index} could not be read")]
    ComputeCapabilityUnknown { index: u32 },
    #[error(
        "GPU {index} ({name}) has compute capability {has}, below the {needs} this stack needs"
    )]
    ArchTooOld {
        index: u32,
        name: String,
        has: ComputeCapability,
        needs: ComputeCapability,
    },
    #[error(
        "GPU {index} ({name}) has compute capability {has}, which these PyTorch wheels do not cover"
    )]
    ArchNotInWheels {
        index: u32,
        name: String,
        has: ComputeCapability,
    },
    #[error("the free disk space could not be read")]
    DiskUnknown { volume: DiskVolume },
    #[error("{needs_bytes} bytes are needed but only {has_bytes} are free")]
    InsufficientDisk {
        volume: DiskVolume,
        needs_bytes: u64,
        has_bytes: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Eligibility {
    Eligible,
    Disabled(Vec<IneligibleReason>),
}

impl Eligibility {
    pub fn is_eligible(&self) -> bool {
        matches!(self, Self::Eligible)
    }
}

/// Decides whether `profile` may be offered on `hardware`. Every detected GPU must be
/// compatible, because no device-selection mechanism exists yet. A CPU-only profile skips the
/// GPU and driver checks; only the OS and disk checks apply.
pub fn evaluate(profile: &SetupProfile, hardware: &HardwareObservation) -> Eligibility {
    let mut reasons = Vec::new();
    if profile.os != hardware.os {
        reasons.push(IneligibleReason::WrongOs {
            profile: profile.os,
            host: hardware.os,
        });
    }
    if profile.torch.cuda != CudaVariant::Cpu {
        match &hardware.gpus {
            GpuProbe::NoNvidiaGpu => reasons.push(IneligibleReason::NoNvidiaGpu),
            GpuProbe::Unavailable => reasons.push(IneligibleReason::GpuProbeFailed),
            GpuProbe::Detected(gpus) if gpus.is_empty() => {
                reasons.push(IneligibleReason::NoNvidiaGpu)
            }
            GpuProbe::Detected(gpus) => {
                for gpu in gpus {
                    match gpu.compute_capability {
                        None => reasons
                            .push(IneligibleReason::ComputeCapabilityUnknown { index: gpu.index }),
                        Some(capability) if capability < profile.min_cc => {
                            reasons.push(IneligibleReason::ArchTooOld {
                                index: gpu.index,
                                name: gpu.name.clone(),
                                has: capability,
                                needs: profile.min_cc,
                            })
                        }
                        Some(capability)
                            if !profile
                                .torch_archs
                                .iter()
                                .any(|arch| capability.runs_binaries_for(*arch)) =>
                        {
                            reasons.push(IneligibleReason::ArchNotInWheels {
                                index: gpu.index,
                                name: gpu.name.clone(),
                                has: capability,
                            })
                        }
                        Some(_) => {}
                    }
                }
            }
        }
        if !matches!(hardware.gpus, GpuProbe::NoNvidiaGpu | GpuProbe::Unavailable) {
            match hardware.driver_cuda {
                None => reasons.push(IneligibleReason::DriverUnknown),
                Some(has) if has < profile.min_driver_cuda => {
                    reasons.push(IneligibleReason::DriverTooOld {
                        needs: profile.min_driver_cuda,
                        has,
                    })
                }
                Some(_) => {}
            }
        }
    }
    let same_volume = !hardware.env_on_separate_volume;
    check_disk(
        &mut reasons,
        DiskVolume::Install,
        hardware.install_volume_free,
        profile.disk.required_on_install_volume(same_volume),
    );
    if hardware.env_on_separate_volume {
        check_disk(
            &mut reasons,
            DiskVolume::Environment,
            hardware.env_volume_free,
            profile.disk.required_on_env_volume(),
        );
    }
    if reasons.is_empty() {
        Eligibility::Eligible
    } else {
        Eligibility::Disabled(reasons)
    }
}

fn check_disk(
    reasons: &mut Vec<IneligibleReason>,
    volume: DiskVolume,
    free: Option<u64>,
    needs_bytes: u64,
) {
    match free {
        None => reasons.push(IneligibleReason::DiskUnknown { volume }),
        Some(has_bytes) if has_bytes < needs_bytes => {
            reasons.push(IneligibleReason::InsufficientDisk {
                volume,
                needs_bytes,
                has_bytes,
            })
        }
        Some(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::environment::{
        hardware::GpuObservation,
        profile::fixtures::{cpu_profile, profile},
        version::{ComputeCapability, CudaVersion},
    };

    fn gpu(index: u32, major: u8, minor: u8) -> GpuObservation {
        GpuObservation {
            index,
            name: format!("GPU {index}"),
            compute_capability: Some(ComputeCapability::new(major, minor)),
            vram_mib: Some(12288),
        }
    }

    fn hardware(gpus: GpuProbe) -> HardwareObservation {
        HardwareObservation {
            gpus,
            driver_version: Some("591.86".into()),
            driver_cuda: Some(CudaVersion::new(13, 1)),
            os: OsKind::Windows,
            os_version: "Windows 11".into(),
            install_volume_free: Some(200 << 30),
            env_volume_free: None,
            env_on_separate_volume: false,
            long_paths_enabled: Some(true),
        }
    }

    fn reasons(eligibility: Eligibility) -> Vec<IneligibleReason> {
        match eligibility {
            Eligibility::Disabled(reasons) => reasons,
            Eligibility::Eligible => Vec::new(),
        }
    }

    #[test]
    fn rtx_3060_with_a_current_driver_is_eligible() {
        let hardware = hardware(GpuProbe::Detected(vec![gpu(0, 8, 6)]));
        assert!(evaluate(&profile(), &hardware).is_eligible());
    }

    #[test]
    fn architecture_boundaries_follow_the_stack_floor_and_wheel_coverage() {
        let mut cases = vec![
            (7, 5, false),
            (7, 9, false),
            (8, 0, true),
            (8, 9, true),
            (9, 0, true),
            (10, 3, true),
            (12, 1, true),
            (13, 0, false),
        ];
        // Blackwell is not covered by wheels built only up to Hopper.
        let mut hopper_only = profile();
        hopper_only.torch_archs.retain(|arch| arch.major <= 9);
        for (major, minor, expected) in cases.drain(..) {
            let observation = hardware(GpuProbe::Detected(vec![gpu(0, major, minor)]));
            assert_eq!(
                evaluate(&profile(), &observation).is_eligible(),
                expected,
                "{major}.{minor}"
            );
        }
        let blackwell = hardware(GpuProbe::Detected(vec![gpu(0, 12, 0)]));
        assert!(matches!(
            reasons(evaluate(&hopper_only, &blackwell))[..],
            [IneligibleReason::ArchNotInWheels { .. }]
        ));
    }

    #[test]
    fn driver_boundary_is_inclusive() {
        let mut observation = hardware(GpuProbe::Detected(vec![gpu(0, 8, 6)]));
        observation.driver_cuda = Some(CudaVersion::new(13, 0));
        assert!(evaluate(&profile(), &observation).is_eligible());
        observation.driver_cuda = Some(CudaVersion::new(12, 9));
        assert_eq!(
            reasons(evaluate(&profile(), &observation)),
            [IneligibleReason::DriverTooOld {
                needs: CudaVersion::new(13, 0),
                has: CudaVersion::new(12, 9)
            }]
        );
    }

    #[test]
    fn unknown_observations_disable_instead_of_guessing() {
        let mut observation = hardware(GpuProbe::Unavailable);
        assert_eq!(
            reasons(evaluate(&profile(), &observation)),
            [IneligibleReason::GpuProbeFailed]
        );
        observation.gpus = GpuProbe::NoNvidiaGpu;
        assert_eq!(
            reasons(evaluate(&profile(), &observation)),
            [IneligibleReason::NoNvidiaGpu]
        );
        let mut observation = hardware(GpuProbe::Detected(vec![GpuObservation {
            compute_capability: None,
            ..gpu(0, 8, 6)
        }]));
        observation.driver_cuda = None;
        observation.install_volume_free = None;
        assert_eq!(
            reasons(evaluate(&profile(), &observation)),
            [
                IneligibleReason::ComputeCapabilityUnknown { index: 0 },
                IneligibleReason::DriverUnknown,
                IneligibleReason::DiskUnknown {
                    volume: DiskVolume::Install
                }
            ]
        );
    }

    #[test]
    fn every_gpu_must_be_compatible() {
        let observation = hardware(GpuProbe::Detected(vec![gpu(0, 8, 6), gpu(1, 6, 1)]));
        let reasons = reasons(evaluate(&profile(), &observation));
        assert_eq!(reasons.len(), 1);
        assert!(matches!(
            reasons[0],
            IneligibleReason::ArchTooOld { index: 1, .. }
        ));
    }

    #[test]
    fn cpu_profile_ignores_the_gpu_and_driver() {
        let cpu = cpu_profile();
        let old_gpu = hardware(GpuProbe::Detected(vec![gpu(0, 6, 1)]));
        let mut no_driver = old_gpu.clone();
        no_driver.driver_cuda = None;
        for observation in [
            hardware(GpuProbe::NoNvidiaGpu),
            hardware(GpuProbe::Unavailable),
            old_gpu,
            no_driver,
        ] {
            assert!(
                evaluate(&cpu, &observation).is_eligible(),
                "{:?}",
                observation.gpus
            );
        }
        let mut linux_host = hardware(GpuProbe::NoNvidiaGpu);
        linux_host.os = OsKind::Linux;
        assert_eq!(
            reasons(evaluate(&cpu, &linux_host)),
            [IneligibleReason::WrongOs {
                profile: OsKind::Windows,
                host: OsKind::Linux
            }]
        );
        let mut low_disk = hardware(GpuProbe::NoNvidiaGpu);
        low_disk.install_volume_free = Some(1 << 30);
        assert!(matches!(
            reasons(evaluate(&cpu, &low_disk))[..],
            [IneligibleReason::InsufficientDisk {
                volume: DiskVolume::Install,
                ..
            }]
        ));
    }

    #[test]
    fn wrong_os_and_disk_shortfalls_are_reported_per_volume() {
        let mut observation = hardware(GpuProbe::Detected(vec![gpu(0, 8, 6)]));
        observation.os = OsKind::Linux;
        observation.install_volume_free = Some(1 << 30);
        observation.env_on_separate_volume = true;
        observation.env_volume_free = Some(1 << 30);
        let reasons = reasons(evaluate(&profile(), &observation));
        assert!(matches!(reasons[0], IneligibleReason::WrongOs { .. }));
        assert!(reasons.iter().any(|reason| matches!(
            reason,
            IneligibleReason::InsufficientDisk {
                volume: DiskVolume::Install,
                ..
            }
        )));
        assert!(reasons.iter().any(|reason| matches!(
            reason,
            IneligibleReason::InsufficientDisk {
                volume: DiskVolume::Environment,
                ..
            }
        )));
    }
}
