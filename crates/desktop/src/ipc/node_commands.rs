//! Commands of the custom-node context. Each one is a thin controller over `NodeService`.
//! Installs and updates are two-step: a prepare command returns a reviewable plan with an opaque
//! ID, and a commit command consumes it with the user's consent. The UI never supplies resolved
//! metadata, paths or transfer URLs. Closing a window or dropping a subscription never cancels
//! backend work; only `node_cancel` does.

use application::custom_nodes::{
    CommitDecision, NodeAppError, OperationPhase, PrepareRequest, RegistryQuery,
};
use domain::custom_nodes::InstallConsent;
use infrastructure::custom_nodes::provenance::validate_remote;
use tauri::{State, ipc::Channel};

use super::{
    error::IpcError,
    node_dto::{
        BatchItemDto, BatchOutcomeDto, CommitInput, InstalledNodeDto, NodeEventDto,
        NodeGitStatusDto, NodeIdInput, NodeListDto, NodeOutcomeDto, OperationDto, PlanReviewDto,
        PrepareGitInput, PrepareRegistryInput, PrepareRetryInput, PrepareUpdateInput,
        RegistryPageDto, RegistrySearchInput, SetEnabledInput, UpdateAllInput, node_id,
        request_key,
    },
};
use crate::{
    app_lifecycle::open_explorer,
    state::{AppState, NodeServices},
};

fn trimmed(value: &str, max: usize) -> Result<&str, IpcError> {
    let value = value.trim();
    if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(IpcError::new("validation", "The request is not valid."));
    }
    Ok(value)
}

fn optional(value: &Option<String>, max: usize) -> Result<Option<String>, IpcError> {
    match value.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        None => Ok(None),
        Some(text) => trimmed(text, max).map(|t| Some(t.to_owned())),
    }
}

pub(super) fn decision(input: CommitInput) -> Result<(String, CommitDecision), IpcError> {
    let protected_changes = input
        .confirmed_changes
        .into_iter()
        .map(|change| change.into_domain())
        .collect::<Result<Vec<_>, _>>()?;
    Ok((
        input.plan_id.clone(),
        CommitDecision {
            consent: InstallConsent {
                plan_id: input.plan_id,
                binding: input.binding,
                execute_node_code: input.execute_node_code,
                run_install_py: input.run_install_py,
                // Wheel-only previews never authorize a later source build.
                allow_source_builds: false,
                acknowledge_warnings: input.acknowledge_warnings,
                remember_source: input.remember_source,
                remember_install_py: input.remember_install_py,
            },
            protected_changes,
        },
    ))
}

async fn list(nodes: &NodeServices) -> Result<NodeListDto, IpcError> {
    let installed = nodes.service.list().await?;
    let unresolved = nodes
        .service
        .unresolved()
        .await?
        .iter()
        // A prepared plan awaiting review is not something the user must resolve.
        .filter(|op| op.phase != OperationPhase::Prepared)
        .map(|op| OperationDto::new(op, nodes.stack.hub.tail(op.id.as_str())))
        .collect();
    Ok(NodeListDto {
        nodes: installed.iter().map(InstalledNodeDto::from).collect(),
        unresolved,
        pending_reviews: nodes
            .service
            .pending_reviews()?
            .iter()
            .map(PlanReviewDto::from)
            .collect(),
        recent: nodes
            .service
            .recent_operations()
            .await?
            .iter()
            .map(|op| OperationDto::new(op, nodes.stack.hub.tail(op.id.as_str())))
            .collect(),
        repair_required: nodes.service.repair_required().await?,
    })
}

fn repository_url(repository: &str) -> Result<tauri::Url, IpcError> {
    let invalid = || IpcError::new("validation", "The repository URL is not valid.");
    // The URL parser drops empty userinfo. Reject its raw delimiter as well as the
    // nonempty credentials rejected by the shared repository validator.
    let authority = repository
        .trim()
        .split_once("://")
        .map(|(_, rest)| rest.split(['/', '\\', '?', '#']).next().unwrap_or_default());
    if authority.is_none_or(|authority| authority.is_empty() || authority.contains('@')) {
        return Err(invalid());
    }
    let remote = validate_remote(repository, false).ok_or_else(invalid)?;
    remote.identity.parse().map_err(|_| invalid())
}

/// Opens a validated repository in the system browser. Manager links use this command
/// because automatic webview link opening is disabled.
#[tauri::command]
#[specta::specta]
pub async fn node_open_repository(
    state: State<'_, AppState>,
    repository: String,
) -> Result<(), IpcError> {
    let url = repository_url(&repository)?;
    state.shell()?.open_browser(url);
    Ok(())
}

/// Shows the node in the system file manager, or the custom nodes folder when no id is given.
#[tauri::command]
#[specta::specta]
pub async fn node_reveal(state: State<'_, AppState>, id: Option<String>) -> Result<(), IpcError> {
    let id = id.as_deref().map(node_id).transpose()?;
    let target = state.services()?.nodes.locate.execute(id.as_ref()).await?;
    open_explorer(&state, target).await
}

#[tauri::command]
#[specta::specta]
pub async fn node_registry_search(
    state: State<'_, AppState>,
    input: RegistrySearchInput,
) -> Result<RegistryPageDto, IpcError> {
    let services = state.services()?;
    let hours = services
        .get_settings
        .execute()
        .await?
        .registry_cache_ttl
        .get();
    let page = services
        .nodes
        .service
        .search(
            RegistryQuery {
                search: input.search,
                page: input.page,
                limit: input.limit,
            },
            input.refresh,
            u64::from(hours) * 3600,
        )
        .await?;
    Ok(RegistryPageDto::from(&page))
}

#[tauri::command]
#[specta::specta]
pub async fn node_list(state: State<'_, AppState>) -> Result<NodeListDto, IpcError> {
    list(&state.services()?.nodes).await
}

#[tauri::command]
#[specta::specta]
pub async fn node_git_status(
    state: State<'_, AppState>,
    input: NodeIdInput,
) -> Result<NodeGitStatusDto, IpcError> {
    let status = state
        .services()?
        .nodes
        .service
        .git_status(&node_id(&input.node_id)?)
        .await?;
    Ok(NodeGitStatusDto::from(&status))
}

/// Finishes interrupted operations, then reads the folders again. Also runs after every verified
/// ComfyUI stop, because Manager or the user may have changed the folders meanwhile.
#[tauri::command]
#[specta::specta]
pub async fn node_rescan(state: State<'_, AppState>) -> Result<NodeListDto, IpcError> {
    let nodes = &state.services()?.nodes;
    let result = nodes.service.rescan().await;
    nodes.changed();
    result?;
    list(nodes).await
}

#[tauri::command]
#[specta::specta]
pub async fn node_prepare_registry(
    state: State<'_, AppState>,
    input: PrepareRegistryInput,
) -> Result<PlanReviewDto, IpcError> {
    let nodes = &state.services()?.nodes;
    let key = request_key(&input.request_id)?;
    let request = PrepareRequest::Registry {
        id: trimmed(&input.id, 100)?.to_owned(),
        version: optional(&input.version, 64)?,
    };
    nodes.prepare(&key, request).await
}

#[tauri::command]
#[specta::specta]
pub async fn node_prepare_git(
    state: State<'_, AppState>,
    input: PrepareGitInput,
) -> Result<PlanReviewDto, IpcError> {
    let nodes = &state.services()?.nodes;
    let key = request_key(&input.request_id)?;
    let request = PrepareRequest::Git {
        repository: trimmed(&input.repository, 500)?.to_owned(),
        reference: optional(&input.reference, 200)?,
    };
    nodes.prepare(&key, request).await
}

#[tauri::command]
#[specta::specta]
pub async fn node_prepare_update(
    state: State<'_, AppState>,
    input: PrepareUpdateInput,
) -> Result<PlanReviewDto, IpcError> {
    let nodes = &state.services()?.nodes;
    let key = request_key(&input.request_id)?;
    let request = PrepareRequest::Update {
        node: node_id(&input.node_id)?,
        reference: optional(&input.reference, 200)?,
    };
    nodes.prepare(&key, request).await
}

#[tauri::command]
#[specta::specta]
pub async fn node_prepare_retry_deps(
    state: State<'_, AppState>,
    input: PrepareRetryInput,
) -> Result<PlanReviewDto, IpcError> {
    let nodes = &state.services()?.nodes;
    let key = request_key(&input.request_id)?;
    let request = PrepareRequest::RetryDependencies {
        node: node_id(&input.node_id)?,
    };
    nodes.prepare(&key, request).await
}

/// Executes a reviewed plan with the user's consent. A stale plan is rejected, never re-resolved.
#[tauri::command]
#[specta::specta]
pub async fn node_commit(
    state: State<'_, AppState>,
    input: CommitInput,
) -> Result<NodeOutcomeDto, IpcError> {
    let nodes = &state.services()?.nodes;
    let (plan_id, decision) = decision(input)?;
    let key = request_key(&plan_id)?;
    let guard = nodes.begin(&key)?;
    let ctx = nodes.context(&guard);
    let result = nodes.service.commit(&plan_id, decision, &ctx).await;
    nodes.changed();
    Ok(NodeOutcomeDto::from(&result?))
}

#[tauri::command]
#[specta::specta]
pub async fn node_set_enabled(
    state: State<'_, AppState>,
    input: SetEnabledInput,
) -> Result<NodeOutcomeDto, IpcError> {
    let nodes = &state.services()?.nodes;
    let request = PrepareRequest::SetEnabled {
        node: node_id(&input.node_id)?,
        enabled: input.enabled,
    };
    nodes.direct(request).await
}

#[tauri::command]
#[specta::specta]
pub async fn node_uninstall(
    state: State<'_, AppState>,
    input: NodeIdInput,
) -> Result<NodeOutcomeDto, IpcError> {
    let nodes = &state.services()?.nodes;
    nodes
        .direct(PrepareRequest::Uninstall {
            node: node_id(&input.node_id)?,
        })
        .await
}

/// Runs reviewed update plans one after another under one maintenance lease, reporting every
/// node's outcome. The batch stops when a dependency failure damages the environment.
#[tauri::command]
#[specta::specta]
pub async fn node_update_all(
    state: State<'_, AppState>,
    input: UpdateAllInput,
) -> Result<BatchOutcomeDto, IpcError> {
    let nodes = &state.services()?.nodes;
    if input.items.is_empty() || input.items.len() > 128 {
        return Err(IpcError::new(
            "validation",
            "Select between 1 and 128 updates.",
        ));
    }
    let decisions = input
        .items
        .into_iter()
        .map(decision)
        .collect::<Result<Vec<_>, _>>()?;
    let guard = nodes.begin("update-all")?;
    let ctx = nodes.context(&guard);
    let result = nodes.service.update_all(decisions, &ctx).await;
    nodes.changed();
    let batch = result?;
    Ok(BatchOutcomeDto {
        items: batch
            .results
            .iter()
            .map(|(plan_id, result)| match result {
                Ok(outcome) => BatchItemDto {
                    plan_id: plan_id.clone(),
                    outcome: Some(NodeOutcomeDto::from(outcome)),
                    error: None,
                },
                Err(error) => BatchItemDto {
                    plan_id: plan_id.clone(),
                    outcome: None,
                    error: Some(IpcError::from(error.clone())),
                },
            })
            .collect(),
        stopped_for_repair: batch.stopped_for_repair,
    })
}

/// Cancels a running preparation or commit by its key, or discards a reviewed plan by its ID.
/// Cancelling after dependency mutation began leaves the environment needing repair.
#[tauri::command]
#[specta::specta]
pub async fn node_cancel(state: State<'_, AppState>, key: String) -> Result<(), IpcError> {
    let nodes = &state.services()?.nodes;
    let key = request_key(&key)?;
    if nodes.cancel(&key) {
        return Ok(());
    }
    match nodes.service.discard(&key).await {
        Ok(()) | Err(NodeAppError::StalePlan) => {
            nodes.changed();
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn node_operation_status(
    state: State<'_, AppState>,
    operation_id: String,
) -> Result<OperationDto, IpcError> {
    let nodes = &state.services()?.nodes;
    let operation = nodes.service.operation_status(&operation_id).await?;
    Ok(OperationDto::new(
        &operation,
        nodes.stack.hub.tail(operation.id.as_str()),
    ))
}

/// Registers a per-subscription channel: an initial snapshot marker with the current sequence,
/// then ordered events. A slow subscriber receives `gap` and must refetch the list.
#[tauri::command]
#[specta::specta]
pub async fn node_subscribe(
    state: State<'_, AppState>,
    on_event: Channel<NodeEventDto>,
) -> Result<u32, IpcError> {
    use crate::state::subscription_gap;
    use infrastructure::custom_nodes::stack::NodeEvent;
    let nodes = &state.services()?.nodes;
    let hub = nodes.stack.hub.clone();
    let mut receiver = hub.subscribe();
    let id = nodes.next_subscription();
    let epoch = hub.epoch.clone();
    let handle = tauri::async_runtime::spawn(async move {
        let first = NodeEventDto::Snapshot {
            epoch: epoch.clone(),
            sequence: hub.sequence() as f64,
        };
        if on_event.send(first).is_err() {
            return;
        }
        loop {
            let event = match receiver.recv().await {
                Ok((sequence, NodeEvent::Changed)) => NodeEventDto::Changed {
                    epoch: epoch.clone(),
                    sequence: sequence as f64,
                },
                Ok((sequence, NodeEvent::Log { operation, line })) => NodeEventDto::Log {
                    epoch: epoch.clone(),
                    sequence: sequence as f64,
                    operation_id: operation,
                    line,
                },
                Err(error) if subscription_gap(&error) => NodeEventDto::Gap {
                    epoch: epoch.clone(),
                    sequence: hub.sequence() as f64,
                },
                Err(_) => return,
            };
            if on_event.send(event).is_err() {
                return;
            }
        }
    });
    if let Some(previous) = nodes
        .subscriptions
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(id, handle)
    {
        previous.abort();
    }
    Ok(id)
}

#[tauri::command]
#[specta::specta]
pub async fn node_unsubscribe(
    state: State<'_, AppState>,
    subscription_id: u32,
) -> Result<(), IpcError> {
    let nodes = &state.services()?.nodes;
    let handle = nodes
        .subscriptions
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(&subscription_id);
    if let Some(handle) = handle {
        handle.abort();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_browser_urls_use_normalized_https_identities() {
        assert_eq!(
            repository_url(" https://GitHub.com/example/node.git/ ")
                .unwrap()
                .as_str(),
            "https://github.com/example/node"
        );
    }

    #[test]
    fn unsafe_repository_browser_urls_return_a_sanitized_error() {
        for repository in [
            "",
            "not a URL",
            "http://github.com/example/node",
            "file:///C:/Windows/win.ini",
            "javascript:alert(1)",
            "git@github.com:example/node.git",
            "https://user:secret@github.com/example/node",
            "https://user@github.com/example/node",
            "https://@github.com/example/node",
            "https://github.com/example/node?token=secret",
            "https://github.com/example/node#secret",
            "https://github.com:444/example/node",
            "https://github.com/",
            "https://github.com/example/\nnode",
        ] {
            assert_eq!(
                repository_url(repository).unwrap_err(),
                IpcError::new("validation", "The repository URL is not valid.")
            );
        }
    }

    #[test]
    fn repository_browser_urls_reject_empty_raw_authority() {
        for repository in [
            "https:////@github.com/example/node",
            "https:///github.com/x/y",
            "https:////github.com/x/y",
            r"https://\github.com/x/y",
            r"https:\\github.com/x/y",
        ] {
            assert_eq!(
                repository_url(repository).unwrap_err(),
                IpcError::new("validation", "The repository URL is not valid.")
            );
        }
    }
}
