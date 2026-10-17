//! Commands of the ComfyUI runtime context. Each one is a thin controller over an application
//! use case; the runtime itself is owned by the Rust core, never by a window.

use application::{
    comfy_runtime::{RuntimeError, SessionId, UpdateRequest, choice_action},
    shared::CancelToken,
};
use tauri::{State, Url, ipc::Channel};

use super::{
    comfy_dto::{
        CloseChoiceDto, ComfyEventDto, ComfyStatusDto, DiagnosticsDto, LaunchOptionsDto,
        ModelRootDto, StartResultDto, UpdateInput, UpdateResultDto, VersionListDto,
    },
    error::IpcError,
};
use crate::state::AppState;

fn invalid_identifier() -> IpcError {
    IpcError::new("invalid_identifier", "The identifier is not valid.")
}

fn start_result(
    result: Result<application::comfy_runtime::RuntimeStatus, RuntimeError>,
    pending: Option<String>,
) -> Result<StartResultDto, IpcError> {
    match result {
        Ok(status) => Ok(StartResultDto::Started {
            status: ComfyStatusDto::new(&status, pending),
        }),
        Err(RuntimeError::PortConflict {
            requested,
            suggested,
        }) => Ok(StartResultDto::PortConflict {
            requested: requested.get(),
            suggested: suggested.map(|port| port.get()),
        }),
        Err(error) => Err(error.into()),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn comfy_status(state: State<'_, AppState>) -> Result<ComfyStatusDto, IpcError> {
    let status = state.services()?.comfy.status.execute().await?;
    Ok(ComfyStatusDto::new(&status, state.close.pending()))
}

#[tauri::command]
#[specta::specta]
pub async fn comfy_start(state: State<'_, AppState>) -> Result<StartResultDto, IpcError> {
    let services = state.services()?;
    let result = services.comfy.start.execute().await;
    start_result(result, state.close.pending())
}

#[tauri::command]
#[specta::specta]
pub async fn comfy_stop(state: State<'_, AppState>) -> Result<ComfyStatusDto, IpcError> {
    let services = state.services()?;
    let status = services.comfy.stop.execute().await?;
    Ok(ComfyStatusDto::new(&status, state.close.pending()))
}

#[tauri::command]
#[specta::specta]
pub async fn comfy_restart(state: State<'_, AppState>) -> Result<StartResultDto, IpcError> {
    let services = state.services()?;
    let result = services.comfy.restart.execute().await;
    start_result(result, state.close.pending())
}

/// Registers a per-subscription channel. The first event is an authoritative snapshot with a
/// replay of the retained log; closing the channel or reloading the UI never stops ComfyUI.
#[tauri::command]
#[specta::specta]
pub async fn comfy_subscribe(
    state: State<'_, AppState>,
    after_cursor: Option<f64>,
    on_event: Channel<ComfyEventDto>,
) -> Result<u32, IpcError> {
    let comfy = &state.services()?.comfy;
    let after = after_cursor
        .filter(|cursor| cursor.is_finite() && *cursor >= 0.0)
        .map(|cursor| cursor as u64);
    let mut subscription = comfy.events.subscribe(after);
    let id = u32::try_from(subscription.id).map_err(|_| {
        IpcError::new(
            "subscription_exhausted",
            "Too many subscriptions were opened.",
        )
    })?;
    let close = state.close.clone();
    let mut close_changes = close.watch();
    let handle = tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                event = subscription.receiver.next() => {
                    let Some(event) = event else { break };
                    let dto = ComfyEventDto::from_event(&event, close.pending());
                    if on_event.send(dto).is_err() {
                        break;
                    }
                }
                changed = close_changes.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    let request_id = close_changes.borrow_and_update().clone();
                    if on_event.send(ComfyEventDto::CloseRequested { request_id }).is_err() {
                        break;
                    }
                }
            }
        }
    });
    if let Some(previous) = comfy
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
pub async fn comfy_unsubscribe(
    state: State<'_, AppState>,
    subscription_id: u32,
) -> Result<(), IpcError> {
    let handle = state
        .services()?
        .comfy
        .subscriptions
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(&subscription_id);
    if let Some(handle) = handle {
        handle.abort();
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn comfy_get_launch_options(
    state: State<'_, AppState>,
) -> Result<LaunchOptionsDto, IpcError> {
    let options = state.services()?.comfy.get_options.execute().await?;
    Ok(LaunchOptionsDto::from(&options))
}

#[tauri::command]
#[specta::specta]
pub async fn comfy_set_launch_options(
    state: State<'_, AppState>,
    options: LaunchOptionsDto,
) -> Result<LaunchOptionsDto, IpcError> {
    let options = options.into_domain()?;
    let comfy = &state.services()?.comfy;
    comfy.set_options.execute(options).await?;
    let saved = comfy.get_options.execute().await?;
    Ok(LaunchOptionsDto::from(&saved))
}

#[tauri::command]
#[specta::specta]
pub async fn comfy_list_versions(state: State<'_, AppState>) -> Result<VersionListDto, IpcError> {
    let comfy = &state.services()?.comfy;
    let versions = comfy.versions.execute().await?;
    let pending = comfy.store.unfinished_update().await?;
    Ok(VersionListDto::new(&versions, pending.as_ref()))
}

#[tauri::command]
#[specta::specta]
pub async fn comfy_update(
    state: State<'_, AppState>,
    input: UpdateInput,
) -> Result<UpdateResultDto, IpcError> {
    let UpdateInput {
        target,
        stop_running,
        retry,
    } = input;
    let request = UpdateRequest {
        target: target.into_domain()?,
        stop_running,
        retry: retry
            .map(SessionId::new)
            .transpose()
            .map_err(|_| invalid_identifier())?,
    };
    match state
        .services()?
        .comfy
        .update
        .execute(request, CancelToken::new())
        .await
    {
        Ok(run) => Ok(UpdateResultDto::Updated { run: (&run).into() }),
        Err(RuntimeError::NeedsStop) => Ok(UpdateResultDto::NeedsStop),
        Err(RuntimeError::UpdateRefused { reason, status }) => Ok(UpdateResultDto::Refused {
            reason: reason.to_string(),
            status,
        }),
        Err(error) => Err(error.into()),
    }
}

fn running_origin(state: &AppState) -> Result<Url, IpcError> {
    let origin = state
        .services()?
        .comfy
        .hub
        .status()
        .origin()
        .ok_or_else(|| IpcError::new("comfyui_not_running", "ComfyUI is not running."))?;
    origin
        .parse()
        .map_err(|_| IpcError::new("comfyui_not_running", "ComfyUI is not running."))
}

/// Opens the one ComfyUI window. The URL comes from the validated runtime state only.
#[tauri::command]
#[specta::specta]
pub async fn comfy_open_window(state: State<'_, AppState>) -> Result<(), IpcError> {
    let url = running_origin(&state)?;
    state.shell()?.open_comfy_window(url)?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn comfy_open_browser(state: State<'_, AppState>) -> Result<(), IpcError> {
    let url = running_origin(&state)?;
    state.shell()?.open_browser(url);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn comfy_diagnostics(state: State<'_, AppState>) -> Result<DiagnosticsDto, IpcError> {
    let diagnostics = state.services()?.comfy.diagnostics.execute().await?;
    Ok(DiagnosticsDto::new(&diagnostics, state.close.pending()))
}

#[tauri::command]
#[specta::specta]
pub async fn comfy_read_extra_model_paths(
    state: State<'_, AppState>,
) -> Result<Vec<ModelRootDto>, IpcError> {
    let roots = state.services()?.comfy.extra_paths.execute().await?;
    Ok(roots.iter().map(ModelRootDto::from).collect())
}

/// Answers a pending manager close request.
#[tauri::command]
#[specta::specta]
pub async fn app_close_decide(
    state: State<'_, AppState>,
    request_id: String,
    choice: CloseChoiceDto,
) -> Result<(), IpcError> {
    if !state.close.resolve(&request_id) {
        return Err(IpcError::new(
            "close_request_unknown",
            "That close request is no longer pending.",
        ));
    }
    if let Some(action) = choice_action(choice.into()) {
        state.shell()?.perform(action);
    }
    Ok(())
}
