use super::*;
use crate::environment::StepContext;
use async_trait::async_trait;
use domain::{custom_nodes::*, environment::PythonEnvironment};
use std::path::{Path, PathBuf};

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
    async fn operations(&self) -> Result<Vec<NodeOperation>, NodeAppError>;
    async fn save_dependency(&self, outcome: &DependencyOutcome) -> Result<(), NodeAppError>;
    async fn trust(&self, namespace: &str) -> Result<Option<Trust>, NodeAppError>;
    async fn save_trust(&self, trust: &Trust) -> Result<(), NodeAppError>;
}
/// Filesystem and Git adapter. Every path is derived from the configured root and validated node IDs.
/// Mutations use shared pinned parents/no-replace primitives, never unchecked rename paths.
#[async_trait]
pub trait NodeFiles: Send + Sync {
    async fn git_status(
        &self,
        root: &Path,
        node: &InstalledNode,
    ) -> Result<NodeGitStatus, NodeAppError>;
    async fn prepare(
        &self,
        root: &Path,
        operation: &NodeId,
        request: &PrepareRequest,
        prior: Option<&InstalledNode>,
        ctx: &StepContext,
    ) -> Result<StagedNode, NodeAppError>;
    async fn snapshot(
        &self,
        root: &Path,
        node: &InstalledNode,
        staged: Option<&NodeId>,
    ) -> Result<TreeSnapshot, NodeAppError>;
    fn path(&self, root: &Path, node: &InstalledNode, staged: Option<&NodeId>) -> PathBuf;
    async fn publish(&self, root: &Path, operation: &NodeOperation) -> Result<(), NodeAppError>;
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
    ) -> Result<RecoveryAction, NodeAppError>;
    async fn scan(&self, root: &Path) -> Result<Vec<InstalledNode>, NodeAppError>;
}
#[async_trait]
pub trait NodeHook: Send + Sync {
    /// The interpreter is the active absolute interpreter; only root install.py may run.
    async fn run(
        &self,
        root: &Path,
        env: &PythonEnvironment,
        node_directory: &Path,
        ctx: &StepContext,
    ) -> Result<(), NodeAppError>;
}
