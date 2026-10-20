//! Composition of the download adapters over one install root, shared by the desktop root and
//! its integration tests.
use super::{
    bandwidth::BandwidthLimiter,
    credentials::ProviderCredentialTester,
    finalize::{LocalDownloadFiles, StagingArea},
    hf_engine::HfDownloader,
    hf_proxy::HfProxyConfig,
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
use std::{num::NonZeroU32, path::Path, sync::Arc};

/// The only authorized model root of this milestone: `<install root>/ComfyUI/models`.
pub const DEFAULT_MODEL_ROOT_ID: &str = "models";

pub struct DownloadStack {
    pub bandwidth: Arc<BandwidthLimiter>,
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
        Self::open_with_bandwidth(root, db, vault, concurrency, locks, None, true)
    }

    pub fn open_with_bandwidth(
        root: &Path,
        db: Db,
        vault: Arc<dyn CredentialVault>,
        concurrency: usize,
        locks: Arc<dyn ModelPathLocks>,
        limit_kib_s: Option<NonZeroU32>,
        create_default_root: bool,
    ) -> Result<Self> {
        let bandwidth = Arc::new(BandwidthLimiter::new(limit_kib_s));
        let layout = Layout::new(root);
        // Before the first ComfyUI clone nothing may exist under `ComfyUI/`, so the stack opens
        // without a model root and downloads are refused until the next start.
        let default_root = if create_default_root {
            let models = layout.comfyui_dir().join("models");
            std::fs::create_dir_all(&models).map_err(|_| DownloadError(DownloadErrorCode::Disk))?;
            vec![(DEFAULT_MODEL_ROOT_ID.to_owned(), models)]
        } else {
            Vec::new()
        };
        let roots = Arc::new(
            AuthorizedModelRoots::new(default_root)
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
            bandwidth: bandwidth.clone(),
            resolver: resolver.clone(),
            staging: staging.clone(),
            repository: repository.clone(),
        });
        let hf = Arc::new(HfDownloader {
            bandwidth: bandwidth.clone(),
            proxy: HfProxyConfig::capture(),
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
            bandwidth,
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
