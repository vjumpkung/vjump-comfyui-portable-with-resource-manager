use std::sync::Arc;
pub mod credential_tester;
pub mod diagnostics;
pub mod updates;

use async_trait::async_trait;
use domain::settings::{CredentialKind, Settings, SettingsError};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SettingsRepoError {
    #[error("settings storage is unavailable")]
    Unavailable,
    #[error("invalid persisted settings: {0}")]
    InvalidPersisted(String),
}

#[async_trait]
pub trait SettingsRepository: Send + Sync {
    async fn load(&self) -> Result<Option<Settings>, SettingsRepoError>;
    async fn save(&self, settings: &Settings) -> Result<(), SettingsRepoError>;
}

/// An opaque process-local reference, never a credential value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PendingCredentialRef(u64);

impl PendingCredentialRef {
    pub fn new(id: u64) -> Self {
        Self(id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialPresence {
    Set,
    NotSet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialBackend {
    Keyring,
    SessionMemory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CredentialStoreError {
    #[error("credential storage is unavailable")]
    Unavailable,
    #[error("credential storage is locked")]
    Locked,
    #[error("pending credential is unknown or already consumed")]
    UnknownPending,
    #[error("pending credential kind does not match")]
    KindMismatch,
    #[error("durable credential could not be deleted")]
    DeleteFailed,
}

#[async_trait]
pub trait CredentialStore: Send + Sync {
    async fn commit(
        &self,
        kind: CredentialKind,
        pending: PendingCredentialRef,
    ) -> Result<(), CredentialStoreError>;
    async fn clear(&self, kind: CredentialKind) -> Result<(), CredentialStoreError>;
    async fn status(
        &self,
        kind: CredentialKind,
    ) -> Result<CredentialPresence, CredentialStoreError>;
    fn backend(&self) -> CredentialBackend;
}

#[derive(Debug, Error)]
pub enum SettingsAppError {
    #[error(transparent)]
    Validation(#[from] SettingsError),
    #[error(transparent)]
    Repository(#[from] SettingsRepoError),
    #[error(transparent)]
    Credential(#[from] CredentialStoreError),
}

pub struct GetSettings {
    repository: Arc<dyn SettingsRepository + Send + Sync>,
}

impl GetSettings {
    pub fn new(repository: Arc<dyn SettingsRepository + Send + Sync>) -> Self {
        Self { repository }
    }

    pub async fn execute(&self) -> Result<Settings, SettingsAppError> {
        Ok(self.repository.load().await?.unwrap_or_default())
    }
}

#[derive(Debug, Clone)]
pub struct UpdateSettingsInput {
    pub download_concurrency: u32,
    pub hf_engine: String,
    pub close_behavior: String,
    pub theme: String,
    pub language: String,
    pub registry_cache_ttl: u32,
    pub bandwidth_cap_kib_s: Option<u32>,
}

pub struct UpdateSettings {
    repository: Arc<dyn SettingsRepository + Send + Sync>,
}

impl UpdateSettings {
    pub fn new(repository: Arc<dyn SettingsRepository + Send + Sync>) -> Self {
        Self { repository }
    }

    pub async fn execute(&self, input: UpdateSettingsInput) -> Result<Settings, SettingsAppError> {
        let settings = Settings::new(
            input.download_concurrency,
            input.hf_engine.parse()?,
            input.close_behavior.parse()?,
            input.theme.parse()?,
            input.language.parse()?,
            input.registry_cache_ttl,
            input.bandwidth_cap_kib_s,
        )?;
        self.repository.save(&settings).await?;
        Ok(settings)
    }
}

pub struct SetCredential {
    store: Arc<dyn CredentialStore + Send + Sync>,
}

impl SetCredential {
    pub fn new(store: Arc<dyn CredentialStore + Send + Sync>) -> Self {
        Self { store }
    }

    pub async fn execute(
        &self,
        kind: CredentialKind,
        pending: PendingCredentialRef,
    ) -> Result<(), SettingsAppError> {
        Ok(self.store.commit(kind, pending).await?)
    }
}

pub struct ClearCredential {
    store: Arc<dyn CredentialStore + Send + Sync>,
}

impl ClearCredential {
    pub fn new(store: Arc<dyn CredentialStore + Send + Sync>) -> Self {
        Self { store }
    }

    pub async fn execute(&self, kind: CredentialKind) -> Result<(), SettingsAppError> {
        Ok(self.store.clear(kind).await?)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CredentialStatus {
    pub kind: CredentialKind,
    pub presence: CredentialPresence,
    pub backend: CredentialBackend,
    pub session_only_warning: bool,
}

pub struct GetCredentialStatus {
    store: Arc<dyn CredentialStore + Send + Sync>,
}

impl GetCredentialStatus {
    pub fn new(store: Arc<dyn CredentialStore + Send + Sync>) -> Self {
        Self { store }
    }

    pub async fn execute(
        &self,
        kind: CredentialKind,
    ) -> Result<CredentialStatus, SettingsAppError> {
        let presence = self.store.status(kind).await?;
        let backend = self.store.backend();
        Ok(CredentialStatus {
            kind,
            presence,
            backend,
            session_only_warning: backend == CredentialBackend::SessionMemory,
        })
    }
}

#[cfg(test)]
mod tests;
