use super::ports::*;
use domain::{models::acquisition::*, shared::model_values::SafeFileName};
use std::sync::Arc;

pub struct FinalizeDownload {
    pub repository: Arc<dyn JobRepository>,
    pub files: Arc<dyn DownloadFiles>,
    pub events: Arc<dyn DownloadEvents>,
}
impl FinalizeDownload {
    pub async fn prepare(&self, job: &mut DownloadJob, result: TransferResult) -> Result<()> {
        let (identity, digest) = self
            .files
            .verify(&result.staged_key, job.expected_sha256.as_ref(), job.size)
            .await?;
        job.bytes_done = identity.size;
        job.validator = result.validator;
        job.state = DownloadState::Verifying;
        job.publication = Some(PublicationIntent {
            phase: FinalizePhase::Verified,
            staged_key: result.staged_key,
            staged_identity: identity,
            digest,
            external_verified: job.expected_sha256.is_some(),
            backup_name: SafeFileName::new(format!(
                ".download-{}-{}.backup",
                job.id.as_str(),
                job.generation
            ))
            .map_err(|_| DownloadErrorCode::InvalidMetadata)?,
            published_identity: None,
        });
        self.repository.save(job, job.generation).await?;
        self.events.state(job);
        self.execute(job).await
    }
    pub async fn execute(&self, job: &mut DownloadJob) -> Result<()> {
        let phase = job
            .publication
            .as_ref()
            .ok_or(DownloadErrorCode::InvalidTransition)?
            .phase;
        if phase == FinalizePhase::Verified {
            let identity = self.files.publish(job).await?;
            let intent = job.publication.as_mut().expect("publication intent");
            intent.published_identity = Some(identity);
            intent.phase = FinalizePhase::Published;
            self.repository.save(job, job.generation).await?;
        }
        if job.publication.as_ref().expect("publication intent").phase == FinalizePhase::Published {
            self.files.recycle_backup(job).await?;
            *job = self.repository.catalog_and_mark(job).await?;
        }
        job.state = DownloadState::Completed;
        job.stop_intent = None;
        job.error = None;
        self.repository.save(job, job.generation).await?;
        self.files.cleanup(job, false).await?;
        self.events.state(job);
        Ok(())
    }
}
