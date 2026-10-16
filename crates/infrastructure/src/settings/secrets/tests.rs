use super::*;
use crate::shared::logging::redact;

#[derive(Default)]
struct FakeBackend {
    probe_error: Mutex<Option<BackendError>>,
    operation_error: Mutex<Option<BackendError>>,
    values: Mutex<HashMap<CredentialKind, String>>,
}

impl KeyringBackend for FakeBackend {
    fn read(&self, kind: CredentialKind) -> Result<Option<String>, BackendError> {
        if let Some(error) = *self.operation_error.lock().unwrap() {
            return Err(error);
        }
        Ok(self.values.lock().unwrap().get(&kind).cloned())
    }
    fn probe(&self) -> Result<(), BackendError> {
        self.probe_error.lock().unwrap().map_or(Ok(()), Err)
    }
    fn set(&self, kind: CredentialKind, value: &str) -> Result<(), BackendError> {
        if let Some(error) = *self.operation_error.lock().unwrap() {
            return Err(error);
        }
        self.values.lock().unwrap().insert(kind, value.into());
        Ok(())
    }
    fn presence(&self, kind: CredentialKind) -> Result<CredentialPresence, BackendError> {
        if let Some(error) = *self.operation_error.lock().unwrap() {
            return Err(error);
        }
        Ok(if self.values.lock().unwrap().contains_key(&kind) {
            CredentialPresence::Set
        } else {
            CredentialPresence::NotSet
        })
    }
    fn delete(&self, kind: CredentialKind) -> Result<(), BackendError> {
        if let Some(error) = *self.operation_error.lock().unwrap() {
            return Err(error);
        }
        self.values.lock().unwrap().remove(&kind);
        Ok(())
    }
}

#[tokio::test]
async fn vault_reads_same_backend_and_generations_invalidate_old_tests() {
    let ingress = SecretIngress::default();
    let backend = Arc::new(FakeBackend::default());
    let store = KeyringCredentialStore::with_backend(ingress.clone(), backend.clone())
        .await
        .unwrap();
    assert!(
        store
            .read(CredentialKind::HuggingFace)
            .await
            .unwrap()
            .value
            .is_none()
    );
    let lease = ingress.stage(CredentialKind::HuggingFace, "vault-private-value".into());
    store
        .commit(CredentialKind::HuggingFace, lease.reference())
        .await
        .unwrap();
    let read = store.read(CredentialKind::HuggingFace).await.unwrap();
    assert_eq!(read.value.unwrap().expose(), "vault-private-value");
    assert_eq!(read.generation, 1);
    assert_eq!(redact("vault-private-value"), "[REDACTED]");
    *backend.operation_error.lock().unwrap() = Some(BackendError::Locked);
    assert!(matches!(
        store.read(CredentialKind::HuggingFace).await,
        Err(CredentialStoreError::Locked)
    ));
    *backend.operation_error.lock().unwrap() = None;
    store.clear(CredentialKind::HuggingFace).await.unwrap();
    assert_eq!(
        store.generation(CredentialKind::HuggingFace).await.unwrap(),
        2
    );
}

#[test]
fn lease_drop_discards_staged_value_and_staging_registers_it_immediately() {
    let ingress = SecretIngress::default();
    let lease = ingress.stage(CredentialKind::CivitAi, "lease-private-value".into());
    let reference = lease.reference();
    assert_eq!(redact("lease-private-value"), "[REDACTED]");
    drop(lease);
    assert!(matches!(
        ingress.take(CredentialKind::CivitAi, reference),
        Err(CredentialStoreError::UnknownPending)
    ));
}

#[tokio::test]
async fn available_backend_round_trip_and_one_shot_kind_binding() {
    let ingress = SecretIngress::default();
    let backend = Arc::new(FakeBackend::default());
    let store = KeyringCredentialStore::with_backend(ingress.clone(), backend.clone())
        .await
        .unwrap();
    assert_eq!(store.backend(), CredentialBackend::Keyring);
    assert_eq!(
        store.status(CredentialKind::CivitAi).await.unwrap(),
        CredentialPresence::NotSet
    );
    let lease = ingress.stage(CredentialKind::CivitAi, "round-trip-private-value".into());
    store
        .commit(CredentialKind::CivitAi, lease.reference())
        .await
        .unwrap();
    assert_eq!(
        store
            .commit(CredentialKind::CivitAi, lease.reference())
            .await,
        Err(CredentialStoreError::UnknownPending)
    );
    drop(lease);
    assert_eq!(
        store.status(CredentialKind::CivitAi).await.unwrap(),
        CredentialPresence::Set
    );
    assert_eq!(
        store.status(CredentialKind::HuggingFace).await.unwrap(),
        CredentialPresence::NotSet
    );
    let wrong_kind = ingress.stage(CredentialKind::CivitAi, "wrong-kind-private-value".into());
    assert_eq!(
        store
            .commit(CredentialKind::HuggingFace, wrong_kind.reference())
            .await,
        Err(CredentialStoreError::KindMismatch)
    );
    assert_eq!(
        store
            .commit(CredentialKind::CivitAi, wrong_kind.reference())
            .await,
        Err(CredentialStoreError::UnknownPending)
    );
    store.clear(CredentialKind::CivitAi).await.unwrap();
    store.clear(CredentialKind::CivitAi).await.unwrap();
    assert_eq!(
        store.status(CredentialKind::CivitAi).await.unwrap(),
        CredentialPresence::NotSet
    );
    assert!(backend.values.lock().unwrap().is_empty());
}

#[tokio::test]
async fn references_are_local_to_the_ingress_that_issued_them() {
    let first = SecretIngress::default();
    let second = SecretIngress::default();
    let lease = first.stage(CredentialKind::HuggingFace, "first-ingress-secret".into());
    let _other = second.stage(CredentialKind::HuggingFace, "second-ingress-secret".into());
    let store = KeyringCredentialStore::with_backend(second, Arc::new(FakeBackend::default()))
        .await
        .unwrap();
    assert_eq!(
        store
            .commit(CredentialKind::HuggingFace, lease.reference())
            .await,
        Err(CredentialStoreError::UnknownPending)
    );
}

#[tokio::test]
async fn unavailable_probe_uses_session_memory_and_clear_succeeds() {
    let ingress = SecretIngress::default();
    let backend = Arc::new(FakeBackend::default());
    *backend.probe_error.lock().unwrap() = Some(BackendError::Unavailable);
    *backend.operation_error.lock().unwrap() = Some(BackendError::Unavailable);
    let store = KeyringCredentialStore::with_backend(ingress.clone(), backend.clone())
        .await
        .unwrap();
    assert_eq!(store.backend(), CredentialBackend::SessionMemory);
    let lease = ingress.stage(CredentialKind::HuggingFace, "session-memory-secret".into());
    store
        .commit(CredentialKind::HuggingFace, lease.reference())
        .await
        .unwrap();
    assert_eq!(
        store.status(CredentialKind::HuggingFace).await.unwrap(),
        CredentialPresence::Set
    );
    assert!(backend.values.lock().unwrap().is_empty());
    store.clear(CredentialKind::HuggingFace).await.unwrap();
    assert_eq!(
        store.status(CredentialKind::HuggingFace).await.unwrap(),
        CredentialPresence::NotSet
    );
    drop(store);
    let new_session = KeyringCredentialStore::with_backend(ingress, backend)
        .await
        .unwrap();
    assert_eq!(
        new_session
            .status(CredentialKind::HuggingFace)
            .await
            .unwrap(),
        CredentialPresence::NotSet
    );
}

#[tokio::test]
async fn unavailable_write_falls_back_but_failed_durable_delete_is_reported() {
    let ingress = SecretIngress::default();
    let backend = Arc::new(FakeBackend::default());
    let store = KeyringCredentialStore::with_backend(ingress.clone(), backend.clone())
        .await
        .unwrap();
    let first = ingress.stage(CredentialKind::CivitAi, "durable-old-private-value".into());
    store
        .commit(CredentialKind::CivitAi, first.reference())
        .await
        .unwrap();
    *backend.operation_error.lock().unwrap() = Some(BackendError::Unavailable);
    let second = ingress.stage(CredentialKind::CivitAi, "session-new-private-value".into());
    store
        .commit(CredentialKind::CivitAi, second.reference())
        .await
        .unwrap();
    assert_eq!(store.backend(), CredentialBackend::SessionMemory);
    assert_eq!(
        store.status(CredentialKind::CivitAi).await.unwrap(),
        CredentialPresence::Set
    );
    assert_eq!(
        store.clear(CredentialKind::CivitAi).await,
        Err(CredentialStoreError::DeleteFailed)
    );
    assert!(store.state.lock().unwrap().memory.is_empty());
    assert!(
        backend
            .values
            .lock()
            .unwrap()
            .contains_key(&CredentialKind::CivitAi)
    );
    *backend.operation_error.lock().unwrap() = None;
    store.clear(CredentialKind::CivitAi).await.unwrap();
    assert!(backend.values.lock().unwrap().is_empty());
}

#[tokio::test]
async fn unavailable_and_locked_reads_are_distinct_from_absent() {
    for (error, expected) in [
        (BackendError::Unavailable, CredentialStoreError::Unavailable),
        (BackendError::Locked, CredentialStoreError::Locked),
    ] {
        let backend = Arc::new(FakeBackend::default());
        let store = KeyringCredentialStore::with_backend(SecretIngress::default(), backend.clone())
            .await
            .unwrap();
        *backend.operation_error.lock().unwrap() = Some(error);
        assert_eq!(store.status(CredentialKind::CivitAi).await, Err(expected));
    }
}

#[tokio::test]
async fn locked_probe_preserves_locked_errors_and_does_not_claim_absence() {
    let ingress = SecretIngress::default();
    let backend = Arc::new(FakeBackend::default());
    *backend.probe_error.lock().unwrap() = Some(BackendError::Locked);
    *backend.operation_error.lock().unwrap() = Some(BackendError::Locked);
    let store = KeyringCredentialStore::with_backend(ingress.clone(), backend)
        .await
        .unwrap();
    let lease = ingress.stage(CredentialKind::CivitAi, "locked-store-private-value".into());
    assert_eq!(
        store
            .commit(CredentialKind::CivitAi, lease.reference())
            .await,
        Err(CredentialStoreError::Locked)
    );
    assert_eq!(
        store.status(CredentialKind::CivitAi).await,
        Err(CredentialStoreError::Locked)
    );
    assert_eq!(
        store.clear(CredentialKind::CivitAi).await,
        Err(CredentialStoreError::DeleteFailed)
    );
}

#[tokio::test]
#[ignore = "requires an unlocked real OS credential store"]
async fn real_keyring_round_trip() {
    tokio::task::spawn_blocking(|| {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let user = format!("m0-test-{}-{unique}", std::process::id());
        let value = format!("disposable-test-secret-{unique}");
        register_secret(&value);
        let entry = keyring::Entry::new(SERVICE, &user)
            .map_err(|error| backend_error(&error))
            .unwrap();
        entry
            .set_password(&value)
            .map_err(|error| backend_error(&error))
            .unwrap();
        let actual = entry.get_password().map_err(|error| backend_error(&error));
        let deleted = entry
            .delete_credential()
            .map_err(|error| backend_error(&error));
        assert!(actual.is_ok_and(|actual| actual == value));
        assert!(deleted.is_ok());
    })
    .await
    .unwrap();
}
