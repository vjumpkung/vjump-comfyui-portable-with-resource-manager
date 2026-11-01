use super::*;
use crate::test_support::block_on;
use async_trait::async_trait;
use domain::{
    models::{acquisition::*, library::ModelCategory},
    shared::model_values::{JobId, RelativeModelPath, SafeFileName, Sha256},
};
use std::sync::{Arc, Mutex};

fn job() -> DownloadJob {
    let id = "remove-me";
    let url = "https://example.com/file.bin";
    DownloadJob {
        id: JobId::new(id).unwrap(),
        source: SourceRef::Direct {
            locator: url.into(),
            ingress_id: None,
            requires_source_refresh: false,
            insecure_http_confirmed: true,
        },
        identity: ProviderIdentity(format!("direct:{id}")),
        destination: Destination {
            root_id: "models".into(),
            category: ModelCategory::new("checkpoints").unwrap(),
            subdirectory: RelativeModelPath::default(),
            filename: SafeFileName::new(format!("{id}.bin")).unwrap(),
        },
        state: DownloadState::Queued,
        requested_engine: DownloadEngine::Native,
        effective_engine: DownloadEngine::Native,
        generation: 0,
        size: DownloadSize::Unknown,
        expected_sha256: None,
        bytes_done: 0,
        validator: None,
        retry_count: 0,
        retry_at: None,
        stop_intent: None,
        error: None,
        conflict: None,
        decision_id: None,
        observed: None,
        publication: None,
        created_at: 1,
        updated_at: 1,
    }
}

struct Repository {
    job: DownloadJob,
    removed: Mutex<Vec<(JobId, u64)>>,
    result: Result<bool>,
    get_error: bool,
}
#[async_trait]
impl JobRepository for Repository {
    async fn get(&self, _: &JobId) -> Result<DownloadJob> {
        if self.get_error {
            return Err(DownloadErrorCode::Storage.into());
        }
        Ok(self.job.clone())
    }
    async fn remove(&self, id: &JobId, generation: u64) -> Result<bool> {
        self.removed.lock().unwrap().push((id.clone(), generation));
        self.result
    }
    async fn enqueue(&self, _: EnqueueRequest) -> Result<EnqueueOutcome> {
        unreachable!()
    }
    async fn list(&self, _: Option<u64>, _: u32) -> Result<Vec<DownloadJob>> {
        unreachable!()
    }
    async fn unfinished(&self) -> Result<Vec<DownloadJob>> {
        unreachable!()
    }
    async fn save(&self, _: &DownloadJob, _: u64) -> Result<()> {
        unreachable!()
    }
    async fn claim(&self, _: u64) -> Result<Option<DownloadJob>> {
        unreachable!()
    }
    async fn catalog_matches(&self, _: &Sha256) -> Result<Vec<(Destination, FileIdentity)>> {
        unreachable!()
    }
    async fn catalog_and_mark(&self, _: &DownloadJob) -> Result<DownloadJob> {
        unreachable!()
    }
}
#[derive(Default)]
struct Events(Mutex<Vec<JobId>>);
impl DownloadEvents for Events {
    fn state(&self, _: &DownloadJob) {
        unreachable!()
    }
    fn removed(&self, id: &JobId) {
        self.0.lock().unwrap().push(id.clone());
    }
}
fn publication() -> PublicationIntent {
    PublicationIntent {
        phase: FinalizePhase::Verified,
        staged_key: "staged".into(),
        staged_identity: FileIdentity {
            size: 1,
            modified_ns: 1,
            platform_id: "file".into(),
            digest: None,
        },
        digest: Sha256::new(&"a".repeat(64)).unwrap(),
        external_verified: false,
        backup_name: SafeFileName::new("backup.bin").unwrap(),
        published_identity: None,
    }
}

#[test]
fn remove_refuses_nonterminal_pending_finalization_and_stale_generations() {
    block_on(async {
        for state in [
            DownloadState::Queued,
            DownloadState::Resolving,
            DownloadState::Downloading,
            DownloadState::Verifying,
            DownloadState::Paused,
            DownloadState::Failed,
            DownloadState::Cancelled,
            DownloadState::Completed,
        ] {
            let mut job = job();
            job.state = state;
            if matches!(state, DownloadState::Failed | DownloadState::Cancelled) {
                job.publication = Some(publication());
            }
            let generation = if state == DownloadState::Completed {
                job.generation + 1
            } else {
                job.generation
            };
            let repository = Arc::new(Repository {
                job: job.clone(),
                removed: Mutex::default(),
                result: Ok(true),
                get_error: false,
            });
            let events = Arc::new(Events::default());
            let remove = RemoveJob {
                repository: repository.clone(),
                events: events.clone(),
            };
            assert_eq!(
                remove.execute(&job.id, generation).await,
                Err(DownloadErrorCode::InvalidTransition.into())
            );
            assert!(repository.removed.lock().unwrap().is_empty());
            assert!(events.0.lock().unwrap().is_empty());
        }
    });
}

#[test]
fn remove_terminal_jobs_emits_only_after_success_and_propagates_failures() {
    block_on(async {
        for state in [
            DownloadState::Completed,
            DownloadState::Failed,
            DownloadState::Cancelled,
        ] {
            for (result, get_error) in [
                (Ok(true), false),
                (Ok(false), false),
                (Err(DownloadErrorCode::Storage.into()), false),
                (Ok(true), true),
            ] {
                let mut job = job();
                job.state = state;
                job.generation = 7;
                if state == DownloadState::Completed {
                    job.publication = Some(publication());
                }
                let repository = Arc::new(Repository {
                    job: job.clone(),
                    removed: Mutex::default(),
                    result,
                    get_error,
                });
                let events = Arc::new(Events::default());
                let remove = RemoveJob {
                    repository: repository.clone(),
                    events: events.clone(),
                };
                let outcome = remove.execute(&job.id, 7).await;
                if get_error || result.is_err() {
                    assert_eq!(outcome, Err(DownloadErrorCode::Storage.into()));
                } else if result == Ok(false) {
                    assert_eq!(outcome, Err(DownloadErrorCode::InvalidTransition.into()));
                } else {
                    assert_eq!(outcome, Ok(()));
                }
                assert_eq!(
                    events.0.lock().unwrap().as_slice(),
                    if outcome.is_ok() {
                        std::slice::from_ref(&job.id)
                    } else {
                        &[]
                    }
                );
                assert_eq!(
                    repository.removed.lock().unwrap().len(),
                    usize::from(!get_error)
                );
                if !get_error {
                    assert_eq!(repository.removed.lock().unwrap()[0], (job.id.clone(), 7));
                }
            }
        }
    });
}
