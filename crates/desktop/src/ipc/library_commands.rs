//! Commands of the model library context. Each one is a thin controller over an application use
//! case. Scans and hashing run on the Rust core, never on the UI; closing the window or dropping
//! a subscription never cancels an operation.

use domain::shared::model_values::RelativeModelPath;
use tauri::{State, ipc::Channel};

use super::{
    error::IpcError,
    library_dto::{
        DeletePlanDto, DeleteReportDto, IdentifyInput, IdentifyOutcomeDto, LibrarySnapshotDto,
        ModelDetailsDto, ModelEventDto, ModelLocationDto, MoveInput, OperationStatusDto,
        RenameInput,
    },
};
use crate::{app_lifecycle::open_explorer, state::AppState};

#[tauri::command]
#[specta::specta]
pub async fn model_list(state: State<'_, AppState>) -> Result<LibrarySnapshotDto, IpcError> {
    let snapshot = state.services()?.library.list.execute().await?;
    Ok(LibrarySnapshotDto::from(&snapshot))
}

/// Finishes interrupted operations, then scans every root and waits for the result.
#[tauri::command]
#[specta::specta]
pub async fn model_rescan(state: State<'_, AppState>) -> Result<LibrarySnapshotDto, IpcError> {
    let snapshot = state.services()?.library.rescan.execute().await?;
    Ok(LibrarySnapshotDto::from(&snapshot))
}

#[tauri::command]
#[specta::specta]
pub async fn model_details(
    state: State<'_, AppState>,
    location: ModelLocationDto,
) -> Result<ModelDetailsDto, IpcError> {
    let details = state
        .services()?
        .library
        .details
        .execute(&location.into_domain()?)
        .await?;
    Ok(ModelDetailsDto::from(&details))
}

#[tauri::command]
#[specta::specta]
pub async fn model_rename(
    state: State<'_, AppState>,
    input: RenameInput,
) -> Result<ModelLocationDto, IpcError> {
    let placed = state
        .services()?
        .library
        .rename
        .execute(
            &input.location.into_domain()?,
            &input.new_name,
            input.allow_extension_change,
        )
        .await?;
    Ok(ModelLocationDto::from(&placed.location))
}

#[tauri::command]
#[specta::specta]
pub async fn model_move(
    state: State<'_, AppState>,
    input: MoveInput,
) -> Result<ModelLocationDto, IpcError> {
    let directory = RelativeModelPath::new(input.target_directory.trim().trim_matches('/'))
        .map_err(|_| IpcError::new("validation", "The target folder is not valid."))?;
    let placed = state
        .services()?
        .library
        .move_model
        .execute(
            &input.location.into_domain()?,
            &input.target_root_id,
            directory,
        )
        .await?;
    Ok(ModelLocationDto::from(&placed.location))
}

/// Builds the confirmation (files and total size) a delete must be bound to.
#[tauri::command]
#[specta::specta]
pub async fn model_delete_plan(
    state: State<'_, AppState>,
    locations: Vec<ModelLocationDto>,
) -> Result<DeletePlanDto, IpcError> {
    if locations.is_empty() || locations.len() > 10_000 {
        return Err(IpcError::new(
            "validation",
            "Select between 1 and 10,000 files.",
        ));
    }
    let locations = locations
        .into_iter()
        .map(ModelLocationDto::into_domain)
        .collect::<Result<Vec<_>, _>>()?;
    let plan = state
        .services()?
        .library
        .plan_delete
        .execute(locations)
        .await?;
    Ok(DeletePlanDto::from(&plan))
}

/// Moves the planned files to the Recycle Bin or Trash, reporting every file separately.
#[tauri::command]
#[specta::specta]
pub async fn model_delete(
    state: State<'_, AppState>,
    plan_id: String,
) -> Result<DeleteReportDto, IpcError> {
    let report = state.services()?.library.delete.execute(&plan_id).await?;
    Ok(DeleteReportDto::from(&report))
}

/// Runs only a permanent-delete plan produced after a trash failure and confirmed separately.
#[tauri::command]
#[specta::specta]
pub async fn model_delete_permanently(
    state: State<'_, AppState>,
    plan_id: String,
) -> Result<DeleteReportDto, IpcError> {
    let report = state
        .services()?
        .library
        .delete_permanently
        .execute(&plan_id)
        .await?;
    Ok(DeleteReportDto::from(&report))
}

/// Shows the model in the system file manager, or the models folder when no location is given.
#[tauri::command]
#[specta::specta]
pub async fn model_reveal(
    state: State<'_, AppState>,
    location: Option<ModelLocationDto>,
) -> Result<(), IpcError> {
    let location = location.map(ModelLocationDto::into_domain).transpose()?;
    let target = state
        .services()?
        .library
        .locate
        .execute(location.as_ref())
        .await?;
    open_explorer(&state, target).await
}

#[tauri::command]
#[specta::specta]
pub async fn model_identify(
    state: State<'_, AppState>,
    input: IdentifyInput,
) -> Result<IdentifyOutcomeDto, IpcError> {
    let outcome = state
        .services()?
        .library
        .identify
        .execute(&input.location.into_domain()?, input.replace_existing)
        .await?;
    Ok(IdentifyOutcomeDto::from(&outcome))
}

/// Interrupted rename or move operations that still await recovery.
#[tauri::command]
#[specta::specta]
pub async fn model_operation_status(
    state: State<'_, AppState>,
) -> Result<Vec<OperationStatusDto>, IpcError> {
    let records = state.services()?.library.operations.execute().await?;
    Ok(records.iter().map(OperationStatusDto::from).collect())
}

/// Registers a per-subscription channel. The first event is the current status; every later
/// event is a coalesced change notice that tells the UI to refetch the list.
#[tauri::command]
#[specta::specta]
pub async fn model_subscribe(
    state: State<'_, AppState>,
    on_event: Channel<ModelEventDto>,
) -> Result<u32, IpcError> {
    let library = &state.services()?.library;
    let supervisor = library.stack.supervisor.clone();
    let status = library.status.clone();
    let epoch = library.epoch.clone();
    let id = library.next_subscription();
    let mut changes = supervisor.subscribe();
    let handle = tauri::async_runtime::spawn(async move {
        changes.borrow_and_update();
        let first = ModelEventDto::Snapshot {
            epoch: epoch.clone(),
            status: (&status.execute()).into(),
        };
        if on_event.send(first).is_err() {
            return;
        }
        while changes.changed().await.is_ok() {
            let event = ModelEventDto::Changed {
                epoch: epoch.clone(),
                status: (&status.execute()).into(),
            };
            if on_event.send(event).is_err() {
                return;
            }
        }
    });
    if let Some(previous) = library
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
pub async fn model_unsubscribe(
    state: State<'_, AppState>,
    subscription_id: u32,
) -> Result<(), IpcError> {
    let library = &state.services()?.library;
    let handle = library
        .subscriptions
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(&subscription_id);
    if let Some(handle) = handle {
        handle.abort();
    }
    Ok(())
}
