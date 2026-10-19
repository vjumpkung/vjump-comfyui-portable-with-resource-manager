use super::{
    DependencyOutcome, NodeAppError, NodeFiles, NodeOperation, NodeRepository, OperationKind,
};
use crate::environment::{
    GuardedRequirements, NodeRequirementSet, NodeRequirementSource, ProvisionError, Retained,
};
use async_trait::async_trait;
use domain::{custom_nodes::*, environment::EnvId};
use std::{path::Path, sync::Arc};

pub struct NodeRequirements {
    store: Arc<dyn NodeRepository>,
    files: Arc<dyn NodeFiles>,
}

impl NodeRequirements {
    pub fn new(store: Arc<dyn NodeRepository>, files: Arc<dyn NodeFiles>) -> Self {
        Self { store, files }
    }
}

pub(crate) fn merge_observed(node: &mut InstalledNode, stored: &[InstalledNode]) {
    if let Some(prior) = stored
        .iter()
        .find(|prior| prior.name == node.name && prior.single_file == node.single_file)
    {
        node.id = prior.id.clone();
        node.origin = prior.origin;
        if prior.source.same_origin(&node.source) {
            node.source = prior.source.with_observed_revision(&node.source);
        } else {
            node.conflict = true;
            node.source = prior.source.clone();
        }
    }
}

fn provider_error(_: NodeAppError) -> ProvisionError {
    ProvisionError::Failed("Custom node requirements could not be read or recorded.".into())
}

#[async_trait]
impl NodeRequirementSource for NodeRequirements {
    async fn requirement_sets(
        &self,
        root: &Path,
        retained: Retained,
    ) -> Result<Vec<NodeRequirementSet>, ProvisionError> {
        let operations = self.store.operations().await.map_err(provider_error)?;
        if operations.iter().any(NodeOperation::blocks_start) {
            return Err(provider_error(NodeAppError::RecoveryRequired));
        }
        let stored = self.store.nodes().await.map_err(provider_error)?;
        let observed = self
            .files
            .scan(root, retained.clone())
            .await
            .map_err(provider_error)?;
        let mut sets = Vec::new();
        for mut node in observed {
            merge_observed(&mut node, &stored);
            if node.conflict {
                return Err(provider_error(NodeAppError::RecoveryRequired));
            }
            if node.single_file {
                continue;
            }
            let guard = self
                .files
                .pin_location(root, &node, None)
                .await
                .map_err(provider_error)?;
            node.tree_digest = guard.snapshot().digest.clone();
            let file = guard.requirements().map(|text| GuardedRequirements {
                path: guard.directory().join("requirements.txt"),
                directory: guard.directory().to_path_buf(),
                text,
                retained: guard.retained().merge(retained.clone()),
            });
            let extra = if matches!(node.source, NodeSource::Registry { .. }) {
                operations
                    .iter()
                    .rev()
                    .find(|op| {
                        op.node.id == node.id
                            && op.node.source.identity() == node.source.identity()
                            && op.consent_identity.is_some()
                            && matches!(
                                op.kind,
                                OperationKind::Install
                                    | OperationKind::Update
                                    | OperationKind::RetryDependencies
                            )
                    })
                    .map(|op| op.dependency_requirements.clone())
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            if file.is_some() || !extra.is_empty() {
                sets.push(NodeRequirementSet {
                    node_id: node.id.clone(),
                    name: node.name.clone(),
                    source_identity: node.dependency_identity(),
                    file,
                    extra,
                });
            }
        }
        Ok(sets)
    }

    async fn record_outcome(
        &self,
        environment: &EnvId,
        set: &NodeRequirementSet,
        state: DependencyState,
    ) -> Result<(), ProvisionError> {
        self.store
            .save_dependency(&DependencyOutcome {
                node: set.node_id.clone(),
                environment: environment.clone(),
                source_identity: set.source_identity.clone(),
                state,
            })
            .await
            .map_err(provider_error)
    }

    async fn needs_repair(
        &self,
        environment: &EnvId,
        root: &Path,
        retained: Retained,
    ) -> Result<bool, ProvisionError> {
        for set in self.requirement_sets(root, retained).await? {
            if self
                .store
                .dependency(&set.node_id, environment, &set.source_identity)
                .await
                .map_err(provider_error)?
                != Some(DependencyState::Ok)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
