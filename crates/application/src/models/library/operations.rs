//! Library workflows: recoverable rename and move groups, planned deletion and identification.
//! Every mutation holds path leases, revalidates identities and containment under them, and
//! records its intent before touching the file system (spec section 3.3.4).
use super::ports::*;
use crate::shared::{
    Clock, IdGenerator,
    path_locks::{PathLease, PathLockError, PathLocks},
};
use domain::{
    models::{
        acquisition::FileIdentity,
        library::{
            ModelLocation, ambiguous_stem_siblings, case_only_difference, renamed_sidecar,
            sidecar_names, validate_rename,
        },
    },
    shared::model_values::{RelativeModelPath, SafeFileName, Sha256},
};
use std::{
    collections::{HashSet, VecDeque},
    sync::{Arc, Mutex},
};

#[derive(Clone)]
pub struct LibraryDeps {
    pub catalog: Arc<dyn ModelCatalog>,
    pub files: Arc<dyn ModelFiles>,
    pub journal: Arc<dyn OperationJournal>,
    pub locks: Arc<dyn PathLocks>,
    pub civitai: Arc<dyn CivitaiLookup>,
    pub control: Arc<dyn LibraryControl>,
    pub details: Arc<dyn ModelDetailsReader>,
    pub ids: Arc<dyn IdGenerator>,
    pub clock: Arc<dyn Clock>,
    pub plans: Arc<DeletePlans>,
}

fn lock_error(error: PathLockError) -> LibraryError {
    match error {
        PathLockError::BusyReserved => LibraryErrorCode::BusyReserved.into(),
        PathLockError::Unavailable => LibraryErrorCode::Storage.into(),
    }
}
fn same_identity(a: &FileIdentity, b: &FileIdentity) -> bool {
    a.size == b.size && a.modified_ns == b.modified_ns && a.platform_id == b.platform_id
}
fn names_match(a: &str, b: &str) -> bool {
    a == b || (cfg!(windows) && a.eq_ignore_ascii_case(b))
}
fn directory_of(location: &ModelLocation) -> ModelLocation {
    ModelLocation::new(location.root_id.clone(), location.directory())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    pub location: ModelLocation,
}

impl LibraryDeps {
    async fn lock(&self, locations: Vec<ModelLocation>) -> Result<Box<dyn PathLease>> {
        self.locks.acquire_many(locations).await.map_err(lock_error)
    }

    /// Renames a model and its sidecars within the same folder.
    pub async fn rename(
        &self,
        location: &ModelLocation,
        new_name: &str,
        allow_extension_change: bool,
    ) -> Result<Placed> {
        let current = location
            .file_name()
            .ok_or(LibraryErrorCode::InvalidRequest)?;
        let new = validate_rename(current, new_name, allow_extension_change)?;
        let to = location.sibling(new);
        self.place_group(location, &to, OperationKind::Rename).await
    }

    /// Moves a model and its sidecars to another folder or root, keeping the file name.
    pub async fn move_to(
        &self,
        location: &ModelLocation,
        target_root: &str,
        target_directory: RelativeModelPath,
    ) -> Result<Placed> {
        let name = location
            .file_name()
            .ok_or(LibraryErrorCode::InvalidRequest)?
            .clone();
        let to = ModelLocation::new(target_root, target_directory.join(name));
        if !self.catalog.accepts(&to).await {
            return Err(LibraryErrorCode::OutsideRoot.into());
        }
        if to.key() == location.key() {
            return Err(LibraryErrorCode::Unchanged.into());
        }
        self.place_group(location, &to, OperationKind::Move).await
    }

    async fn members(
        &self,
        from: &ModelLocation,
        to: &ModelLocation,
        op_id: &str,
    ) -> Result<Vec<OperationMember>> {
        let name = from.file_name().ok_or(LibraryErrorCode::InvalidRequest)?;
        let new_name = to.file_name().ok_or(LibraryErrorCode::InvalidRequest)?;
        let listing = self.files.list_directory(&directory_of(from)).await?;
        let mut found = Vec::new();
        for candidate in sidecar_names(name) {
            if let Some(listed) = listing
                .iter()
                .find(|n| names_match(n.as_str(), candidate.as_str()))
            {
                found.push(listed.clone());
            }
        }
        if !found.is_empty()
            && !ambiguous_stem_siblings(name.as_str(), listing.iter().map(SafeFileName::as_str))
                .is_empty()
        {
            return Err(LibraryErrorCode::AmbiguousSidecar.into());
        }
        let target_directory = to.directory();
        let mut pairs = vec![(name.clone(), new_name.clone(), true)];
        for sidecar in found {
            let renamed =
                renamed_sidecar(new_name, &sidecar).ok_or(LibraryErrorCode::InvalidName)?;
            pairs.push((sidecar, renamed, false));
        }
        let same_directory = from.directory() == target_directory;
        let mut members = Vec::new();
        for (index, (old, new, model)) in pairs.into_iter().enumerate() {
            let member_from = from.sibling(old.clone());
            let member_to =
                ModelLocation::new(to.root_id.clone(), target_directory.join(new.clone()));
            let via = (cfg!(windows)
                && same_directory
                && from.root_id == to.root_id
                && case_only_difference(old.as_str(), new.as_str()))
            .then(|| {
                SafeFileName::new(format!(".vjump-rename-{op_id}-{index}.tmp"))
                    .map(|temporary| from.sibling(temporary))
            })
            .transpose()
            .map_err(|_| LibraryErrorCode::InvalidName)?;
            members.push(OperationMember {
                from: member_from,
                to: member_to,
                via,
                model,
            });
        }
        Ok(members)
    }

    async fn touched_by_unfinished(&self, locations: &[ModelLocation]) -> Result<bool> {
        let keys: HashSet<String> = locations.iter().map(ModelLocation::key).collect();
        Ok(self
            .journal
            .unfinished()
            .await?
            .iter()
            .any(|record| record.locations().iter().any(|l| keys.contains(&l.key()))))
    }

    async fn place_group(
        &self,
        from: &ModelLocation,
        to: &ModelLocation,
        kind: OperationKind,
    ) -> Result<Placed> {
        let row = self
            .catalog
            .get(from)
            .await?
            .ok_or(LibraryErrorCode::NotFound)?;
        let op_id = self.ids.new_id();
        // The member list depends on the directory content, so it is built once to know which
        // paths to lease and again under the lease; a difference means the folder changed.
        let preview = self.members(from, to, &op_id).await?;
        let mut leased: Vec<ModelLocation> = Vec::new();
        for member in &preview {
            leased.push(member.from.clone());
            leased.push(member.to.clone());
            leased.extend(member.via.clone());
        }
        let _lease = self.lock(leased.clone()).await?;
        if self.touched_by_unfinished(&leased).await? {
            return Err(LibraryErrorCode::RecoveryRequired.into());
        }
        let members = self.members(from, to, &op_id).await?;
        if members != preview {
            return Err(LibraryErrorCode::Stale.into());
        }
        let live = self
            .files
            .inspect(from)
            .await?
            .ok_or(LibraryErrorCode::NotFound)?;
        if live.size != row.size || live.modified_ns != row.modified_ns {
            return Err(LibraryErrorCode::Stale.into());
        }
        for member in &members {
            if member.via.is_none() && self.files.inspect(&member.to).await?.is_some() {
                return Err(LibraryErrorCode::TargetExists.into());
            }
            if let Some(via) = &member.via
                && self.files.inspect(via).await?.is_some()
            {
                return Err(LibraryErrorCode::TargetExists.into());
            }
            // Sharing and permission failures surface before anything changed.
            self.files.check_unlocked(&member.from).await?;
        }
        let record = OperationRecord {
            id: op_id.clone(),
            kind,
            phase: OperationPhase::Intent,
            members: members.clone(),
            created_at: self.clock.now(),
        };
        self.journal.begin(&record).await?;
        let mut done: Vec<(ModelLocation, ModelLocation)> = Vec::new();
        let mut retained = false;
        for member in &members {
            let steps = match &member.via {
                Some(via) => vec![(&member.from, via), (via, &member.to)],
                None => vec![(&member.from, &member.to)],
            };
            for (a, b) in steps {
                let expected = member.model.then_some(&live).filter(|_| *a == member.from);
                match self.files.relocate(a, b, expected, &op_id).await {
                    Ok(outcome) => {
                        retained |= outcome == Relocated::CopiedSourceRetained;
                        done.push((a.clone(), b.clone()));
                    }
                    Err(error) => return self.compensate(&op_id, done, error).await,
                }
            }
        }
        self.catalog.apply_operation(&op_id, from, to).await?;
        self.control.changed();
        if retained {
            return Err(LibraryErrorCode::SourceRetained.into());
        }
        Ok(Placed {
            location: to.clone(),
        })
    }

    /// Restores completed steps in reverse. A partial restore is never reported as "unchanged":
    /// the journal entry stays and the caller learns that recovery is required.
    async fn compensate(
        &self,
        op_id: &str,
        done: Vec<(ModelLocation, ModelLocation)>,
        error: LibraryError,
    ) -> Result<Placed> {
        let mut restored = true;
        for (a, b) in done.iter().rev() {
            if self.files.relocate(b, a, None, op_id).await.is_err() {
                restored = false;
                break;
            }
        }
        if restored {
            self.journal.complete(op_id).await?;
            Err(error)
        } else {
            let _ = self
                .journal
                .set_phase(op_id, OperationPhase::RecoveryRequired)
                .await;
            Err(LibraryErrorCode::RecoveryRequired.into())
        }
    }

    /// Finishes interrupted operations before any conflicting mutation is admitted.
    pub async fn reconcile(&self) -> Result<ReconcileReport> {
        let mut report = ReconcileReport::default();
        for record in self.journal.unfinished().await? {
            let Ok(_lease) = self.lock(record.locations()).await else {
                report.blocked += 1;
                continue;
            };
            match self.roll_forward(&record).await {
                Ok(()) => report.completed += 1,
                Err(_) => {
                    let _ = self
                        .journal
                        .set_phase(&record.id, OperationPhase::RecoveryRequired)
                        .await;
                    report.blocked += 1;
                }
            }
        }
        if report.completed > 0 {
            self.control.changed();
        }
        Ok(report)
    }

    async fn roll_forward(&self, record: &OperationRecord) -> Result<()> {
        for member in &record.members {
            let from = self.files.inspect(&member.from).await?.is_some();
            let to = self.files.inspect(&member.to).await?.is_some();
            let via = match &member.via {
                Some(via) => self.files.inspect(via).await?.is_some(),
                None => false,
            };
            match (from, to, via) {
                (false, true, false) => {}
                (true, false, false) => match &member.via {
                    Some(via) => {
                        self.files
                            .relocate(&member.from, via, None, &record.id)
                            .await?;
                        self.files
                            .relocate(via, &member.to, None, &record.id)
                            .await?;
                    }
                    None => {
                        self.files
                            .relocate(&member.from, &member.to, None, &record.id)
                            .await?;
                    }
                },
                (false, false, true) => {
                    let via = member
                        .via
                        .as_ref()
                        .ok_or(LibraryErrorCode::RecoveryRequired)?;
                    self.files
                        .relocate(via, &member.to, None, &record.id)
                        .await?;
                }
                _ => return Err(LibraryErrorCode::RecoveryRequired.into()),
            }
        }
        let model = record
            .members
            .iter()
            .find(|m| m.model)
            .ok_or(LibraryErrorCode::RecoveryRequired)?;
        self.catalog
            .apply_operation(&record.id, &model.from, &model.to)
            .await
    }

    /// Builds a confirmation bound to the current identities of the selected model files.
    pub async fn plan_delete(
        &self,
        locations: Vec<ModelLocation>,
        kind: DeleteKind,
    ) -> Result<DeletePlan> {
        let mut seen = HashSet::new();
        let mut items = Vec::new();
        for location in locations {
            if !seen.insert(location.key()) {
                continue;
            }
            let identity = self
                .files
                .inspect(&location)
                .await?
                .ok_or(LibraryErrorCode::NotFound)?;
            items.push(DeleteItem {
                name: location
                    .file_name()
                    .map_or_else(String::new, |n| n.as_str().to_owned()),
                size: identity.size,
                location,
                identity,
            });
        }
        if items.is_empty() {
            return Err(LibraryErrorCode::InvalidRequest.into());
        }
        Ok(self.store_plan(kind, items))
    }

    fn store_plan(&self, kind: DeleteKind, items: Vec<DeleteItem>) -> DeletePlan {
        let plan = DeletePlan {
            id: self.ids.new_id(),
            kind,
            total_bytes: items.iter().map(|i| i.size).sum(),
            items,
        };
        self.plans.insert(plan.clone());
        plan
    }

    /// Executes a confirmed plan. Each file is handled independently and its catalog row is
    /// removed only after that file was deleted. A trash failure never turns into a permanent
    /// delete: it yields a separate permanent-delete plan that needs its own confirmation.
    pub async fn execute_delete(&self, plan_id: &str, kind: DeleteKind) -> Result<DeleteReport> {
        let plan = self
            .plans
            .take(plan_id)
            .ok_or(LibraryErrorCode::StaleDecision)?;
        if plan.kind != kind {
            return Err(LibraryErrorCode::StaleDecision.into());
        }
        let mut results = Vec::new();
        let mut failed = Vec::new();
        for item in &plan.items {
            let outcome = self.delete_one(item, kind).await;
            if outcome == DeleteOutcome::TrashFailed {
                failed.push(item.clone());
            }
            results.push(DeleteResult {
                location: item.location.clone(),
                outcome,
            });
        }
        let permanent_plan = (!failed.is_empty() && kind == DeleteKind::Trash)
            .then(|| self.store_plan(DeleteKind::Permanent, failed));
        self.control.changed();
        Ok(DeleteReport {
            results,
            permanent_plan,
        })
    }

    async fn delete_one(&self, item: &DeleteItem, kind: DeleteKind) -> DeleteOutcome {
        let lease = match self.lock(vec![item.location.clone()]).await {
            Ok(lease) => lease,
            Err(error) => return DeleteOutcome::from_error(error),
        };
        let outcome = async {
            let live = self.files.inspect(&item.location).await?;
            let Some(live) = live else {
                return Ok(DeleteOutcome::Missing);
            };
            if !same_identity(&live, &item.identity) {
                return Ok(DeleteOutcome::Stale);
            }
            self.files.check_unlocked(&item.location).await?;
            match kind {
                DeleteKind::Trash => self.files.trash(&item.location, &item.identity).await?,
                DeleteKind::Permanent => {
                    self.files
                        .delete_permanently(&item.location, &item.identity)
                        .await?;
                }
            }
            self.catalog.remove(&item.location).await?;
            Ok::<_, LibraryError>(DeleteOutcome::Deleted)
        }
        .await;
        drop(lease);
        outcome.unwrap_or_else(DeleteOutcome::from_error)
    }

    /// Hashes the file, asks CivitAI, and writes the curated sidecars. The lease is released
    /// during the network requests and the identity is verified again before anything is written.
    pub async fn identify(
        &self,
        location: &ModelLocation,
        replace_existing: bool,
    ) -> Result<IdentifyOutcome> {
        let row = self
            .catalog
            .get(location)
            .await?
            .ok_or(LibraryErrorCode::NotFound)?;
        let (identity, sha256) = {
            let _lease = self.lock(vec![location.clone()]).await?;
            let live = self
                .files
                .inspect(location)
                .await?
                .ok_or(LibraryErrorCode::NotFound)?;
            if live.size != row.size || live.modified_ns != row.modified_ns {
                return Err(LibraryErrorCode::Stale.into());
            }
            let sha256 = match self.catalog.cached_hash(location, &live).await? {
                Some(hash) => hash,
                None => {
                    let hash = self.files.hash(location, &live).await?;
                    self.catalog.record_hash(location, &live, &hash).await?;
                    hash
                }
            };
            (live, sha256)
        };
        let Some(found) = self.civitai.by_hash(&sha256).await? else {
            self.control.changed();
            return Ok(IdentifyOutcome::NotIdentified { sha256 });
        };
        let preview = self.civitai.preview_png(&found.version).await?;
        let _lease = self.lock(vec![location.clone()]).await?;
        let live = self
            .files
            .inspect(location)
            .await?
            .ok_or(LibraryErrorCode::NotFound)?;
        if !same_identity(&live, &identity) {
            // A response for a changed file is discarded.
            return Err(LibraryErrorCode::Stale.into());
        }
        let name = location
            .file_name()
            .ok_or(LibraryErrorCode::InvalidRequest)?;
        let siblings = self.files.list_directory(&directory_of(location)).await?;
        let sidecars = sidecar_names(name);
        let info_name = sidecars[1].clone();
        let preview_name = sidecars[0].clone();
        let exists = |wanted: &SafeFileName| {
            siblings
                .iter()
                .any(|n| names_match(n.as_str(), wanted.as_str()))
        };
        if !replace_existing && (exists(&info_name) || exists(&preview_name)) {
            return Ok(IdentifyOutcome::SidecarsExist {
                version: found.version,
                sha256,
            });
        }
        self.files
            .write_sidecar(
                &location.sibling(info_name),
                &found.info_json,
                replace_existing,
            )
            .await?;
        let preview_outcome = match preview {
            None => PreviewOutcome::Missing,
            Some(bytes) => match self
                .files
                .write_sidecar(&location.sibling(preview_name), &bytes, replace_existing)
                .await
            {
                Ok(()) => PreviewOutcome::Written,
                Err(error) => PreviewOutcome::Failed(error.0),
            },
        };
        self.catalog
            .record_identification(location, &live, &sha256, found.version.version_id)
            .await?;
        self.control.changed();
        Ok(IdentifyOutcome::Identified {
            version: found.version,
            sha256,
            preview: preview_outcome,
        })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReconcileReport {
    pub completed: usize,
    pub blocked: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteKind {
    Trash,
    Permanent,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteItem {
    pub location: ModelLocation,
    pub name: String,
    pub size: u64,
    pub identity: FileIdentity,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletePlan {
    pub id: String,
    pub kind: DeleteKind,
    pub items: Vec<DeleteItem>,
    pub total_bytes: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteOutcome {
    Deleted,
    Locked,
    TrashFailed,
    Stale,
    Missing,
    Busy,
    Failed(LibraryErrorCode),
}
impl DeleteOutcome {
    fn from_error(error: LibraryError) -> Self {
        match error.0 {
            LibraryErrorCode::Locked => Self::Locked,
            LibraryErrorCode::TrashFailed => Self::TrashFailed,
            LibraryErrorCode::Stale => Self::Stale,
            LibraryErrorCode::NotFound => Self::Missing,
            LibraryErrorCode::BusyReserved => Self::Busy,
            other => Self::Failed(other),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteResult {
    pub location: ModelLocation,
    pub outcome: DeleteOutcome,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteReport {
    pub results: Vec<DeleteResult>,
    pub permanent_plan: Option<DeletePlan>,
}

/// Single-use confirmations. A plan older than the newest sixteen is forgotten.
#[derive(Default)]
pub struct DeletePlans {
    plans: Mutex<VecDeque<DeletePlan>>,
}
impl DeletePlans {
    pub fn insert(&self, plan: DeletePlan) {
        let mut plans = self.plans.lock().unwrap_or_else(|p| p.into_inner());
        plans.push_back(plan);
        while plans.len() > 16 {
            plans.pop_front();
        }
    }
    pub fn take(&self, id: &str) -> Option<DeletePlan> {
        let mut plans = self.plans.lock().unwrap_or_else(|p| p.into_inner());
        let index = plans.iter().position(|p| p.id == id)?;
        plans.remove(index)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewOutcome {
    Written,
    Missing,
    Failed(LibraryErrorCode),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentifyOutcome {
    Identified {
        version: CivitaiVersion,
        sha256: Sha256,
        preview: PreviewOutcome,
    },
    NotIdentified {
        sha256: Sha256,
    },
    /// Sidecars exist; nothing was written. Repeat with an explicit replacement decision.
    SidecarsExist {
        version: CivitaiVersion,
        sha256: Sha256,
    },
}
