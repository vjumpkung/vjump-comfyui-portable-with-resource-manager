use super::*;
use crate::{
    environment::{Retained, StepContext},
    shared::ExplorerTarget,
};
use async_trait::async_trait;
use domain::{custom_nodes::*, environment::PythonEnvironment};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

#[async_trait]
pub trait RegistryClient: Send + Sync {
    async fn search(
        &self,
        query: RegistryQuery,
        refresh: bool,
        ttl_seconds: u64,
    ) -> Result<CachedPage, NodeAppError>;
    async fn node(&self, id: &str) -> Result<RegistryNode, NodeAppError>;
    async fn versions(&self, id: &str) -> Result<Vec<RegistryRelease>, NodeAppError>;
    async fn install(&self, id: &str, exact: Option<&str>)
    -> Result<RegistryRelease, NodeAppError>;
}
#[async_trait]
pub trait NodeRepository: Send + Sync {
    async fn nodes(&self) -> Result<Vec<InstalledNode>, NodeAppError>;
    async fn save_node(&self, node: &InstalledNode) -> Result<(), NodeAppError>;
    async fn remove_node(&self, id: &NodeId) -> Result<(), NodeAppError>;
    async fn save_operation(&self, operation: &NodeOperation) -> Result<(), NodeAppError>;
    /// Oldest creation first; equal timestamps retain insertion order.
    async fn operations(&self) -> Result<Vec<NodeOperation>, NodeAppError>;
    async fn dependency(
        &self,
        node: &NodeId,
        environment: &domain::environment::EnvId,
        source_identity: &str,
    ) -> Result<Option<DependencyState>, NodeAppError>;
    async fn save_dependency(&self, outcome: &DependencyOutcome) -> Result<(), NodeAppError>;
    async fn trust(&self, namespace: &str) -> Result<Option<Trust>, NodeAppError>;
    async fn save_trust(&self, trust: &Trust) -> Result<(), NodeAppError>;
}
/// Filesystem and Git adapter. Every path is derived from the configured root and validated node IDs.
/// Adapter-owned mutations traverse parents without following links/reparse points and retain
/// protection through filesystem work, its subprocess completion and cleanup, including parent
/// substitution between checking and use. Publication and moves never replace a destination.
/// Recycling validates the pinned entry identity and never falls back to permanent deletion.
/// Containment refusal is UnsafeSource; ordinary I/O failure is Filesystem. Recovery preserves
/// uncertain copies and leaves their operation unresolved.
///
/// `path()` returns an informational location and grants no filesystem authority. Execution
/// consumers must obtain a `NodeLocationGuard` from `pin_location` and retain it through
/// subprocess teardown and cleanup. Windows directory pins prevent rename or replacement of the
/// node directory and its parents through delete-sharing restrictions. Linux pins retain
/// directory identity and the guard's directory is descriptor-anchored; neither platform freezes
/// pathnames or contents inside the node. The guarded requirements and install script are
/// therefore snapshots read once through non-following handles and bound to the reviewed
/// digests, and consumers must use those snapshots without reopening the original paths. These
/// guarantees cover adapter-selected locations and inputs, not arbitrary filesystem access by
/// third-party code. Unsafe or uncertain locations are refused and recovery copies are preserved.
#[async_trait]
pub trait NodeFiles: Send + Sync {
    async fn git_status(
        &self,
        root: &Path,
        node: &InstalledNode,
        retained: Retained,
    ) -> Result<NodeGitStatus, NodeAppError>;
    /// On failure, clean only entries acquired by this attempt. Callers must not discard
    /// staging blindly: a refused create-new may have encountered a foreign entry.
    async fn prepare(
        &self,
        root: &Path,
        operation: &NodeId,
        request: &PrepareRequest,
        prior: Option<&InstalledNode>,
        policy: NodePreparePolicy,
        ctx: &StepContext,
    ) -> Result<StagedNode, NodeAppError>;
    async fn snapshot(
        &self,
        root: &Path,
        node: &InstalledNode,
        staged: Option<&NodeId>,
        retained: Retained,
    ) -> Result<TreeSnapshot, NodeAppError>;
    /// Returns a derived, informational path only. It is never authority for execution.
    fn path(&self, root: &Path, node: &InstalledNode, staged: Option<&NodeId>) -> PathBuf;
    /// Pins the node directory (live, or staged for `staged`) with its full parent chain and
    /// reads its root requirements and install script through non-following handles. Acquire a
    /// live guard after publication, because Windows pins obstruct renaming the directory.
    /// Only absent requirements produce `None`; oversized or non-UTF-8 input is `UnsafeSource`.
    async fn pin_location(
        &self,
        root: &Path,
        node: &InstalledNode,
        staged: Option<&NodeId>,
    ) -> Result<Box<dyn NodeLocationGuard>, NodeAppError>;
    async fn publish(
        &self,
        root: &Path,
        operation: &NodeOperation,
        retained: Retained,
    ) -> Result<(), NodeAppError>;
    async fn backup(&self, root: &Path, operation: &NodeOperation) -> Result<(), NodeAppError>;
    async fn move_enabled(
        &self,
        root: &Path,
        prior: &InstalledNode,
        enabled: bool,
    ) -> Result<(), NodeAppError>;
    async fn recycle(&self, root: &Path, operation: &NodeOperation) -> Result<(), NodeAppError>;
    async fn discard_staging(&self, root: &Path, operation: &NodeId) -> Result<(), NodeAppError>;
    /// Reconciles identities conservatively. Uncertain copies must remain and block start.
    async fn recover(
        &self,
        root: &Path,
        operation: &NodeOperation,
        retained: Retained,
    ) -> Result<RecoveryAction, NodeAppError>;
    async fn scan(
        &self,
        root: &Path,
        retained: Retained,
    ) -> Result<Vec<InstalledNode>, NodeAppError>;
    /// Where the system file manager should open: `ComfyUI/custom_nodes` for `None`, a node
    /// folder (live or disabled) is opened, and a single-file node is revealed. The location is
    /// validated through non-following handles, nothing is created, and a missing target is
    /// `NotFound`.
    async fn explorer_target(
        &self,
        root: &Path,
        node: Option<&InstalledNode>,
    ) -> Result<ExplorerTarget, NodeAppError>;
}
/// A node location whose containment protection lives exactly as long as this value. Hold it
/// across every use of the paths and snapshots it hands out: previews, installation, the hook and
/// their awaited cleanup.
pub trait NodeLocationGuard: Send + Sync {
    /// The working directory for execution, anchored to the pinned directory.
    fn directory(&self) -> &Path;
    /// Root `requirements.txt` as read once from the guarded handle. `None` means absent;
    /// unsupported input prevents guard acquisition with an error. Empty UTF-8 is valid.
    fn requirements(&self) -> Option<Arc<[u8]>>;
    /// Root `install.py` as read once from the guarded handle.
    fn install_script(&self) -> Option<Arc<[u8]>>;
    /// Digests of the guarded tree and inputs, comparable to the reviewed plan.
    fn snapshot(&self) -> &TreeSnapshot;
    /// Handle that keeps the protection alive for processes which outlive a failed cleanup.
    fn retained(&self) -> Retained;
}

#[async_trait]
pub trait NodeHook: Send + Sync {
    /// The interpreter is the active absolute interpreter; only the guarded root install.py may
    /// run, with the guarded directory as its working directory.
    async fn run(
        &self,
        root: &Path,
        env: &PythonEnvironment,
        location: &dyn NodeLocationGuard,
        ctx: &StepContext,
    ) -> Result<(), NodeAppError>;
}
