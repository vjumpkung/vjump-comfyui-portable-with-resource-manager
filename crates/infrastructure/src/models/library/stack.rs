//! Composition of the library adapters over one install root, shared by the desktop root and its
//! integration tests.
use super::{
    file_ops::LocalModelFiles,
    identify::{CivitaiClient, SidecarDetails},
    roots::LibraryRoots,
    sqlite::{SqliteCatalog, SqliteJournal},
    supervisor::LibrarySupervisor,
};
use crate::{
    models::acquisition::{http::ArtifactHttp, sqlite::SqliteDownloads},
    settings::secrets::CredentialVault,
    shared::{
        clock::{RandomIds, SystemClock},
        path_locks::PathLocks,
        sqlite::Db,
    },
};
use application::{
    comfy_runtime::ExtraModelPaths,
    models::library::{LibraryDeps, LibraryError, LibraryErrorCode, Result},
};
use std::{path::Path, sync::Arc};

pub struct LibraryStack {
    pub deps: LibraryDeps,
    pub supervisor: Arc<LibrarySupervisor>,
    pub roots: Arc<LibraryRoots>,
    pub catalog: Arc<SqliteCatalog>,
}

impl LibraryStack {
    /// `locks` is the same registry the download stack publishes through; its durable
    /// destination reservations are supplied by the download repository. Without
    /// `create_default_root` nothing is created under `ComfyUI/` and the roots stay empty until a
    /// later refresh finds the folder.
    pub fn open(
        root: &Path,
        db: Db,
        vault: Arc<dyn CredentialVault>,
        locks: Arc<PathLocks>,
        extra: Arc<dyn ExtraModelPaths>,
        downloads: Arc<SqliteDownloads>,
        create_default_root: bool,
    ) -> Result<Self> {
        let roots = if create_default_root {
            LibraryRoots::new(root, extra, locks.clone())
        } else {
            LibraryRoots::deferred(root, extra, locks.clone())
        };
        let roots = Arc::new(roots.map_err(|_| LibraryError(LibraryErrorCode::Io))?);
        locks.set_reservations(downloads);
        let catalog = Arc::new(SqliteCatalog {
            db: db.clone(),
            roots: roots.clone(),
        });
        let files = Arc::new(LocalModelFiles {
            roots: roots.clone(),
        });
        let http = ArtifactHttp::new(vault).map_err(|_| LibraryError(LibraryErrorCode::Network))?;
        let supervisor = LibrarySupervisor::new(catalog.clone(), roots.clone());
        let deps = LibraryDeps {
            catalog: catalog.clone(),
            files: files.clone(),
            journal: Arc::new(SqliteJournal { db }),
            locks,
            civitai: Arc::new(CivitaiClient::new(http)),
            control: supervisor.clone(),
            details: Arc::new(SidecarDetails { files }),
            ids: Arc::new(RandomIds::default()),
            clock: Arc::new(SystemClock),
            plans: Arc::default(),
        };
        Ok(Self {
            deps,
            supervisor,
            roots,
            catalog,
        })
    }

    /// Starts the scan loop on the current runtime.
    pub fn start(&self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(self.supervisor.clone().run())
    }
}
