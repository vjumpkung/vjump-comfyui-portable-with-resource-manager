use super::{operations::*, ports::*};
use domain::{models::library::ModelLocation, shared::model_values::RelativeModelPath};

pub struct ListModels {
    deps: LibraryDeps,
}
impl ListModels {
    pub fn new(deps: LibraryDeps) -> Self {
        Self { deps }
    }
    pub async fn execute(&self) -> Result<LibrarySnapshot> {
        Ok(LibrarySnapshot {
            revision: self.deps.catalog.revision().await?,
            models: self.deps.catalog.list().await?,
            status: self.deps.control.status(),
        })
    }
}

/// Manual rescan. Interrupted operations are finished first so the scan sees settled files.
pub struct RescanModels {
    deps: LibraryDeps,
}
impl RescanModels {
    pub fn new(deps: LibraryDeps) -> Self {
        Self { deps }
    }
    pub async fn execute(&self) -> Result<LibrarySnapshot> {
        self.deps.reconcile().await?;
        self.deps.control.rescan().await?;
        ListModels::new(self.deps.clone()).execute().await
    }
}

pub struct ReconcileModelOperations {
    deps: LibraryDeps,
}
impl ReconcileModelOperations {
    pub fn new(deps: LibraryDeps) -> Self {
        Self { deps }
    }
    pub async fn execute(&self) -> Result<ReconcileReport> {
        self.deps.reconcile().await
    }
}

pub struct GetOperationStatus {
    deps: LibraryDeps,
}
impl GetOperationStatus {
    pub fn new(deps: LibraryDeps) -> Self {
        Self { deps }
    }
    pub async fn execute(&self) -> Result<Vec<OperationRecord>> {
        self.deps.journal.unfinished().await
    }
}

pub struct RenameModel {
    deps: LibraryDeps,
}
impl RenameModel {
    pub fn new(deps: LibraryDeps) -> Self {
        Self { deps }
    }
    pub async fn execute(
        &self,
        location: &ModelLocation,
        new_name: &str,
        allow_extension_change: bool,
    ) -> Result<Placed> {
        self.deps
            .rename(location, new_name, allow_extension_change)
            .await
    }
}

pub struct MoveModel {
    deps: LibraryDeps,
}
impl MoveModel {
    pub fn new(deps: LibraryDeps) -> Self {
        Self { deps }
    }
    pub async fn execute(
        &self,
        location: &ModelLocation,
        target_root: &str,
        target_directory: RelativeModelPath,
    ) -> Result<Placed> {
        self.deps
            .move_to(location, target_root, target_directory)
            .await
    }
}

pub struct PlanDelete {
    deps: LibraryDeps,
}
impl PlanDelete {
    pub fn new(deps: LibraryDeps) -> Self {
        Self { deps }
    }
    pub async fn execute(&self, locations: Vec<ModelLocation>) -> Result<DeletePlan> {
        self.deps.plan_delete(locations, DeleteKind::Trash).await
    }
}

pub struct DeleteModels {
    deps: LibraryDeps,
}
impl DeleteModels {
    pub fn new(deps: LibraryDeps) -> Self {
        Self { deps }
    }
    pub async fn execute(&self, plan_id: &str) -> Result<DeleteReport> {
        self.deps.execute_delete(plan_id, DeleteKind::Trash).await
    }
}

/// Only runs plans that the user confirmed separately after a trash failure.
pub struct DeleteModelsPermanently {
    deps: LibraryDeps,
}
impl DeleteModelsPermanently {
    pub fn new(deps: LibraryDeps) -> Self {
        Self { deps }
    }
    pub async fn execute(&self, plan_id: &str) -> Result<DeleteReport> {
        self.deps
            .execute_delete(plan_id, DeleteKind::Permanent)
            .await
    }
}

pub struct IdentifyModel {
    deps: LibraryDeps,
}
impl IdentifyModel {
    pub fn new(deps: LibraryDeps) -> Self {
        Self { deps }
    }
    pub async fn execute(
        &self,
        location: &ModelLocation,
        replace_existing: bool,
    ) -> Result<IdentifyOutcome> {
        self.deps.identify(location, replace_existing).await
    }
}

pub struct GetModelDetails {
    deps: LibraryDeps,
}
impl GetModelDetails {
    pub fn new(deps: LibraryDeps) -> Self {
        Self { deps }
    }
    pub async fn execute(&self, location: &ModelLocation) -> Result<ModelDetails> {
        self.deps
            .catalog
            .get(location)
            .await?
            .ok_or(LibraryErrorCode::NotFound)?;
        self.deps.details.read(location).await
    }
}

pub struct GetLibraryStatus {
    deps: LibraryDeps,
}
impl GetLibraryStatus {
    pub fn new(deps: LibraryDeps) -> Self {
        Self { deps }
    }
    pub fn execute(&self) -> LibraryStatus {
        self.deps.control.status()
    }
}
