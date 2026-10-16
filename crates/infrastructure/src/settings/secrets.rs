use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use application::settings::{
    CredentialBackend, CredentialPresence, CredentialStore, CredentialStoreError,
    PendingCredentialRef,
};
use async_trait::async_trait;
use domain::settings::CredentialKind;

use crate::shared::logging::register_secret;

struct StagedCredential {
    kind: CredentialKind,
    value: String,
}

#[derive(Clone, Default)]
pub struct SecretIngress {
    pending: Arc<Mutex<HashMap<PendingCredentialRef, StagedCredential>>>,
}

impl SecretIngress {
    pub fn stage(&self, kind: CredentialKind, value: String) -> PendingCredentialLease {
        // Register before ID allocation, locking, or any backend operation can fail.
        register_secret(&value);
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        let id = NEXT_ID
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .expect("pending credential identifiers exhausted");
        let reference = PendingCredentialRef::new(id);
        self.pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(reference, StagedCredential { kind, value });
        PendingCredentialLease {
            ingress: self.clone(),
            reference,
        }
    }

    fn take(
        &self,
        kind: CredentialKind,
        reference: PendingCredentialRef,
    ) -> Result<String, CredentialStoreError> {
        let staged = self
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&reference)
            .ok_or(CredentialStoreError::UnknownPending)?;
        if staged.kind != kind {
            return Err(CredentialStoreError::KindMismatch);
        }
        Ok(staged.value)
    }
}

pub struct PendingCredentialLease {
    ingress: SecretIngress,
    reference: PendingCredentialRef,
}

impl PendingCredentialLease {
    pub fn reference(&self) -> PendingCredentialRef {
        self.reference
    }
}

impl Drop for PendingCredentialLease {
    fn drop(&mut self) {
        self.ingress
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&self.reference);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BackendError {
    Unavailable,
    Locked,
    Failed,
}

trait KeyringBackend: Send + Sync {
    fn probe(&self) -> Result<(), BackendError>;
    fn set(&self, kind: CredentialKind, value: &str) -> Result<(), BackendError>;
    fn presence(&self, kind: CredentialKind) -> Result<CredentialPresence, BackendError>;
    fn delete(&self, kind: CredentialKind) -> Result<(), BackendError>;
    fn read(&self, kind: CredentialKind) -> Result<Option<String>, BackendError>;
}

struct SystemKeyring;

const SERVICE: &str = "vjumpkung-comfyui";

fn entry(kind: CredentialKind) -> Result<keyring::Entry, BackendError> {
    let user = match kind {
        CredentialKind::CivitAi => "civitai",
        CredentialKind::HuggingFace => "huggingface",
    };
    keyring::Entry::new(SERVICE, user).map_err(|error| backend_error(&error))
}

fn backend_error(error: &keyring::Error) -> BackendError {
    match error {
        keyring::Error::NoStorageAccess(_) => BackendError::Locked,
        keyring::Error::NoDefaultStore | keyring::Error::PlatformFailure(_) => {
            BackendError::Unavailable
        }
        _ => BackendError::Failed,
    }
}

impl KeyringBackend for SystemKeyring {
    fn read(&self, kind: CredentialKind) -> Result<Option<String>, BackendError> {
        match entry(kind)?.get_password() {
            Ok(value) => {
                register_secret(&value);
                Ok(Some(value))
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(backend_error(&error)),
        }
    }
    fn probe(&self) -> Result<(), BackendError> {
        keyring::Entry::store_status()
            .as_ref()
            .map(|_| ())
            .map_err(backend_error)
    }

    fn set(&self, kind: CredentialKind, value: &str) -> Result<(), BackendError> {
        entry(kind)?
            .set_password(value)
            .map_err(|error| backend_error(&error))
    }

    fn presence(&self, kind: CredentialKind) -> Result<CredentialPresence, BackendError> {
        match entry(kind)?.get_password() {
            Ok(value) => {
                register_secret(&value);
                Ok(CredentialPresence::Set)
            }
            Err(keyring::Error::NoEntry) => Ok(CredentialPresence::NotSet),
            Err(error) => Err(backend_error(&error)),
        }
    }

    fn delete(&self, kind: CredentialKind) -> Result<(), BackendError> {
        match entry(kind)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(backend_error(&error)),
        }
    }
}

struct StoreState {
    backend: CredentialBackend,
    memory: HashMap<CredentialKind, String>,
    durable_possible: bool,
    generations: HashMap<CredentialKind, u64>,
}

pub struct KeyringCredentialStore {
    ingress: SecretIngress,
    keyring: Arc<dyn KeyringBackend>,
    state: Arc<Mutex<StoreState>>,
}

impl KeyringCredentialStore {
    pub async fn new(ingress: SecretIngress) -> Result<Self, CredentialStoreError> {
        Self::with_backend(ingress, Arc::new(SystemKeyring)).await
    }

    async fn with_backend(
        ingress: SecretIngress,
        keyring: Arc<dyn KeyringBackend>,
    ) -> Result<Self, CredentialStoreError> {
        let probe = keyring.clone();
        let result = tokio::task::spawn_blocking(move || probe.probe())
            .await
            .map_err(|_| CredentialStoreError::Unavailable)?;
        let available = matches!(result, Ok(()) | Err(BackendError::Locked));
        Ok(Self {
            ingress,
            keyring,
            state: Arc::new(Mutex::new(StoreState {
                backend: if available {
                    CredentialBackend::Keyring
                } else {
                    CredentialBackend::SessionMemory
                },
                memory: HashMap::new(),
                durable_possible: available,
                generations: HashMap::new(),
            })),
        })
    }
}

#[async_trait]
impl CredentialStore for KeyringCredentialStore {
    async fn commit(
        &self,
        kind: CredentialKind,
        pending: PendingCredentialRef,
    ) -> Result<(), CredentialStoreError> {
        let value = self.ingress.take(kind, pending)?;
        let keyring = self.keyring.clone();
        let state = self.state.clone();
        tokio::task::spawn_blocking(move || {
            let mut state = state
                .lock()
                .map_err(|_| CredentialStoreError::Unavailable)?;
            if state.backend == CredentialBackend::SessionMemory {
                state.memory.insert(kind, value);
                *state.generations.entry(kind).or_default() += 1;
                return Ok(());
            }
            match keyring.set(kind, &value) {
                Ok(()) => {
                    state.memory.remove(&kind);
                    state.durable_possible = true;
                    *state.generations.entry(kind).or_default() += 1;
                    Ok(())
                }
                Err(BackendError::Unavailable) => {
                    state.backend = CredentialBackend::SessionMemory;
                    state.memory.insert(kind, value);
                    *state.generations.entry(kind).or_default() += 1;
                    Ok(())
                }
                Err(BackendError::Locked) => Err(CredentialStoreError::Locked),
                Err(BackendError::Failed) => Err(CredentialStoreError::Unavailable),
            }
        })
        .await
        .map_err(|_| CredentialStoreError::Unavailable)?
    }

    async fn clear(&self, kind: CredentialKind) -> Result<(), CredentialStoreError> {
        let keyring = self.keyring.clone();
        let state = self.state.clone();
        tokio::task::spawn_blocking(move || {
            let mut state = state
                .lock()
                .map_err(|_| CredentialStoreError::Unavailable)?;
            state.memory.remove(&kind);
            *state.generations.entry(kind).or_default() += 1;
            if state.durable_possible
                && let Err(error) = keyring.delete(kind)
            {
                if error == BackendError::Unavailable {
                    state.backend = CredentialBackend::SessionMemory;
                }
                return Err(CredentialStoreError::DeleteFailed);
            }
            Ok(())
        })
        .await
        .map_err(|_| CredentialStoreError::DeleteFailed)?
    }

    async fn status(
        &self,
        kind: CredentialKind,
    ) -> Result<CredentialPresence, CredentialStoreError> {
        let keyring = self.keyring.clone();
        let state = self.state.clone();
        tokio::task::spawn_blocking(move || {
            let mut state = state
                .lock()
                .map_err(|_| CredentialStoreError::Unavailable)?;
            if state.backend == CredentialBackend::SessionMemory {
                return Ok(if state.memory.contains_key(&kind) {
                    CredentialPresence::Set
                } else {
                    CredentialPresence::NotSet
                });
            }
            match keyring.presence(kind) {
                Ok(presence) => Ok(presence),
                Err(BackendError::Locked) => Err(CredentialStoreError::Locked),
                Err(BackendError::Unavailable) => {
                    state.backend = CredentialBackend::SessionMemory;
                    Err(CredentialStoreError::Unavailable)
                }
                Err(BackendError::Failed) => Err(CredentialStoreError::Unavailable),
            }
        })
        .await
        .map_err(|_| CredentialStoreError::Unavailable)?
    }

    fn backend(&self) -> CredentialBackend {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .backend
    }
}

/// Infrastructure-only secret wrapper. Deliberately has no Debug implementation.
pub struct SecretValue(String);
impl SecretValue {
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}
pub struct CredentialSnapshot {
    pub generation: u64,
    pub value: Option<SecretValue>,
}
#[async_trait]
pub trait CredentialVault: Send + Sync {
    async fn read(&self, kind: CredentialKind) -> Result<CredentialSnapshot, CredentialStoreError>;
    async fn generation(&self, kind: CredentialKind) -> Result<u64, CredentialStoreError>;
}
#[async_trait]
impl CredentialVault for KeyringCredentialStore {
    async fn read(&self, kind: CredentialKind) -> Result<CredentialSnapshot, CredentialStoreError> {
        let keyring = self.keyring.clone();
        let state = self.state.clone();
        tokio::task::spawn_blocking(move || {
            let state = state
                .lock()
                .map_err(|_| CredentialStoreError::Unavailable)?;
            let generation = *state.generations.get(&kind).unwrap_or(&0);
            let value = if state.backend == CredentialBackend::SessionMemory {
                state.memory.get(&kind).cloned()
            } else {
                keyring.read(kind).map_err(|e| match e {
                    BackendError::Locked => CredentialStoreError::Locked,
                    _ => CredentialStoreError::Unavailable,
                })?
            };
            if let Some(value) = &value {
                register_secret(value);
            }
            Ok(CredentialSnapshot {
                generation,
                value: value.map(SecretValue),
            })
        })
        .await
        .map_err(|_| CredentialStoreError::Unavailable)?
    }
    async fn generation(&self, kind: CredentialKind) -> Result<u64, CredentialStoreError> {
        Ok(*self
            .state
            .lock()
            .map_err(|_| CredentialStoreError::Unavailable)?
            .generations
            .get(&kind)
            .unwrap_or(&0))
    }
}

#[cfg(test)]
mod tests;
