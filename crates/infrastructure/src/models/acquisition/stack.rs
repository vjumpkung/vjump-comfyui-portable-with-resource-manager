//! Composition of the download adapters over one install root, shared by the desktop root and
//! its integration tests.
use super::{
    credentials::ProviderCredentialTester,
    finalize::{LocalDownloadFiles, StagingArea},
    hf_engine::HfDownloader,
    http::ArtifactHttp,
    native_engine::NativeDownloader,
    pack::ResourcePacks,
    progress::DownloadHub,
    resolver::ProviderResolver,
    sources::SourceIngress,
    sqlite::SqliteDownloads,
    supervisor::DownloadSupervisor,
};
use crate::{
    environment::layout::Layout,
    settings::secrets::CredentialVault,
    shared::{model_paths::AuthorizedModelRoots, path_locks::PathLocks, sqlite::Db},
};
use application::{
    models::acquisition::{DownloadError, Result},
    shared::{IdGenerator, path_locks::ModelPathLocks},
};
use domain::models::acquisition::DownloadErrorCode;
use std::{path::Path, sync::Arc};

/// The only authorized model root of this milestone: `<install root>/ComfyUI/models`.
pub const DEFAULT_MODEL_ROOT_ID: &str = "models";

pub struct DownloadStack {
    pub repository: Arc<SqliteDownloads>,
    pub resolver: Arc<ProviderResolver>,
    pub ingress: Arc<SourceIngress>,
    pub files: Arc<LocalDownloadFiles>,
    pub packs: Arc<ResourcePacks>,
    pub hub: Arc<DownloadHub>,
    pub supervisor: Arc<DownloadSupervisor>,
    pub tester: Arc<ProviderCredentialTester>,
}
impl DownloadStack {
    pub fn open(
        root: &Path,
        db: Db,
        vault: Arc<dyn CredentialVault>,
        concurrency: usize,
        locks: Arc<dyn ModelPathLocks>,
    ) -> Result<Self> {
        let layout = Layout::new(root);
        let models = layout.comfyui_dir().join("models");
        std::fs::create_dir_all(&models).map_err(|_| DownloadError(DownloadErrorCode::Disk))?;
        let roots = Arc::new(
            AuthorizedModelRoots::new([(DEFAULT_MODEL_ROOT_ID.to_owned(), models)])
                .map_err(|_| DownloadError(DownloadErrorCode::Containment))?,
        );
        let staging = Arc::new(
            StagingArea::new(&layout.downloads_dir())
                .map_err(|_| DownloadError(DownloadErrorCode::Disk))?,
        );
        let repository = Arc::new(SqliteDownloads { db: db.clone() });
        let http = ArtifactHttp::new(vault.clone())?;
        let ingress = Arc::new(SourceIngress::default());
        let resolver = Arc::new(ProviderResolver::new(http.clone(), ingress.clone()));
        let files = Arc::new(LocalDownloadFiles {
            roots,
            staging: staging.clone(),
            locks,
        });
        let hub = Arc::new(DownloadHub::new(
            crate::shared::clock::RandomIds::default().new_id(),
            256,
        ));
        let native = Arc::new(NativeDownloader {
            resolver: resolver.clone(),
            staging: staging.clone(),
            repository: repository.clone(),
        });
        let hf = Arc::new(HfDownloader {
            root: root.to_owned(),
            staging,
            vault: vault.clone(),
        });
        let supervisor = DownloadSupervisor::new(
            repository.clone(),
            files.clone(),
            native,
            hf,
            hub.clone(),
            concurrency,
        );
        let packs = Arc::new(ResourcePacks {
            http: http.clone(),
            ingress: ingress.clone(),
        });
        let tester = Arc::new(ProviderCredentialTester { http, vault, db });
        Ok(Self {
            repository,
            resolver,
            ingress,
            files,
            packs,
            hub,
            supervisor,
            tester,
        })
    }
}
/// Default lease registry shared with later library mutations.
pub fn path_locks() -> Arc<PathLocks> {
    Arc::new(PathLocks::default())
}
