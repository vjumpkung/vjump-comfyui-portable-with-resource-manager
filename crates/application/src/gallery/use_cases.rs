use super::{model::*, ports::*};
use crate::{
    environment::RootSwitcher,
    shared::{ExplorerTarget, IdGenerator, MaintenanceGate},
};
use domain::{
    gallery::{GalleryLocation, STAGING_DIR},
    shared::model_values::SafeFileName,
};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{Arc, Mutex},
};

/// The most entries one transfer or delete request may name.
pub const MAX_ITEMS: usize = 500;
const MAX_PLANS: usize = 8;

/// Delete plans waiting for confirmation. They live only in this process, so a restart
/// invalidates them, and the newest eight are kept.
#[derive(Default)]
pub struct GalleryDeletePlans(Mutex<VecDeque<DeletePlan>>);
impl GalleryDeletePlans {
    pub fn insert(&self, plan: DeletePlan) {
        let mut plans = self.0.lock().unwrap_or_else(|p| p.into_inner());
        plans.push_back(plan);
        while plans.len() > MAX_PLANS {
            plans.pop_front();
        }
    }
    /// Hands the plan out exactly once.
    pub fn take(&self, id: &str) -> Option<DeletePlan> {
        let mut plans = self.0.lock().unwrap_or_else(|p| p.into_inner());
        let index = plans.iter().position(|plan| plan.id == id)?;
        plans.remove(index)
    }
}

/// Every port the gallery workflows use. Each use case holds a clone.
#[derive(Clone)]
pub struct GalleryDeps {
    pub root: Arc<dyn RootSwitcher>,
    pub files: Arc<dyn GalleryFiles>,
    pub gate: Arc<dyn MaintenanceGate>,
    pub ids: Arc<dyn IdGenerator>,
    pub plans: Arc<GalleryDeletePlans>,
}
impl GalleryDeps {
    fn root(&self) -> PathBuf {
        self.root.current().root
    }
}

fn invalid_request(source: &GalleryEntryRef) -> ItemResult {
    failed(source, GalleryErrorCode::InvalidRequest.into())
}
fn failed(source: &GalleryEntryRef, error: GalleryError) -> ItemResult {
    ItemResult {
        source: source.location.clone(),
        destination: None,
        outcome: ItemOutcome::Failed,
        error: Some(error),
    }
}
/// Keeps the first reference to each location.
fn deduplicated(items: Vec<GalleryEntryRef>) -> Vec<GalleryEntryRef> {
    let mut unique: Vec<GalleryEntryRef> = Vec::with_capacity(items.len());
    for item in items {
        if !unique.iter().any(|seen| seen.location == item.location) {
            unique.push(item);
        }
    }
    unique
}
/// `outer` is a strict ancestor of `inner` with every component equal exactly. Folders that
/// differ only by case are distinct on a case-sensitive volume, so a delete plan must not treat
/// one as holding the other; the case-insensitive `contains` stays for the move/copy safety check.
fn is_exact_ancestor(outer: &GalleryLocation, inner: &GalleryLocation) -> bool {
    let (outer_parts, inner_parts) = (outer.path().components(), inner.path().components());
    outer.area() == inner.area()
        && outer_parts.len() < inner_parts.len()
        && inner_parts.starts_with(outer_parts)
}
fn within_limits(items: &[GalleryEntryRef]) -> Result<()> {
    if items.is_empty() || items.len() > MAX_ITEMS {
        Err(GalleryErrorCode::InvalidRequest.into())
    } else {
        Ok(())
    }
}

pub struct ListGallery {
    deps: GalleryDeps,
}
impl ListGallery {
    pub fn new(deps: GalleryDeps) -> Self {
        Self { deps }
    }
    pub async fn execute(&self, directory: &GalleryLocation) -> Result<GalleryListing> {
        self.deps.files.list(&self.deps.root(), directory).await
    }
}

pub struct RenameGalleryEntry {
    deps: GalleryDeps,
}
impl RenameGalleryEntry {
    pub fn new(deps: GalleryDeps) -> Self {
        Self { deps }
    }
    pub async fn execute(
        &self,
        entry: &GalleryEntryRef,
        new_name: &str,
    ) -> Result<GalleryLocation> {
        let _lease = self.deps.gate.try_acquire().await?;
        let current = entry
            .location
            .name()
            .ok_or(GalleryErrorCode::InvalidRequest)?;
        let new = SafeFileName::new(new_name).map_err(|_| GalleryErrorCode::InvalidName)?;
        if new.as_str().eq_ignore_ascii_case(STAGING_DIR) {
            return Err(GalleryErrorCode::InvalidName.into());
        }
        if new == *current {
            return Ok(entry.location.clone());
        }
        self.deps.files.rename(&self.deps.root(), entry, &new).await
    }
}

/// Copies or moves a batch into one folder. Every item reports its own outcome.
pub struct TransferGalleryEntries {
    deps: GalleryDeps,
}
impl TransferGalleryEntries {
    pub fn new(deps: GalleryDeps) -> Self {
        Self { deps }
    }
    pub async fn execute(
        &self,
        items: Vec<GalleryEntryRef>,
        destination: GalleryLocation,
        mode: TransferMode,
    ) -> Result<Vec<ItemResult>> {
        within_limits(&items)?;
        let lease = self.deps.gate.try_acquire().await?;
        let root = self.deps.root();
        // Leftovers of an interrupted copy are settled before any new staging directory exists.
        self.deps.files.cleanup_staging(&root).await;
        let mut results = Vec::new();
        for item in deduplicated(items) {
            if item.location.is_root() || item.location.contains(&destination) {
                results.push(invalid_request(&item));
            } else if mode == TransferMode::Move
                && item.location.parent().as_ref() == Some(&destination)
            {
                results.push(ItemResult {
                    source: item.location.clone(),
                    destination: Some(item.location.clone()),
                    outcome: ItemOutcome::Unchanged,
                    error: None,
                });
            } else {
                let operation = self.deps.ids.new_id();
                results.push(
                    self.deps
                        .files
                        .transfer(
                            &root,
                            &item,
                            &destination,
                            mode,
                            &operation,
                            lease.retained(),
                        )
                        .await,
                );
            }
        }
        Ok(results)
    }
}

pub struct PlanGalleryDelete {
    deps: GalleryDeps,
}
impl PlanGalleryDelete {
    pub fn new(deps: GalleryDeps) -> Self {
        Self { deps }
    }
    pub async fn execute(&self, items: Vec<GalleryEntryRef>) -> Result<DeletePlan> {
        within_limits(&items)?;
        if items.iter().any(|item| item.location.is_root()) {
            return Err(GalleryErrorCode::InvalidRequest.into());
        }
        let items = deduplicated(items);
        // Recycling a folder takes everything below it, so selected descendants are redundant.
        let outermost: Vec<&GalleryEntryRef> = items
            .iter()
            .filter(|item| {
                !items
                    .iter()
                    .any(|other| is_exact_ancestor(&other.location, &item.location))
            })
            .collect();
        let root = self.deps.root();
        let mut planned = Vec::with_capacity(outermost.len());
        for item in outermost {
            planned.push(self.deps.files.plan_delete(&root, item).await?);
        }
        let plan = DeletePlan {
            id: self.deps.ids.new_id(),
            total_bytes: planned
                .iter()
                .fold(0u64, |total, item| total.saturating_add(item.bytes)),
            items: planned,
        };
        self.deps.plans.insert(plan.clone());
        Ok(plan)
    }
}

/// Runs a confirmed plan. Entries only ever go to the recycle bin; there is no other path.
pub struct DeleteGalleryEntries {
    deps: GalleryDeps,
}
impl DeleteGalleryEntries {
    pub fn new(deps: GalleryDeps) -> Self {
        Self { deps }
    }
    pub async fn execute(&self, plan_id: &str) -> Result<Vec<ItemResult>> {
        // A busy gate leaves the plan usable for a retry.
        let _lease = self.deps.gate.try_acquire().await?;
        let plan = self
            .deps
            .plans
            .take(plan_id)
            .ok_or(GalleryErrorCode::Stale)?;
        let root = self.deps.root();
        let mut results = Vec::with_capacity(plan.items.len());
        for item in &plan.items {
            let source = item.entry.location.clone();
            results.push(match self.deps.files.trash(&root, item).await {
                Ok(()) => ItemResult {
                    source,
                    destination: None,
                    outcome: ItemOutcome::Succeeded,
                    error: None,
                },
                Err(error) => failed(&item.entry, error),
            });
        }
        Ok(results)
    }
}

pub struct OpenGalleryMedia {
    deps: GalleryDeps,
}
impl OpenGalleryMedia {
    pub fn new(deps: GalleryDeps) -> Self {
        Self { deps }
    }
    pub async fn execute(&self, location: &GalleryLocation) -> Result<Box<dyn GalleryMedia>> {
        self.deps
            .files
            .open_media(&self.deps.root(), location)
            .await
    }
}

pub struct LocateGalleryEntry {
    deps: GalleryDeps,
}
impl LocateGalleryEntry {
    pub fn new(deps: GalleryDeps) -> Self {
        Self { deps }
    }
    pub async fn execute(&self, location: &GalleryLocation) -> Result<ExplorerTarget> {
        self.deps
            .files
            .explorer_target(&self.deps.root(), location)
            .await
    }
}
