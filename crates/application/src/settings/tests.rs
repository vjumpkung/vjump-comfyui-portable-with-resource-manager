use std::sync::Mutex;

use super::*;
use crate::test_support::block_on;

#[derive(Default)]
struct FakeRepository {
    settings: Mutex<Option<Settings>>,
    saves: Mutex<usize>,
    error: Option<SettingsRepoError>,
}

#[async_trait]
impl SettingsRepository for FakeRepository {
    async fn load(&self) -> Result<Option<Settings>, SettingsRepoError> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        Ok(self.settings.lock().unwrap().clone())
    }

    async fn save(&self, settings: &Settings) -> Result<(), SettingsRepoError> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        *self.saves.lock().unwrap() += 1;
        *self.settings.lock().unwrap() = Some(settings.clone());
        Ok(())
    }
}

fn input() -> UpdateSettingsInput {
    UpdateSettingsInput {
        download_concurrency: 12,
        hf_engine: "native".into(),
        close_behavior: "keep_running_headless".into(),
        theme: "dark".into(),
        language: "th".into(),
        registry_cache_ttl: 24,
        bandwidth_cap_kib_s: Some(512),
    }
}

#[test]
fn settings_default_load_and_one_call_save() {
    block_on(async {
        let repository = Arc::new(FakeRepository::default());
        let get = GetSettings::new(repository.clone());
        assert_eq!(get.execute().await.unwrap(), Settings::default());
        let updated = UpdateSettings::new(repository.clone())
            .execute(input())
            .await
            .unwrap();
        assert_eq!(get.execute().await.unwrap(), updated);
        assert_eq!(*repository.saves.lock().unwrap(), 1);
    });
}

#[test]
fn invalid_inputs_do_not_save() {
    block_on(async {
        let repository = Arc::new(FakeRepository::default());
        let update = UpdateSettings::new(repository.clone());
        for field in 0..7 {
            let mut invalid = input();
            match field {
                0 => invalid.download_concurrency = 0,
                1 => invalid.registry_cache_ttl = 0,
                2 => invalid.bandwidth_cap_kib_s = Some(0),
                3 => invalid.hf_engine = "unknown".into(),
                4 => invalid.close_behavior = "unknown".into(),
                5 => invalid.theme = "unknown".into(),
                _ => invalid.language = "unknown".into(),
            }
            assert!(matches!(
                update.execute(invalid).await,
                Err(SettingsAppError::Validation(_))
            ));
        }
        assert_eq!(*repository.saves.lock().unwrap(), 0);
    });
}

#[test]
fn language_round_trips_through_settings_repository() {
    block_on(async {
        let repository = Arc::new(FakeRepository::default());
        let update = UpdateSettings::new(repository.clone());
        let get = GetSettings::new(repository.clone());
        for language in [
            domain::settings::Language::En,
            domain::settings::Language::Th,
        ] {
            let mut input = input();
            input.language = language.as_str().into();
            let updated = update.execute(input).await.unwrap();
            assert_eq!(updated.language, language);
            assert_eq!(get.execute().await.unwrap(), updated);
        }
        assert_eq!(*repository.saves.lock().unwrap(), 2);
    });
}

#[test]
fn repository_errors_propagate_without_resetting_to_defaults() {
    block_on(async {
        for error in [
            SettingsRepoError::Unavailable,
            SettingsRepoError::InvalidPersisted("invalid theme".into()),
        ] {
            let repository = Arc::new(FakeRepository {
                error: Some(error.clone()),
                ..Default::default()
            });
            assert!(
                matches!(GetSettings::new(repository.clone()).execute().await,
                Err(SettingsAppError::Repository(actual)) if actual == error)
            );
            assert!(
                matches!(UpdateSettings::new(repository).execute(input()).await,
                Err(SettingsAppError::Repository(actual)) if actual == error)
            );
        }
    });
}

struct FakeCredentials {
    presence: Mutex<CredentialPresence>,
    backend: CredentialBackend,
    error: Option<CredentialStoreError>,
}

#[async_trait]
impl CredentialStore for FakeCredentials {
    async fn commit(
        &self,
        _: CredentialKind,
        _: PendingCredentialRef,
    ) -> Result<(), CredentialStoreError> {
        if let Some(error) = self.error {
            return Err(error);
        }
        *self.presence.lock().unwrap() = CredentialPresence::Set;
        Ok(())
    }
    async fn clear(&self, _: CredentialKind) -> Result<(), CredentialStoreError> {
        if let Some(error) = self.error {
            return Err(error);
        }
        *self.presence.lock().unwrap() = CredentialPresence::NotSet;
        Ok(())
    }
    async fn status(&self, _: CredentialKind) -> Result<CredentialPresence, CredentialStoreError> {
        if let Some(error) = self.error {
            return Err(error);
        }
        Ok(*self.presence.lock().unwrap())
    }
    fn backend(&self) -> CredentialBackend {
        self.backend
    }
}

#[test]
fn credential_use_cases_report_presence_backend_and_session_warning() {
    block_on(async {
        for backend in [CredentialBackend::Keyring, CredentialBackend::SessionMemory] {
            let store = Arc::new(FakeCredentials {
                presence: Mutex::new(CredentialPresence::NotSet),
                backend,
                error: None,
            });
            let status = GetCredentialStatus::new(store.clone());
            let initial = status.execute(CredentialKind::CivitAi).await.unwrap();
            assert_eq!(initial.presence, CredentialPresence::NotSet);
            assert_eq!(initial.backend, backend);
            assert_eq!(
                initial.session_only_warning,
                backend == CredentialBackend::SessionMemory
            );
            SetCredential::new(store.clone())
                .execute(CredentialKind::CivitAi, PendingCredentialRef::new(1))
                .await
                .unwrap();
            assert_eq!(
                status
                    .execute(CredentialKind::CivitAi)
                    .await
                    .unwrap()
                    .presence,
                CredentialPresence::Set
            );
            ClearCredential::new(store)
                .execute(CredentialKind::CivitAi)
                .await
                .unwrap();
            assert_eq!(
                status
                    .execute(CredentialKind::CivitAi)
                    .await
                    .unwrap()
                    .presence,
                CredentialPresence::NotSet
            );
        }
    });
}

#[test]
fn credential_errors_are_preserved() {
    block_on(async {
        for error in [
            CredentialStoreError::Unavailable,
            CredentialStoreError::Locked,
            CredentialStoreError::UnknownPending,
            CredentialStoreError::KindMismatch,
            CredentialStoreError::DeleteFailed,
        ] {
            let store = Arc::new(FakeCredentials {
                presence: Mutex::new(CredentialPresence::NotSet),
                backend: CredentialBackend::Keyring,
                error: Some(error),
            });
            assert!(
                matches!(SetCredential::new(store.clone()).execute(CredentialKind::HuggingFace, PendingCredentialRef::new(1)).await,
                Err(SettingsAppError::Credential(actual)) if actual == error)
            );
            assert!(
                matches!(ClearCredential::new(store.clone()).execute(CredentialKind::HuggingFace).await,
                Err(SettingsAppError::Credential(actual)) if actual == error)
            );
            assert!(
                matches!(GetCredentialStatus::new(store).execute(CredentialKind::HuggingFace).await,
                Err(SettingsAppError::Credential(actual)) if actual == error)
            );
        }
    });
}
