use std::{num::NonZeroU32, str::FromStr};

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SettingsError {
    #[error("download concurrency must be positive")]
    InvalidConcurrency,
    #[error("registry cache TTL must be positive")]
    InvalidTtl,
    #[error("bandwidth cap must be positive when set")]
    InvalidBandwidthCap,
    #[error("unknown Hugging Face engine")]
    InvalidHfEngine,
    #[error("unknown close behavior")]
    InvalidCloseBehavior,
    #[error("unknown theme")]
    InvalidTheme,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Concurrency(NonZeroU32);

impl Concurrency {
    pub fn new(value: u32) -> Result<Self, SettingsError> {
        NonZeroU32::new(value)
            .map(Self)
            .ok_or(SettingsError::InvalidConcurrency)
    }

    pub fn get(self) -> u32 {
        self.0.get()
    }
}

impl Default for Concurrency {
    fn default() -> Self {
        Self(NonZeroU32::new(3).unwrap())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TtlHours(NonZeroU32);

impl TtlHours {
    pub fn new(value: u32) -> Result<Self, SettingsError> {
        NonZeroU32::new(value)
            .map(Self)
            .ok_or(SettingsError::InvalidTtl)
    }

    pub fn get(self) -> u32 {
        self.0.get()
    }
}

impl Default for TtlHours {
    fn default() -> Self {
        Self(NonZeroU32::new(6).unwrap())
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum HfEngine {
    #[default]
    HfCli,
    Native,
}

impl HfEngine {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::HfCli => "hf_cli",
            Self::Native => "native",
        }
    }
}

impl FromStr for HfEngine {
    type Err = SettingsError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "hf_cli" => Ok(Self::HfCli),
            "native" => Ok(Self::Native),
            _ => Err(SettingsError::InvalidHfEngine),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum CloseBehavior {
    #[default]
    Ask,
    StopComfy,
    KeepRunningHeadless,
}

impl CloseBehavior {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::StopComfy => "stop_comfy",
            Self::KeepRunningHeadless => "keep_running_headless",
        }
    }
}

impl FromStr for CloseBehavior {
    type Err = SettingsError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "ask" => Ok(Self::Ask),
            "stop_comfy" => Ok(Self::StopComfy),
            "keep_running_headless" => Ok(Self::KeepRunningHeadless),
            _ => Err(SettingsError::InvalidCloseBehavior),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

impl Theme {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

impl FromStr for Theme {
    type Err = SettingsError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "system" => Ok(Self::System),
            "light" => Ok(Self::Light),
            "dark" => Ok(Self::Dark),
            _ => Err(SettingsError::InvalidTheme),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CredentialKind {
    CivitAi,
    HuggingFace,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Settings {
    pub download_concurrency: Concurrency,
    pub hf_engine: HfEngine,
    pub close_behavior: CloseBehavior,
    pub theme: Theme,
    pub registry_cache_ttl: TtlHours,
    pub bandwidth_cap_kib_s: Option<NonZeroU32>,
}

impl Settings {
    pub fn new(
        download_concurrency: u32,
        hf_engine: HfEngine,
        close_behavior: CloseBehavior,
        theme: Theme,
        registry_cache_ttl: u32,
        bandwidth_cap_kib_s: Option<u32>,
    ) -> Result<Self, SettingsError> {
        Ok(Self {
            download_concurrency: Concurrency::new(download_concurrency)?,
            hf_engine,
            close_behavior,
            theme,
            registry_cache_ttl: TtlHours::new(registry_cache_ttl)?,
            bandwidth_cap_kib_s: bandwidth_cap_kib_s
                .map(|value| NonZeroU32::new(value).ok_or(SettingsError::InvalidBandwidthCap))
                .transpose()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_settings_policy() {
        let settings = Settings::default();
        assert_eq!(settings.download_concurrency.get(), 3);
        assert_eq!(settings.hf_engine, HfEngine::HfCli);
        assert_eq!(settings.close_behavior, CloseBehavior::Ask);
        assert_eq!(settings.theme, Theme::System);
        assert_eq!(settings.registry_cache_ttl.get(), 6);
        assert_eq!(settings.bandwidth_cap_kib_s, None);
    }

    #[test]
    fn positive_values_have_no_policy_maximum() {
        assert_eq!(Concurrency::new(0), Err(SettingsError::InvalidConcurrency));
        assert_eq!(TtlHours::new(0), Err(SettingsError::InvalidTtl));
        for value in [1, 100, u32::MAX] {
            assert_eq!(Concurrency::new(value).unwrap().get(), value);
            assert_eq!(TtlHours::new(value).unwrap().get(), value);
        }
        assert_eq!(
            Settings::new(
                1,
                HfEngine::Native,
                CloseBehavior::Ask,
                Theme::Dark,
                1,
                Some(0)
            ),
            Err(SettingsError::InvalidBandwidthCap)
        );
        let settings = Settings::new(
            u32::MAX,
            HfEngine::Native,
            CloseBehavior::StopComfy,
            Theme::Light,
            u32::MAX,
            Some(u32::MAX),
        )
        .unwrap();
        assert_eq!(settings.bandwidth_cap_kib_s.unwrap().get(), u32::MAX);
    }

    #[test]
    fn enum_values_round_trip_and_reject_unknown_values() {
        for value in [HfEngine::HfCli, HfEngine::Native] {
            assert_eq!(value.as_str().parse(), Ok(value));
        }
        for value in [
            CloseBehavior::Ask,
            CloseBehavior::StopComfy,
            CloseBehavior::KeepRunningHeadless,
        ] {
            assert_eq!(value.as_str().parse(), Ok(value));
        }
        for value in [Theme::System, Theme::Light, Theme::Dark] {
            assert_eq!(value.as_str().parse(), Ok(value));
        }
        assert_eq!(
            "unknown".parse::<HfEngine>(),
            Err(SettingsError::InvalidHfEngine)
        );
        assert_eq!(
            "unknown".parse::<CloseBehavior>(),
            Err(SettingsError::InvalidCloseBehavior)
        );
        assert_eq!("unknown".parse::<Theme>(), Err(SettingsError::InvalidTheme));
    }
}
