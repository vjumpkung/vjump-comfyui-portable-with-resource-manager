use super::progress::DownloadHub;
use application::{
    models::acquisition::{finalization::FinalizeDownload, *},
    shared::CancelToken,
};
use async_trait::async_trait;
use domain::{models::acquisition::*, shared::model_values::JobId};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{Mutex as AsyncMutex, Notify, watch};

struct Running {
    cancel: CancelToken,
    intent: Arc<Mutex<Option<StopIntent>>>,
    done: watch::Receiver<bool>,
}
pub struct DownloadSupervisor {
    pub repository: Arc<dyn JobRepository>,
    pub files: Arc<dyn DownloadFiles>,
    pub native: Arc<dyn Downloader>,
    pub hf: Arc<dyn Downloader>,
    pub hub: Arc<DownloadHub>,
    concurrency: AtomicUsize,
    shutdown: AtomicBool,
    started: AtomicBool,
    wake: Notify,
    coordination: AsyncMutex<()>,
    running: Mutex<HashMap<JobId, Running>>,
    errors: Mutex<Vec<DownloadError>>,
}
impl DownloadSupervisor {
    pub fn new(
        repository: Arc<dyn JobRepository>,
        files: Arc<dyn DownloadFiles>,
        native: Arc<dyn Downloader>,
        hf: Arc<dyn Downloader>,
        hub: Arc<DownloadHub>,
        concurrency: usize,
    ) -> Arc<Self> {
        Arc::new(Self {
            repository,
            files,
            native,
            hf,
            hub,
            concurrency: AtomicUsize::new(concurrency.clamp(1, 32)),
            shutdown: AtomicBool::new(false),
            started: AtomicBool::new(false),
            wake: Notify::new(),
            coordination: AsyncMutex::new(()),
            running: Mutex::new(HashMap::new()),
            errors: Mutex::new(Vec::new()),
        })
    }
    pub fn set_concurrency(&self, value: usize) {
        self.concurrency.store(value.clamp(1, 32), Ordering::SeqCst);
        self.wake.notify_one();
    }
    fn finalizer(&self) -> Arc<FinalizeDownload> {
        Arc::new(FinalizeDownload {
            repository: self.repository.clone(),
            files: self.files.clone(),
            events: self.hub.clone(),
        })
    }
    pub async fn run(self: Arc<Self>) -> Result<()> {
        if self.started.swap(true, Ordering::SeqCst) {
            return Err(DownloadErrorCode::InvalidTransition.into());
        }
        ReconcileOnStartup {
            finalizer: self.finalizer(),
        }
        .execute()
        .await?;
        self.hub.seed(self.repository.list(None, 500).await?);
        loop {
            if self.shutdown.load(Ordering::SeqCst) {
                break;
            }
            let guard = self.coordination.lock().await;
            loop {
                let active = self.running.lock().unwrap_or_else(|p| p.into_inner()).len();
                if active >= self.concurrency.load(Ordering::SeqCst)
                    || self.shutdown.load(Ordering::SeqCst)
                {
                    break;
                }
                let Some(job) = self.repository.claim(now()).await? else {
                    break;
                };
                let cancel = CancelToken::new();
                let intent = Arc::new(Mutex::new(None));
                let (sender, done) = watch::channel(false);
                self.running
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .insert(
                        job.id.clone(),
                        Running {
                            cancel: cancel.clone(),
                            intent: intent.clone(),
                            done,
                        },
                    );
                self.hub.state(&job);
                let supervisor = self.clone();
                tokio::spawn(async move {
                    let id = job.id.clone();
                    let result = supervisor.attempt(job, cancel, intent).await;
                    if let Err(error) = result {
                        supervisor
                            .errors
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .push(error);
                    }
                    supervisor
                        .running
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .remove(&id);
                    let _ = sender.send(true);
                    supervisor.wake.notify_one();
                });
            }
            drop(guard);
            tokio::select! {_=self.wake.notified()=>{},_=tokio::time::sleep(Duration::from_millis(200))=>{}}
        }
        self.pause_all().await?;
        match self
            .errors
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .first()
            .copied()
        {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
    async fn attempt(
        &self,
        mut job: DownloadJob,
        cancel: CancelToken,
        intent: Arc<Mutex<Option<StopIntent>>>,
    ) -> Result<()> {
        if job.publication.is_some() {
            let result = self.finalizer().execute(&mut job).await;
            return self.finish_result(job, result, intent).await;
        }
        if job.effective_engine == DownloadEngine::HfCli {
            self.files.cleanup(&job, false).await?;
            job.bytes_done = 0;
            job.validator = None;
        }
        job.state = DownloadState::Downloading;
        job.stop_intent = None;
        self.repository.save(&job, job.generation).await?;
        self.hub.state(&job);
        let mut refreshed = false;
        let transfer = loop {
            let engine = if job.effective_engine == DownloadEngine::HfCli {
                &self.hf
            } else {
                &self.native
            };
            let result = engine
                .transfer(&job, self.hub.clone(), cancel.clone())
                .await;
            match result {
                Err(error)
                    if error.0 == DownloadErrorCode::ToolLaunch
                        && job.effective_engine == DownloadEngine::HfCli =>
                {
                    self.files.cleanup(&job, false).await?;
                    job.effective_engine = DownloadEngine::Native;
                    job.bytes_done = 0;
                    job.validator = None;
                    self.repository.save(&job, job.generation).await?;
                    self.hub.state(&job);
                }
                Err(error)
                    if retry_action(error.0, job.retry_count, refreshed)
                        == RetryAction::ReResolve =>
                {
                    refreshed = true;
                    job = self.repository.get(&job.id).await?;
                }
                result => break result,
            }
        };
        job = self.repository.get(&job.id).await?;
        if let Some(snapshot) = self.hub.current(&job.id)
            && snapshot.generation == job.generation
        {
            job.bytes_done = snapshot.bytes_done;
        }
        let stop = *intent.lock().unwrap_or_else(|p| p.into_inner());
        if stop.is_some() {
            return self
                .finish_result(job, Err(DownloadErrorCode::Cancelled.into()), intent)
                .await;
        }
        let result = match transfer {
            Ok(result) => self.finalizer().prepare(&mut job, result).await,
            Err(error) => Err(error),
        };
        self.finish_result(job, result, intent).await
    }
    async fn finish_result(
        &self,
        mut job: DownloadJob,
        result: Result<()>,
        intent: Arc<Mutex<Option<StopIntent>>>,
    ) -> Result<()> {
        let Err(error) = result else { return Ok(()) };
        let stop = *intent.lock().unwrap_or_else(|p| p.into_inner());
        job.updated_at = now();
        job.error = Some(error.0);
        if error.0 == DownloadErrorCode::ProcessCleanupFailed {
            job.state = DownloadState::Paused;
        } else if job.publication.is_some() {
            job.state = DownloadState::Paused;
            if error.0 == DownloadErrorCode::Conflict {
                job.decision_id = Some(format!(
                    "{}-{}-{}",
                    job.id.as_str(),
                    job.generation,
                    job.updated_at
                ));
                job.observed = self.files.inspect(&job.destination).await.ok().flatten();
            }
        } else if let Some(stop) = stop {
            self.files
                .cleanup(
                    &job,
                    stop == StopIntent::Pause && job.effective_engine == DownloadEngine::Native,
                )
                .await?;
            job.stop_intent = None;
            job.error = None;
            job.state = if stop == StopIntent::Pause {
                DownloadState::Paused
            } else {
                DownloadState::Cancelled
            };
            if job.effective_engine == DownloadEngine::HfCli {
                job.bytes_done = 0;
                job.validator = None;
            }
        } else if let RetryAction::RetryAfter(seconds) =
            retry_action(error.0, job.retry_count, true)
        {
            if job.effective_engine == DownloadEngine::HfCli {
                self.files.cleanup(&job, false).await?;
                job.bytes_done = 0;
            }
            job.retry_count += 1;
            job.retry_at = Some(now() + seconds);
            job.state = DownloadState::Queued;
        } else {
            self.files
                .cleanup(
                    &job,
                    !matches!(
                        error.0,
                        DownloadErrorCode::HashMismatch
                            | DownloadErrorCode::LengthMismatch
                            | DownloadErrorCode::InvalidRange
                    ) && job.effective_engine == DownloadEngine::Native,
                )
                .await?;
            job.state = DownloadState::Failed;
        }
        self.repository.save(&job, job.generation).await?;
        self.hub.state(&job);
        Ok(())
    }
    pub fn request_shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
        self.wake.notify_one();
    }
    pub async fn pause_all(&self) -> Result<()> {
        let ids = self
            .running
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for id in ids {
            self.stop(&id, StopIntent::Pause).await?;
        }
        Ok(())
    }
}
#[async_trait]
impl DownloadControl for DownloadSupervisor {
    async fn stop(&self, id: &JobId, intent: StopIntent) -> Result<DownloadJob> {
        let guard = self.coordination.lock().await;
        let running = {
            let running = self.running.lock().unwrap_or_else(|p| p.into_inner());
            running.get(id).map(|r| {
                *r.intent.lock().unwrap_or_else(|p| p.into_inner()) = Some(intent);
                r.cancel.cancel();
                r.done.clone()
            })
        };
        if let Some(mut done) = running {
            drop(guard);
            while !*done.borrow() {
                if done.changed().await.is_err() {
                    break;
                }
            }
            return self.repository.get(id).await;
        }
        let mut job = self.repository.get(id).await?;
        if job.publication.is_some() {
            self.finalizer().execute(&mut job).await?;
            return Ok(job);
        }
        if !job.state.active() {
            return Err(DownloadErrorCode::InvalidTransition.into());
        }
        self.files
            .cleanup(
                &job,
                intent == StopIntent::Pause && job.effective_engine == DownloadEngine::Native,
            )
            .await?;
        job.state = if intent == StopIntent::Pause {
            DownloadState::Paused
        } else {
            DownloadState::Cancelled
        };
        job.stop_intent = None;
        job.updated_at = now();
        if job.effective_engine == DownloadEngine::HfCli {
            job.bytes_done = 0;
            job.validator = None;
        }
        self.repository.save(&job, job.generation).await?;
        self.hub.state(&job);
        Ok(job)
    }
    fn wake(&self) {
        self.wake.notify_one();
    }
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
