use super::{
    civitai, direct, gdown_engine::GdownDownloader, gdrive, http::ArtifactHttp, huggingface,
    sources::SourceIngress,
};
use application::models::acquisition::{Resolution, ResolvedFile, Result, SourceResolver};
use async_trait::async_trait;
use domain::{models::acquisition::*, settings::CredentialKind};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
use url::Url;

pub struct ProviderResolver {
    pub http: ArtifactHttp,
    pub ingress: Arc<SourceIngress>,
    gdown: Option<Arc<GdownDownloader>>,
    next: AtomicU64,
    resolutions: Mutex<HashMap<u64, Resolution>>,
}
impl ProviderResolver {
    pub fn new(http: ArtifactHttp, ingress: Arc<SourceIngress>) -> Self {
        Self {
            http,
            ingress,
            gdown: None,
            next: AtomicU64::new(0),
            resolutions: Mutex::new(HashMap::new()),
        }
    }
    pub fn with_gdown(mut self, gdown: Arc<GdownDownloader>) -> Self {
        self.gdown = Some(gdown);
        self
    }
    pub(crate) async fn transfer_url(
        &self,
        source: &SourceRef,
    ) -> Result<(Url, Option<CredentialKind>, bool)> {
        match source {
            SourceRef::CivitAi {
                version_id: Some(v),
                file_id: Some(f),
                ..
            } => Ok((
                civitai::transfer_url(*v, *f),
                Some(CredentialKind::CivitAi),
                false,
            )),
            SourceRef::HuggingFace {
                repo_type,
                repo,
                revision,
                path: Some(path),
            } => Ok((
                huggingface::transfer_url(*repo_type, repo, revision, path),
                Some(CredentialKind::HuggingFace),
                false,
            )),
            SourceRef::GoogleDrive { file_id } => {
                Ok((gdrive::locate(&self.http, file_id).await?.0, None, false))
            }
            SourceRef::Direct {
                insecure_http_confirmed,
                ..
            } => Ok((
                self.ingress.executable(source)?,
                None,
                *insecure_http_confirmed,
            )),
            _ => Err(DownloadErrorCode::InvalidSource.into()),
        }
    }
}
#[async_trait]
impl SourceResolver for ProviderResolver {
    async fn resolve(&self, source: &SourceRef) -> Result<Resolution> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let files = match source {
            SourceRef::CivitAi { .. } => civitai::resolve(&self.http, source, now).await?,
            SourceRef::HuggingFace { .. } => huggingface::resolve(&self.http, source).await?,
            SourceRef::GoogleDrive { .. } => vec![match &self.gdown {
                Some(gdown) => match gdown.resolve(source).await {
                    Err(error) if error.0 == DownloadErrorCode::ToolLaunch => {
                        gdrive::resolve(&self.http, source).await?
                    }
                    result => result?,
                },
                None => gdrive::resolve(&self.http, source).await?,
            }],
            SourceRef::Direct { .. } => {
                vec![direct::resolve(&self.http, &self.ingress, source).await?]
            }
        };
        let resolution = Resolution {
            id: self.next.fetch_add(1, Ordering::Relaxed) + 1,
            selection_required: files.len() > 1,
            files,
        };
        let mut cache = self
            .resolutions
            .lock()
            .map_err(|_| DownloadErrorCode::Storage)?;
        if cache.len() >= 1000
            && let Some(oldest) = cache.keys().min().copied()
        {
            cache.remove(&oldest);
        }
        cache.insert(resolution.id, resolution.clone());
        Ok(resolution)
    }
    async fn selected(&self, resolution_id: u64, index: usize) -> Result<ResolvedFile> {
        self.resolutions
            .lock()
            .map_err(|_| DownloadErrorCode::Storage)?
            .get(&resolution_id)
            .and_then(|r| r.files.get(index))
            .cloned()
            .ok_or(DownloadErrorCode::SelectionNotFound.into())
    }
}
