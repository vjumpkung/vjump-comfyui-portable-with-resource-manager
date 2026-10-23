use std::{fmt, str::FromStr};

use super::error::EnvironmentError;

/// An exact CPython version. Profiles must pin the patch level, so `3.12.x` is not valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PythonVersion {
    pub major: u8,
    pub minor: u8,
    pub patch: u8,
}

impl PythonVersion {
    pub const fn new(major: u8, minor: u8, patch: u8) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    pub fn same_minor(self, other: Self) -> bool {
        self.major == other.major && self.minor == other.minor
    }

    /// The Python range the app supports for ComfyUI (3.12 through 3.14).
    pub fn is_supported(self) -> bool {
        self.major == 3 && (12..=14).contains(&self.minor)
    }
}

impl fmt::Display for PythonVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl FromStr for PythonVersion {
    type Err = EnvironmentError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let invalid = || EnvironmentError::InvalidPythonVersion(value.chars().take(32).collect());
        let mut parts = value.split('.');
        let mut next = || -> Result<u8, EnvironmentError> {
            let part = parts.next().ok_or_else(invalid)?;
            if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(invalid());
            }
            part.parse().map_err(|_| invalid())
        };
        let version = Self::new(next()?, next()?, next()?);
        if parts.next().is_some() {
            return Err(invalid());
        }
        Ok(version)
    }
}

/// A CUDA compute capability such as `8.6`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ComputeCapability {
    pub major: u8,
    pub minor: u8,
}

impl ComputeCapability {
    pub const fn new(major: u8, minor: u8) -> Self {
        Self { major, minor }
    }

    /// CUDA binaries built for `arch` run on a device of the same major generation whose
    /// minor revision is not lower.
    pub fn runs_binaries_for(self, arch: Self) -> bool {
        self.major == arch.major && self.minor >= arch.minor
    }
}

impl fmt::Display for ComputeCapability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

impl FromStr for ComputeCapability {
    type Err = EnvironmentError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (major, minor) = value
            .split_once('.')
            .ok_or(EnvironmentError::InvalidComputeCapability)?;
        Ok(Self::new(
            major
                .parse()
                .map_err(|_| EnvironmentError::InvalidComputeCapability)?,
            minor
                .parse()
                .map_err(|_| EnvironmentError::InvalidComputeCapability)?,
        ))
    }
}

/// A CUDA toolkit or driver-supported CUDA version such as `13.1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CudaVersion {
    pub major: u16,
    pub minor: u16,
}

impl CudaVersion {
    pub const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }

    /// Decodes the integer form reported by the CUDA driver API (`major * 1000 + minor * 10`).
    pub fn from_driver_int(value: i32) -> Option<Self> {
        if value <= 0 {
            return None;
        }
        Some(Self::new(
            u16::try_from(value / 1000).ok()?,
            u16::try_from((value % 1000) / 10).ok()?,
        ))
    }
}

impl fmt::Display for CudaVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

impl FromStr for CudaVersion {
    type Err = EnvironmentError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (major, minor) = value
            .split_once('.')
            .ok_or(EnvironmentError::InvalidCudaVersion)?;
        Ok(Self::new(
            major
                .parse()
                .map_err(|_| EnvironmentError::InvalidCudaVersion)?,
            minor
                .parse()
                .map_err(|_| EnvironmentError::InvalidCudaVersion)?,
        ))
    }
}

/// The compute build of a PyTorch wheel set: a CUDA version or CPU only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CudaVariant {
    Cu126,
    Cu130,
    Cpu,
}

impl CudaVariant {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cu126 => "cu126",
            Self::Cu130 => "cu130",
            Self::Cpu => "cpu",
        }
    }

    /// The driver-supported CUDA version below which the wheels cannot run, or `None` for a
    /// CPU-only build, which needs no driver.
    pub fn minimum_driver_cuda(self) -> Option<CudaVersion> {
        match self {
            Self::Cu126 => Some(CudaVersion::new(12, 6)),
            Self::Cu130 => Some(CudaVersion::new(13, 0)),
            Self::Cpu => None,
        }
    }
}

impl FromStr for CudaVariant {
    type Err = EnvironmentError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "cu126" => Ok(Self::Cu126),
            "cu130" => Ok(Self::Cu130),
            "cpu" => Ok(Self::Cpu),
            _ => Err(EnvironmentError::InvalidCudaVariant),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OsKind {
    Windows,
    Linux,
}

impl OsKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Linux => "linux",
        }
    }
}

impl FromStr for OsKind {
    type Err = EnvironmentError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "windows" => Ok(Self::Windows),
            "linux" => Ok(Self::Linux),
            _ => Err(EnvironmentError::InvalidOs),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_profiles_require_an_exact_patch_and_enforce_the_supported_minors() {
        let pinned: PythonVersion = "3.13.16".parse().unwrap();
        assert_eq!(pinned.to_string(), "3.13.16");
        assert!(pinned.is_supported());
        assert!(pinned.same_minor(PythonVersion::new(3, 13, 0)));
        assert!(!pinned.same_minor(PythonVersion::new(3, 12, 16)));
        for value in ["3.12.0", "3.14.8"] {
            assert!(value.parse::<PythonVersion>().unwrap().is_supported());
        }
        for value in ["2.13.0", "3.11.9", "3.15.0", "4.13.0"] {
            assert!(!value.parse::<PythonVersion>().unwrap().is_supported());
        }
        for value in [
            "",
            "3",
            "3.13",
            "3.13.x",
            "3.13.16.1",
            "3..16",
            "3.13.-1",
            "3.13.256",
            " 3.13.16",
            "3.13.16rc1",
        ] {
            assert!(value.parse::<PythonVersion>().is_err(), "{value}");
        }
    }

    #[test]
    fn cuda_driver_encoding_and_architecture_compatibility_keep_their_boundaries() {
        assert_eq!(
            CudaVersion::from_driver_int(13_010),
            Some(CudaVersion::new(13, 1))
        );
        assert_eq!(
            CudaVersion::from_driver_int(12_060),
            Some(CudaVersion::new(12, 6))
        );
        assert_eq!(CudaVersion::from_driver_int(0), None);
        assert_eq!(CudaVersion::from_driver_int(-1), None);
        assert_eq!(CudaVersion::from_driver_int(i32::MAX), None);
        let device: ComputeCapability = "8.6".parse().unwrap();
        assert_eq!(device.to_string(), "8.6");
        assert!(device.runs_binaries_for(ComputeCapability::new(8, 0)));
        assert!(!ComputeCapability::new(8, 0).runs_binaries_for(device));
        assert!(!ComputeCapability::new(9, 0).runs_binaries_for(device));
        let cuda: CudaVersion = "13.1".parse().unwrap();
        assert_eq!(cuda.to_string(), "13.1");
        for invalid in ["", "13", "a.1", "13.a", "13.1.0", "-1.0", "256.0"] {
            assert!(invalid.parse::<ComputeCapability>().is_err(), "{invalid}");
        }
        for invalid in ["", "13", "a.1", "13.a", "13.1.0", "-1.0", "65536.0"] {
            assert!(invalid.parse::<CudaVersion>().is_err(), "{invalid}");
        }
        assert_eq!(
            "cu126"
                .parse::<CudaVariant>()
                .unwrap()
                .minimum_driver_cuda(),
            Some(CudaVersion::new(12, 6))
        );
        assert_eq!(
            "cu130"
                .parse::<CudaVariant>()
                .unwrap()
                .minimum_driver_cuda(),
            Some(CudaVersion::new(13, 0))
        );
        let cpu: CudaVariant = "cpu".parse().unwrap();
        assert_eq!(cpu, CudaVariant::Cpu);
        assert_eq!(cpu.as_str(), "cpu");
        assert_eq!(cpu.minimum_driver_cuda(), None);
        assert_eq!(CudaVariant::Cu126.as_str(), "cu126");
        assert!("cu129".parse::<CudaVariant>().is_err());
        assert!("macos".parse::<OsKind>().is_err());
        assert_eq!("windows".parse::<OsKind>().unwrap().as_str(), "windows");
        assert_eq!("linux".parse::<OsKind>().unwrap().as_str(), "linux");
    }
}
