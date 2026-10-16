use super::{finalization::FinalizeDownload, ports::*};
use domain::{
    models::acquisition::*,
    shared::model_values::{JobId, RelativeModelPath, SafeFileName},
};
use std::sync::Arc;

pub struct ResolveDownload {
    pub resolver: Arc<dyn SourceResolver>,
}
impl ResolveDownload {
    pub async fn execute(&self, source: &SourceRef) -> Result<Resolution> {
        self.resolver.resolve(source).await
    }
}
pub struct EnqueueItem {
    pub resolution_id: u64,
    pub file_index: usize,
    pub id: JobId,
    pub root_id: String,
    pub category: domain::models::library::ModelCategory,
    pub subdirectory: RelativeModelPath,
    pub filename: Option<SafeFileName>,
    pub engine: DownloadEngine,
    pub conflict: Option<ConflictChoice>,
    pub force: bool,
}
pub struct EnqueueDownloads {
    pub resolver: Arc<dyn SourceResolver>,
    pub repository: Arc<dyn JobRepository>,
    pub files: Arc<dyn DownloadFiles>,
    pub events: Arc<dyn DownloadEvents>,
    pub control: Arc<dyn DownloadControl>,
}
impl EnqueueDownloads {
    pub async fn execute(&self, items: Vec<EnqueueItem>, now: u64) -> Vec<EnqueueOutcome> {
        let mut result = Vec::with_capacity(items.len());
        for item in items {
            result.push(
                self.one(item, now)
                    .await
                    .unwrap_or_else(|e| EnqueueOutcome::Invalid(e.0)),
            );
        }
        self.control.wake();
        result
    }
    async fn one(&self, item: EnqueueItem, now: u64) -> Result<EnqueueOutcome> {
        let file = self
            .resolver
            .selected(item.resolution_id, item.file_index)
            .await?;
        let mut catalog_match = None;
        if !item.force
            && let Some(hash) = &file.sha256
        {
            for (destination, identity) in self.repository.catalog_matches(hash).await? {
                if self.files.inspect(&destination).await?.as_ref() == Some(&identity) {
                    catalog_match = Some(destination);
                    break;
                }
            }
        }
        if item.conflict == Some(ConflictChoice::Skip) {
            return Ok(EnqueueOutcome::Skipped);
        }
        let mut destination = Destination {
            root_id: item.root_id,
            category: item.category,
            subdirectory: item.subdirectory,
            filename: item.filename.unwrap_or(file.filename),
        };
        let original = destination.filename.clone();
        for suffix in 0..10_000 {
            if suffix > 0 {
                destination.filename = original
                    .with_suffix(suffix)
                    .map_err(|_| DownloadErrorCode::InvalidSource)?;
            }
            let current = self.files.inspect(&destination).await?;
            if let Some(ref identity) = current {
                match &item.conflict {
                    Some(ConflictChoice::KeepBoth) => continue,
                    Some(ConflictChoice::Overwrite { approved }) if approved == identity => {}
                    _ => {
                        return Ok(EnqueueOutcome::Conflict {
                            destination,
                            current,
                        });
                    }
                }
            }
            let engine = if matches!(file.source, SourceRef::HuggingFace { .. }) {
                item.engine
            } else {
                DownloadEngine::Native
            };
            let job = DownloadJob {
                id: item.id.clone(),
                source: file.source.clone(),
                identity: file.identity.clone(),
                destination: destination.clone(),
                state: DownloadState::Queued,
                requested_engine: engine,
                effective_engine: engine,
                generation: 0,
                size: file.size,
                expected_sha256: file.sha256.clone(),
                bytes_done: 0,
                validator: None,
                retry_count: 0,
                retry_at: None,
                stop_intent: None,
                error: None,
                conflict: item.conflict.clone(),
                decision_id: None,
                observed: None,
                publication: None,
                created_at: now,
                updated_at: now,
            };
            let outcome = self
                .repository
                .enqueue(EnqueueRequest {
                    job: job.clone(),
                    force: item.force,
                    catalog_match: catalog_match.clone(),
                })
                .await?;
            if matches!(outcome, EnqueueOutcome::Conflict { .. })
                && item.conflict == Some(ConflictChoice::KeepBoth)
            {
                continue;
            }
            if matches!(outcome, EnqueueOutcome::Queued(_)) {
                self.events.state(&job);
            }
            return Ok(outcome);
        }
        Err(DownloadErrorCode::Conflict.into())
    }
}
pub struct ResumeJob {
    pub repository: Arc<dyn JobRepository>,
    pub files: Arc<dyn DownloadFiles>,
    pub events: Arc<dyn DownloadEvents>,
    pub control: Arc<dyn DownloadControl>,
}
pub struct BoundConflictDecision {
    pub decision_id: String,
    pub choice: ConflictChoice,
}
impl ResumeJob {
    pub async fn execute(
        &self,
        id: &JobId,
        decision: Option<BoundConflictDecision>,
    ) -> Result<DownloadJob> {
        let mut job = self.repository.get(id).await?;
        if job.state != DownloadState::Paused && job.state != DownloadState::Failed {
            return Err(DownloadErrorCode::InvalidTransition.into());
        }
        if let Some(expected) = &job.decision_id {
            let decision = decision.ok_or(DownloadErrorCode::SelectionRequired)?;
            if &decision.decision_id != expected {
                return Err(DownloadErrorCode::StaleDecision.into());
            }
            if let ConflictChoice::Overwrite { approved } = &decision.choice {
                let current = self.files.inspect(&job.destination).await?;
                if current.as_ref() != Some(approved) {
                    // The file changed after the prompt: raise a fresh prompt that carries the
                    // newly observed identity instead of overwriting something unseen.
                    let previous = job.generation;
                    job.generation += 1;
                    job.decision_id = Some(format!("{}-{}-stale", job.id.as_str(), job.generation));
                    job.observed = current;
                    self.repository.save(&job, previous).await?;
                    self.events.state(&job);
                    return Err(DownloadErrorCode::StaleDecision.into());
                }
            }
            match decision.choice {
                ConflictChoice::Skip => return Err(DownloadErrorCode::Cancelled.into()),
                ConflictChoice::KeepBoth => {
                    let original = job.destination.filename.clone();
                    let mut chosen = false;
                    for n in 1..10_000 {
                        job.destination.filename = original
                            .with_suffix(n)
                            .map_err(|_| DownloadErrorCode::InvalidSource)?;
                        if self.files.inspect(&job.destination).await?.is_none() {
                            chosen = true;
                            break;
                        }
                    }
                    if !chosen {
                        return Err(DownloadErrorCode::Conflict.into());
                    }
                    if !matches!(job.conflict, Some(ConflictChoice::Overwrite { .. })) {
                        job.conflict = Some(ConflictChoice::KeepBoth);
                    }
                }
                choice => job.conflict = Some(choice),
            }
        }
        if job.restart_from_zero() && job.publication.is_none() {
            self.files.cleanup(&job, false).await?;
            job.bytes_done = 0;
            job.validator = None;
        }
        job.transition(DownloadState::Queued)
            .map_err(DownloadError)?;
        job.stop_intent = None;
        job.retry_count = 0;
        job.retry_at = None;
        job.error = None;
        job.decision_id = None;
        let previous = job.generation;
        job.generation += 1;
        self.repository.save(&job, previous).await?;
        self.events.state(&job);
        self.control.wake();
        Ok(job)
    }
}
pub struct PauseJob {
    pub control: Arc<dyn DownloadControl>,
}
impl PauseJob {
    pub async fn execute(&self, id: &JobId) -> Result<DownloadJob> {
        self.control.stop(id, StopIntent::Pause).await
    }
}
pub struct CancelJob {
    pub control: Arc<dyn DownloadControl>,
}
impl CancelJob {
    pub async fn execute(&self, id: &JobId) -> Result<DownloadJob> {
        self.control.stop(id, StopIntent::Cancel).await
    }
}
pub struct ListJobs {
    pub repository: Arc<dyn JobRepository>,
}
pub struct RetryJob {
    pub resume: ResumeJob,
}
impl RetryJob {
    pub async fn execute(&self, id: &JobId) -> Result<DownloadJob> {
        let job = self.resume.repository.get(id).await?;
        if job.state != DownloadState::Failed {
            return Err(DownloadErrorCode::InvalidTransition.into());
        }
        self.resume.execute(id, None).await
    }
}
impl ListJobs {
    pub async fn execute(&self, before: Option<u64>, limit: u32) -> Result<Vec<DownloadJob>> {
        self.repository.list(before, limit.clamp(1, 500)).await
    }
}
pub struct ImportPack {
    pub packs: Arc<dyn ModelPacks>,
    pub resolver: Arc<dyn SourceResolver>,
}
impl ImportPack {
    pub async fn execute(&self, input: &str, remote: bool) -> Result<Vec<PackPreviewRow>> {
        let mut preview = Vec::new();
        for row in self.packs.parse(input, remote).await? {
            preview.push(PackPreviewRow {
                name: row.name,
                category: row.category,
                resolved: self.resolver.resolve(&row.source).await,
            });
        }
        Ok(preview)
    }
}
pub struct ExportResourceList {
    pub packs: Arc<dyn ModelPacks>,
}
impl ExportResourceList {
    pub fn execute(&self, jobs: &[DownloadJob]) -> Result<PackExport> {
        self.packs.export(jobs)
    }
}
pub struct ReconcileOnStartup {
    pub finalizer: Arc<FinalizeDownload>,
}
impl ReconcileOnStartup {
    pub async fn execute(&self) -> Result<()> {
        let jobs = self.finalizer.repository.unfinished().await?;
        for mut job in jobs {
            if job.publication.is_some() {
                if let Err(error) = self.finalizer.execute(&mut job).await {
                    job.state = DownloadState::Paused;
                    job.error = Some(error.0);
                    if error.0 == DownloadErrorCode::Conflict {
                        job.decision_id =
                            Some(format!("{}-{}-recovery", job.id.as_str(), job.generation));
                        job.observed = self
                            .finalizer
                            .files
                            .inspect(&job.destination)
                            .await
                            .ok()
                            .flatten();
                    }
                    self.finalizer.repository.save(&job, job.generation).await?;
                }
            } else if matches!(
                job.state,
                DownloadState::Resolving | DownloadState::Downloading | DownloadState::Verifying
            ) {
                self.finalizer
                    .files
                    .cleanup(&job, job.effective_engine == DownloadEngine::Native)
                    .await?;
                job.state = DownloadState::Paused;
                job.stop_intent = None;
                if matches!(
                    job.source,
                    SourceRef::Direct {
                        requires_source_refresh: true,
                        ..
                    }
                ) {
                    job.error = Some(DownloadErrorCode::SourceRefreshRequired);
                }
                self.finalizer.repository.save(&job, job.generation).await?;
            }
            self.finalizer.events.state(&job);
        }
        self.finalizer
            .files
            .clean_orphans(&self.finalizer.repository.unfinished().await?)
            .await
    }
}
