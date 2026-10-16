use application::settings::{CredentialBackend, CredentialPresence, CredentialStatus};
use domain::settings::{CloseBehavior, CredentialKind, HfEngine, Settings, Theme};
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum HfEngineDto {
    HfCli,
    Native,
}

impl From<HfEngine> for HfEngineDto {
    fn from(value: HfEngine) -> Self {
        match value {
            HfEngine::HfCli => Self::HfCli,
            HfEngine::Native => Self::Native,
        }
    }
}

impl From<HfEngineDto> for HfEngine {
    fn from(value: HfEngineDto) -> Self {
        match value {
            HfEngineDto::HfCli => Self::HfCli,
            HfEngineDto::Native => Self::Native,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CloseBehaviorDto {
    Ask,
    StopComfy,
    KeepRunningHeadless,
}

impl From<CloseBehavior> for CloseBehaviorDto {
    fn from(value: CloseBehavior) -> Self {
        match value {
            CloseBehavior::Ask => Self::Ask,
            CloseBehavior::StopComfy => Self::StopComfy,
            CloseBehavior::KeepRunningHeadless => Self::KeepRunningHeadless,
        }
    }
}

impl From<CloseBehaviorDto> for CloseBehavior {
    fn from(value: CloseBehaviorDto) -> Self {
        match value {
            CloseBehaviorDto::Ask => Self::Ask,
            CloseBehaviorDto::StopComfy => Self::StopComfy,
            CloseBehaviorDto::KeepRunningHeadless => Self::KeepRunningHeadless,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ThemeDto {
    System,
    Light,
    Dark,
}

impl From<Theme> for ThemeDto {
    fn from(value: Theme) -> Self {
        match value {
            Theme::System => Self::System,
            Theme::Light => Self::Light,
            Theme::Dark => Self::Dark,
        }
    }
}

impl From<ThemeDto> for Theme {
    fn from(value: ThemeDto) -> Self {
        match value {
            ThemeDto::System => Self::System,
            ThemeDto::Light => Self::Light,
            ThemeDto::Dark => Self::Dark,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum CredentialKindDto {
    CivitAi,
    HuggingFace,
}

impl From<CredentialKind> for CredentialKindDto {
    fn from(value: CredentialKind) -> Self {
        match value {
            CredentialKind::CivitAi => Self::CivitAi,
            CredentialKind::HuggingFace => Self::HuggingFace,
        }
    }
}

impl From<CredentialKindDto> for CredentialKind {
    fn from(value: CredentialKindDto) -> Self {
        match value {
            CredentialKindDto::CivitAi => Self::CivitAi,
            CredentialKindDto::HuggingFace => Self::HuggingFace,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CredentialBackendDto {
    Keyring,
    SessionMemory,
}

impl From<CredentialBackend> for CredentialBackendDto {
    fn from(value: CredentialBackend) -> Self {
        match value {
            CredentialBackend::Keyring => Self::Keyring,
            CredentialBackend::SessionMemory => Self::SessionMemory,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    pub download_concurrency: u32,
    pub hf_engine: HfEngineDto,
    pub close_behavior: CloseBehaviorDto,
    pub theme: ThemeDto,
    pub registry_cache_ttl: u32,
    pub bandwidth_cap_kib_s: Option<u32>,
}

impl From<&Settings> for SettingsDto {
    fn from(value: &Settings) -> Self {
        Self {
            download_concurrency: value.download_concurrency.get(),
            hf_engine: value.hf_engine.into(),
            close_behavior: value.close_behavior.into(),
            theme: value.theme.into(),
            registry_cache_ttl: value.registry_cache_ttl.get(),
            bandwidth_cap_kib_s: value.bandwidth_cap_kib_s.map(|cap| cap.get()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSettingsDto {
    pub download_concurrency: u32,
    pub hf_engine: HfEngineDto,
    pub close_behavior: CloseBehaviorDto,
    pub theme: ThemeDto,
    pub registry_cache_ttl: u32,
    pub bandwidth_cap_kib_s: Option<u32>,
}

/// Secret-bearing input. Deliberately has no `Debug` implementation.
#[derive(Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SecretSetInput {
    pub kind: CredentialKindDto,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CredentialStatusDto {
    pub kind: CredentialKindDto,
    pub is_set: bool,
    pub backend: CredentialBackendDto,
    pub session_only_warning: bool,
    /// The last successful or failed token test, when one exists for the current token.
    pub last_test: Option<super::download_dto::CredentialTestDto>,
}

impl From<CredentialStatus> for CredentialStatusDto {
    fn from(value: CredentialStatus) -> Self {
        Self {
            kind: value.kind.into(),
            is_set: value.presence == CredentialPresence::Set,
            backend: value.backend.into(),
            session_only_warning: value.session_only_warning,
            last_test: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppInfoDto {
    pub version: String,
    pub data_dir: String,
    pub credential_backend: CredentialBackendDto,
}
