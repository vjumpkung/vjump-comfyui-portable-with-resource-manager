use crate::shared::model_values::{SafeFileName, Sha256};

use super::{
    error::EnvironmentError,
    pins::DistName,
    version::{ComputeCapability, CudaVariant, CudaVersion, OsKind, PythonVersion},
};

/// The current profile schema; schema 1 remains readable with an empty accelerator list.
pub const PROFILE_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileStatus {
    Stable,
    Experimental,
}

impl ProfileStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Experimental => "experimental",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TorchBuild {
    pub torch: String,
    pub torchvision: String,
    /// `None` when no torchaudio wheel matching this torch release is published.
    pub torchaudio: Option<String>,
    pub cuda: CudaVariant,
    pub index_url: String,
}

impl TorchBuild {
    /// The `name==version` requirements installed from the profile index.
    pub fn requirements(&self) -> Vec<String> {
        let mut requirements = vec![
            format!("torch=={}", self.torch),
            format!("torchvision=={}", self.torchvision),
        ];
        if let Some(torchaudio) = &self.torchaudio {
            requirements.push(format!("torchaudio=={torchaudio}"));
        }
        requirements
    }

    pub fn protected_distributions(&self) -> Vec<DistName> {
        let mut names = vec![dist("torch"), dist("torchvision")];
        if self.torchaudio.is_some() {
            names.push(dist("torchaudio"));
        }
        names
    }
}

fn dist(name: &str) -> DistName {
    DistName::new(name).expect("static distribution names are valid")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TritonSpec {
    pub distribution: DistName,
    /// A PEP 440 specifier such as `==3.8.0` or `>=3.8,<3.9`.
    pub specifier: String,
}

impl TritonSpec {
    pub fn requirement(&self) -> String {
        format!("{}{}", self.distribution, self.specifier)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcceleratorSource {
    Wheel {
        url: String,
        filename: SafeFileName,
        sha256: Sha256,
        size: Option<u64>,
    },
    Packages {
        requirements: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceleratorSpec {
    pub id: String,
    pub name: String,
    pub modules: Vec<String>,
    pub source: AcceleratorSource,
    pub no_deps: bool,
    pub default_selected: bool,
    pub non_fatal: bool,
    pub provenance: String,
}

impl AcceleratorSpec {
    pub fn validate(&self) -> Result<(), EnvironmentError> {
        let invalid = EnvironmentError::InvalidProfile;
        if self.id.is_empty()
            || self.id.len() > 64
            || !self
                .id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            || self.id.starts_with('-')
            || self.id.ends_with('-')
        {
            return Err(invalid("accelerator id"));
        }
        if self.name.trim().is_empty() || self.provenance.trim().is_empty() {
            return Err(invalid("accelerator name or provenance"));
        }
        for module in &self.modules {
            if module.is_empty()
                || !module.split('.').all(|part| {
                    let mut bytes = part.bytes();
                    bytes
                        .next()
                        .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
                        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
                })
            {
                return Err(invalid("accelerator module"));
            }
        }
        match &self.source {
            AcceleratorSource::Wheel {
                url,
                filename,
                size,
                ..
            } => {
                if !url.starts_with("https://")
                    || !filename.as_str().ends_with(".whl")
                    || *size == Some(0)
                {
                    return Err(invalid("accelerator wheel"));
                }
            }
            AcceleratorSource::Packages { requirements } => {
                if requirements.is_empty() || requirements.iter().any(|r| r.trim().is_empty()) {
                    return Err(invalid("accelerator requirements"));
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyExpectations {
    /// The required prefix of `torch.__version__`, for example `2.14.1+cu130`.
    pub torch_version_prefix: String,
    pub require_cuda: bool,
    pub require_triton: bool,
}

/// An estimated peak-space budget. `calibrated` stays false until it is measured on a real install.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiskBudget {
    pub env_bytes: u64,
    pub other_bytes: u64,
    pub margin_bytes: u64,
    pub calibrated: bool,
}

impl DiskBudget {
    pub fn required_on_install_volume(&self, env_on_same_volume: bool) -> u64 {
        let base = self.other_bytes.saturating_add(self.margin_bytes);
        if env_on_same_volume {
            base.saturating_add(self.env_bytes)
        } else {
            base
        }
    }

    pub fn required_on_env_volume(&self) -> u64 {
        self.env_bytes.saturating_add(self.margin_bytes)
    }
}

/// An immutable, versioned setup profile (spec section 5.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupProfile {
    pub id: String,
    pub schema_version: u32,
    pub revision: u32,
    pub label: String,
    pub status: ProfileStatus,
    pub recommended: bool,
    pub os: OsKind,
    pub python: PythonVersion,
    pub torch: TorchBuild,
    pub min_driver_cuda: CudaVersion,
    /// The complete-stack compute-capability floor (Triton).
    pub min_cc: ComputeCapability,
    /// The compute capabilities the torch wheels ship binaries for.
    pub torch_archs: Vec<ComputeCapability>,
    pub triton: TritonSpec,
    pub accelerators: Vec<AcceleratorSpec>,
    /// Extra requirement lines added to every constrained install.
    pub constraints: Vec<String>,
    pub verify: VerifyExpectations,
    pub disk: DiskBudget,
    /// Where each compatibility claim comes from.
    pub provenance: Vec<String>,
}

impl SetupProfile {
    /// Names whose installed versions the pin guard protects for this profile.
    pub fn protected_distributions(&self) -> Vec<DistName> {
        let mut names = self.torch.protected_distributions();
        names.push(self.triton.distribution.clone());
        names
    }

    pub fn validate(&self) -> Result<(), EnvironmentError> {
        let invalid = EnvironmentError::InvalidProfile;
        let simple_id = !self.id.is_empty()
            && self.id.len() <= 64
            && self
                .id
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
        if !simple_id {
            return Err(invalid("id"));
        }
        if self.schema_version != PROFILE_SCHEMA_VERSION && self.schema_version != 1 {
            return Err(invalid("schema version"));
        }
        if self.schema_version == 1 && !self.accelerators.is_empty() {
            return Err(invalid("legacy accelerators are unsupported"));
        }
        if self.label.trim().is_empty() {
            return Err(invalid("label"));
        }
        if !self.python.is_supported() {
            return Err(invalid("python version outside the supported range"));
        }
        if self.torch.torch.is_empty() || self.torch.torchvision.is_empty() {
            return Err(invalid("torch versions"));
        }
        if self.torch.torchaudio.as_deref() == Some("") {
            return Err(invalid("torchaudio version"));
        }
        if !self.torch.index_url.starts_with("https://") {
            return Err(invalid("torch index must use https"));
        }
        if !self.torch.index_url.ends_with(self.torch.cuda.as_str()) {
            return Err(invalid("torch index does not match the CUDA variant"));
        }
        if self.min_driver_cuda < self.torch.cuda.minimum_driver_cuda() {
            return Err(invalid("driver floor below the CUDA variant minimum"));
        }
        if self.torch_archs.is_empty() {
            return Err(invalid("torch architecture list"));
        }
        let expected_triton = match self.os {
            OsKind::Windows => "triton-windows",
            OsKind::Linux => "triton",
        };
        if self.triton.distribution.as_str() != expected_triton {
            return Err(invalid("triton distribution does not match the OS"));
        }
        if self.triton.specifier.is_empty() || self.triton.specifier.contains('*') {
            return Err(invalid("triton specifier"));
        }
        if !self
            .verify
            .torch_version_prefix
            .starts_with(&self.torch.torch)
            || !self
                .verify
                .torch_version_prefix
                .ends_with(self.torch.cuda.as_str())
        {
            return Err(invalid("torch verification prefix"));
        }
        if self.disk.env_bytes == 0 || self.disk.margin_bytes == 0 {
            return Err(invalid("disk budget"));
        }
        if self.provenance.is_empty() {
            return Err(invalid("provenance"));
        }
        let mut ids = std::collections::HashSet::new();
        for accelerator in &self.accelerators {
            accelerator.validate()?;
            if !ids.insert(&accelerator.id) {
                return Err(invalid("duplicate accelerator id"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    use super::*;

    pub fn profile() -> SetupProfile {
        SetupProfile {
            id: "win-py313-torch2141-cu130".into(),
            schema_version: PROFILE_SCHEMA_VERSION,
            revision: 1,
            label: "Python 3.13, PyTorch 2.14.1, CUDA 13.0".into(),
            status: ProfileStatus::Stable,
            recommended: true,
            os: OsKind::Windows,
            python: PythonVersion::new(3, 13, 16),
            torch: TorchBuild {
                torch: "2.14.1".into(),
                torchvision: "0.29.1".into(),
                torchaudio: None,
                cuda: CudaVariant::Cu130,
                index_url: "https://download.pytorch.org/whl/cu130".into(),
            },
            min_driver_cuda: CudaVersion::new(13, 0),
            min_cc: ComputeCapability::new(8, 0),
            torch_archs: [(7, 5), (8, 0), (8, 6), (9, 0), (10, 0), (12, 0)]
                .into_iter()
                .map(|(major, minor)| ComputeCapability::new(major, minor))
                .collect(),
            triton: TritonSpec {
                distribution: DistName::new("triton-windows").unwrap(),
                specifier: ">=3.8,<3.9".into(),
            },
            accelerators: Vec::new(),
            constraints: Vec::new(),
            verify: VerifyExpectations {
                torch_version_prefix: "2.14.1+cu130".into(),
                require_cuda: true,
                require_triton: true,
            },
            disk: DiskBudget {
                env_bytes: 16 << 30,
                other_bytes: 8 << 30,
                margin_bytes: 4 << 30,
                calibrated: false,
            },
            provenance: vec!["test fixture".into()],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{fixtures::profile, *};

    #[test]
    fn fixture_is_valid_and_lists_protected_distributions() {
        let profile = profile();
        profile.validate().unwrap();
        let names: Vec<_> = profile
            .protected_distributions()
            .iter()
            .map(|name| name.to_string())
            .collect();
        assert_eq!(names, ["torch", "torchvision", "triton-windows"]);
        assert_eq!(
            profile.torch.requirements(),
            ["torch==2.14.1", "torchvision==0.29.1"]
        );
        assert_eq!(profile.triton.requirement(), "triton-windows>=3.8,<3.9");
    }

    #[test]
    fn invalid_profiles_are_rejected() {
        type Mutation = fn(&mut SetupProfile);
        let mutations: Vec<(&str, Mutation)> = vec![
            ("id", |p| p.id = "Bad Id".into()),
            ("schema", |p| p.schema_version = 3),
            ("python", |p| p.python = PythonVersion::new(3, 11, 9)),
            ("index", |p| {
                p.torch.index_url = "http://example.com/cu130".into()
            }),
            ("variant", |p| {
                p.torch.index_url = "https://x.test/whl/cu126".into()
            }),
            ("driver", |p| p.min_driver_cuda = CudaVersion::new(12, 9)),
            ("archs", |p| p.torch_archs.clear()),
            ("triton os", |p| p.os = OsKind::Linux),
            ("triton wildcard", |p| p.triton.specifier = "==3.8.*".into()),
            ("prefix", |p| {
                p.verify.torch_version_prefix = "2.10.0+cu130".into()
            }),
            ("disk", |p| p.disk.margin_bytes = 0),
            ("provenance", |p| p.provenance.clear()),
            ("accelerator provenance", |p| {
                p.accelerators.push(AcceleratorSpec {
                    id: "x".into(),
                    name: "x".into(),
                    modules: vec![],
                    source: AcceleratorSource::Packages {
                        requirements: vec!["x".into()],
                    },
                    no_deps: false,
                    default_selected: false,
                    non_fatal: true,
                    provenance: String::new(),
                })
            }),
        ];
        for (case, mutate) in mutations {
            let mut candidate = profile();
            mutate(&mut candidate);
            assert!(candidate.validate().is_err(), "{case} must be rejected");
        }
    }

    #[test]
    fn disk_budget_depends_on_volume_layout() {
        let budget = profile().disk;
        assert_eq!(budget.required_on_install_volume(true), 28 << 30);
        assert_eq!(budget.required_on_install_volume(false), 12 << 30);
        assert_eq!(budget.required_on_env_volume(), 20 << 30);
    }
}
