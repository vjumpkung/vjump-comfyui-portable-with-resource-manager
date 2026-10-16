use super::CredentialStoreError;
use async_trait::async_trait;
use domain::settings::CredentialKind;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialTestResult {
    Valid,
    InvalidOrInsufficientAccess,
    Absent,
    BackendUnavailable,
    BackendLocked,
    NetworkOrServiceFailure,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CredentialTestStatus {
    pub generation: u64,
    pub tested_at: u64,
    pub result: CredentialTestResult,
}
#[async_trait]
pub trait CredentialTester: Send + Sync {
    async fn test(
        &self,
        kind: CredentialKind,
        now: u64,
    ) -> Result<CredentialTestStatus, CredentialStoreError>;
    async fn last_test(
        &self,
        kind: CredentialKind,
    ) -> Result<Option<CredentialTestStatus>, CredentialStoreError>;
}
pub struct TestCredential {
    pub tester: Arc<dyn CredentialTester>,
}
impl TestCredential {
    pub async fn execute(
        &self,
        kind: CredentialKind,
        now: u64,
    ) -> Result<CredentialTestStatus, CredentialStoreError> {
        self.tester.test(kind, now).await
    }
}
