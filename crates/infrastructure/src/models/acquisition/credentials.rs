use super::http::ArtifactHttp;
use crate::{
    settings::secrets::CredentialVault,
    shared::sqlite::{Db, DbError},
};
use application::settings::{CredentialStoreError, credential_tester::*};
use async_trait::async_trait;
use domain::settings::CredentialKind;
use rusqlite::{OptionalExtension, params};
use std::sync::Arc;

pub struct ProviderCredentialTester {
    pub http: ArtifactHttp,
    pub vault: Arc<dyn CredentialVault>,
    pub db: Db,
}
fn key(kind: CredentialKind) -> &'static str {
    match kind {
        CredentialKind::CivitAi => "civitai",
        CredentialKind::HuggingFace => "huggingface",
    }
}
fn result_name(value: CredentialTestResult) -> &'static str {
    match value {
        CredentialTestResult::Valid => "valid",
        CredentialTestResult::InvalidOrInsufficientAccess => "invalid",
        CredentialTestResult::Absent => "absent",
        CredentialTestResult::BackendUnavailable => "unavailable",
        CredentialTestResult::BackendLocked => "locked",
        CredentialTestResult::NetworkOrServiceFailure => "network",
    }
}
#[async_trait]
impl CredentialTester for ProviderCredentialTester {
    async fn test(
        &self,
        kind: CredentialKind,
        now: u64,
    ) -> std::result::Result<CredentialTestStatus, CredentialStoreError> {
        let snapshot = self.vault.read(kind).await;
        let generation = self.vault.generation(kind).await?;
        let (observed, result) = match snapshot {
            Ok(snapshot) => {
                let result = if let Some(secret) = snapshot.value {
                    let url = match kind {
                        CredentialKind::CivitAi => "https://civitai.com/api/v1/me",
                        CredentialKind::HuggingFace => "https://huggingface.co/api/whoami-v2",
                    };
                    match self
                        .http
                        .client
                        .get(url)
                        .bearer_auth(secret.expose())
                        .timeout(std::time::Duration::from_secs(30))
                        .send()
                        .await
                    {
                        Ok(response) if response.status().is_success() => {
                            CredentialTestResult::Valid
                        }
                        Ok(response) if matches!(response.status().as_u16(), 401 | 403) => {
                            CredentialTestResult::InvalidOrInsufficientAccess
                        }
                        _ => CredentialTestResult::NetworkOrServiceFailure,
                    }
                } else {
                    CredentialTestResult::Absent
                };
                (snapshot.generation, result)
            }
            Err(CredentialStoreError::Locked) => (generation, CredentialTestResult::BackendLocked),
            Err(_) => (generation, CredentialTestResult::BackendUnavailable),
        };
        // Generation comparison prevents a late response from testing a replacement credential.
        if self.vault.generation(kind).await? != observed {
            return Err(CredentialStoreError::UnknownPending);
        }
        let status = CredentialTestStatus {
            generation: observed,
            tested_at: now,
            result,
        };
        self.db.run(move|c|{c.execute("INSERT INTO credential_tests(kind,generation,tested_at,result) VALUES (?1,?2,?3,?4) ON CONFLICT(kind) DO UPDATE SET generation=excluded.generation,tested_at=excluded.tested_at,result=excluded.result",params![key(kind),i64::try_from(observed).map_err(|_|DbError::Unavailable)?,i64::try_from(now).map_err(|_|DbError::Unavailable)?,result_name(result)]).map_err(|_|DbError::Unavailable)?;Ok(())}).await.map_err(|_|CredentialStoreError::Unavailable)?;
        if self.vault.generation(kind).await? != observed {
            return Err(CredentialStoreError::UnknownPending);
        }
        Ok(status)
    }
    async fn last_test(
        &self,
        kind: CredentialKind,
    ) -> std::result::Result<Option<CredentialTestStatus>, CredentialStoreError> {
        let generation = self.vault.generation(kind).await?;
        self.db
            .run(move |c| {
                let row: Option<(i64, i64, String)> = c
                    .query_row(
                        "SELECT generation,tested_at,result FROM credential_tests WHERE kind=?1",
                        [key(kind)],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                    )
                    .optional()
                    .map_err(|_| DbError::Unavailable)?;
                Ok(row
                    .filter(|(g, _, _)| *g >= 0 && *g as u64 == generation)
                    .map(|(generation, tested_at, result)| CredentialTestStatus {
                        generation: generation as u64,
                        tested_at: tested_at.max(0) as u64,
                        result: match result.as_str() {
                            "valid" => CredentialTestResult::Valid,
                            "invalid" => CredentialTestResult::InvalidOrInsufficientAccess,
                            "absent" => CredentialTestResult::Absent,
                            "locked" => CredentialTestResult::BackendLocked,
                            "unavailable" => CredentialTestResult::BackendUnavailable,
                            _ => CredentialTestResult::NetworkOrServiceFailure,
                        },
                    }))
            })
            .await
            .map_err(|_| CredentialStoreError::Unavailable)
    }
}
