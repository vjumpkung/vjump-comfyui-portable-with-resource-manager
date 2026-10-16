//! The bundled setup profiles: versioned JSON documents (spec section 5.4), validated into
//! domain `SetupProfile` values.

use application::environment::ProfileCatalog;
use domain::environment::{
    AcceleratorSpec, ComputeCapability, CudaVariant, CudaVersion, DiskBudget, DistName,
    EnvironmentError, OsKind, ProfileStatus, PythonVersion, SetupProfile, TorchBuild, TritonSpec,
    VerifyExpectations,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TorchDoc {
    pub torch: String,
    pub torchvision: String,
    pub torchaudio: Option<String>,
    pub cuda: String,
    pub index_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TritonDoc {
    pub package: String,
    pub specifier: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceleratorDoc {
    pub name: String,
    pub requirement: String,
    pub sha256: Option<String>,
    pub non_fatal: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifyDoc {
    pub torch_version_prefix: String,
    pub require_cuda: bool,
    pub require_triton: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiskDoc {
    pub env_bytes: u64,
    pub other_bytes: u64,
    pub margin_bytes: u64,
    pub calibrated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileDoc {
    pub schema_version: u32,
    pub id: String,
    pub revision: u32,
    pub label: String,
    pub status: String,
    pub recommended: bool,
    pub os: String,
    pub python: String,
    pub torch: TorchDoc,
    pub min_driver_cuda: String,
    pub min_cc: String,
    pub torch_archs: Vec<String>,
    pub triton: TritonDoc,
    pub accelerators: Vec<AcceleratorDoc>,
    pub constraints: Vec<String>,
    pub verify: VerifyDoc,
    pub disk: DiskDoc,
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProfileError {
    #[error("a bundled profile is not valid JSON for this schema")]
    Parse,
    #[error(transparent)]
    Invalid(#[from] EnvironmentError),
    #[error("duplicate profile id")]
    Duplicate,
}

impl TryFrom<ProfileDoc> for SetupProfile {
    type Error = ProfileError;

    fn try_from(doc: ProfileDoc) -> Result<Self, Self::Error> {
        let status = match doc.status.as_str() {
            "stable" => ProfileStatus::Stable,
            "experimental" => ProfileStatus::Experimental,
            _ => return Err(EnvironmentError::InvalidProfile("status").into()),
        };
        let profile = SetupProfile {
            id: doc.id,
            schema_version: doc.schema_version,
            revision: doc.revision,
            label: doc.label,
            status,
            recommended: doc.recommended,
            os: doc.os.parse()?,
            python: doc.python.parse::<PythonVersion>()?,
            torch: TorchBuild {
                torch: doc.torch.torch,
                torchvision: doc.torch.torchvision,
                torchaudio: doc.torch.torchaudio,
                cuda: doc.torch.cuda.parse::<CudaVariant>()?,
                index_url: doc.torch.index_url,
            },
            min_driver_cuda: doc.min_driver_cuda.parse::<CudaVersion>()?,
            min_cc: doc.min_cc.parse::<ComputeCapability>()?,
            torch_archs: doc
                .torch_archs
                .iter()
                .map(|arch| arch.parse::<ComputeCapability>())
                .collect::<Result<_, _>>()?,
            triton: TritonSpec {
                distribution: DistName::new(&doc.triton.package)?,
                specifier: doc.triton.specifier,
            },
            accelerators: doc
                .accelerators
                .into_iter()
                .map(|accelerator| AcceleratorSpec {
                    name: accelerator.name,
                    requirement: accelerator.requirement,
                    sha256: accelerator.sha256,
                    non_fatal: accelerator.non_fatal,
                })
                .collect(),
            constraints: doc.constraints,
            verify: VerifyExpectations {
                torch_version_prefix: doc.verify.torch_version_prefix,
                require_cuda: doc.verify.require_cuda,
                require_triton: doc.verify.require_triton,
            },
            disk: DiskBudget {
                env_bytes: doc.disk.env_bytes,
                other_bytes: doc.disk.other_bytes,
                margin_bytes: doc.disk.margin_bytes,
                calibrated: doc.disk.calibrated,
            },
            provenance: doc.provenance,
        };
        profile.validate()?;
        Ok(profile)
    }
}

impl From<&SetupProfile> for ProfileDoc {
    fn from(profile: &SetupProfile) -> Self {
        Self {
            schema_version: profile.schema_version,
            id: profile.id.clone(),
            revision: profile.revision,
            label: profile.label.clone(),
            status: profile.status.as_str().to_owned(),
            recommended: profile.recommended,
            os: profile.os.as_str().to_owned(),
            python: profile.python.to_string(),
            torch: TorchDoc {
                torch: profile.torch.torch.clone(),
                torchvision: profile.torch.torchvision.clone(),
                torchaudio: profile.torch.torchaudio.clone(),
                cuda: profile.torch.cuda.as_str().to_owned(),
                index_url: profile.torch.index_url.clone(),
            },
            min_driver_cuda: profile.min_driver_cuda.to_string(),
            min_cc: profile.min_cc.to_string(),
            torch_archs: profile
                .torch_archs
                .iter()
                .map(ToString::to_string)
                .collect(),
            triton: TritonDoc {
                package: profile.triton.distribution.to_string(),
                specifier: profile.triton.specifier.clone(),
            },
            accelerators: profile
                .accelerators
                .iter()
                .map(|accelerator| AcceleratorDoc {
                    name: accelerator.name.clone(),
                    requirement: accelerator.requirement.clone(),
                    sha256: accelerator.sha256.clone(),
                    non_fatal: accelerator.non_fatal,
                })
                .collect(),
            constraints: profile.constraints.clone(),
            verify: VerifyDoc {
                torch_version_prefix: profile.verify.torch_version_prefix.clone(),
                require_cuda: profile.verify.require_cuda,
                require_triton: profile.verify.require_triton,
            },
            disk: DiskDoc {
                env_bytes: profile.disk.env_bytes,
                other_bytes: profile.disk.other_bytes,
                margin_bytes: profile.disk.margin_bytes,
                calibrated: profile.disk.calibrated,
            },
            provenance: profile.provenance.clone(),
        }
    }
}

pub fn parse_profile(json: &str) -> Result<SetupProfile, ProfileError> {
    let doc: ProfileDoc = serde_json::from_str(json).map_err(|_| ProfileError::Parse)?;
    SetupProfile::try_from(doc)
}

const BUNDLED: [&str; 7] = [
    include_str!("data/profiles/win-py313-torch2141-cu130.json"),
    include_str!("data/profiles/win-py313-torch2141-cu126.json"),
    include_str!("data/profiles/win-py314-torch2141-cu130.json"),
    include_str!("data/profiles/win-py312-torch2100-cu130.json"),
    include_str!("data/profiles/linux-py313-torch2141-cu130.json"),
    include_str!("data/profiles/linux-py313-torch2141-cu126.json"),
    include_str!("data/profiles/linux-py314-torch2141-cu130.json"),
];

#[derive(Debug, Clone)]
pub struct BundledProfiles {
    profiles: Vec<SetupProfile>,
}

impl BundledProfiles {
    pub fn load() -> Result<Self, ProfileError> {
        let mut profiles: Vec<SetupProfile> = Vec::new();
        for json in BUNDLED {
            let profile = parse_profile(json)?;
            if profiles.iter().any(|existing| existing.id == profile.id) {
                return Err(ProfileError::Duplicate);
            }
            profiles.push(profile);
        }
        Ok(Self { profiles })
    }
}

impl ProfileCatalog for BundledProfiles {
    fn list(&self) -> Vec<SetupProfile> {
        self.profiles.clone()
    }

    fn get(&self, id: &str) -> Option<SetupProfile> {
        self.profiles
            .iter()
            .find(|profile| profile.id == id)
            .cloned()
    }
}

/// The host OS this build runs on.
pub fn host_os() -> OsKind {
    if cfg!(windows) {
        OsKind::Windows
    } else {
        OsKind::Linux
    }
}

#[cfg(test)]
mod tests;
