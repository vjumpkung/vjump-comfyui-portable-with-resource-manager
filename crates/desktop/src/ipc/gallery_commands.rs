//! Commands of the gallery context. Each one is a thin controller over an application use case.
//! Entries are always named by an area plus a relative path and the revision the UI last saw;
//! the backend resolves them below ComfyUI's `input` and `output` folders and never takes an
//! absolute path. Deleting only ever goes to the Recycle Bin or Trash.

use application::gallery::{GalleryEntryRef, MAX_ITEMS, TransferMode};
use tauri::State;

use super::{
    error::IpcError,
    gallery_dto::{
        GalleryBatchReportDto, GalleryDeletePlanDto, GalleryEntryRefDto, GalleryListingDto,
        GalleryLocationDto, GalleryRenameInput, GalleryTransferInput,
    },
};
use crate::{app_lifecycle::open_explorer, state::AppState};

fn entries(items: Vec<GalleryEntryRefDto>) -> Result<Vec<GalleryEntryRef>, IpcError> {
    if items.is_empty() || items.len() > MAX_ITEMS {
        return Err(IpcError::new(
            "validation",
            &format!("Select between 1 and {MAX_ITEMS} items."),
        ));
    }
    items
        .into_iter()
        .map(GalleryEntryRefDto::into_domain)
        .collect()
}

async fn transfer(
    state: &AppState,
    input: GalleryTransferInput,
    mode: TransferMode,
) -> Result<GalleryBatchReportDto, IpcError> {
    let items = entries(input.items)?;
    let destination = input.destination.into_domain()?;
    let results = state
        .services()?
        .gallery
        .transfer
        .execute(items, destination, mode)
        .await?;
    Ok(GalleryBatchReportDto::from(results.as_slice()))
}

#[tauri::command]
#[specta::specta]
pub async fn gallery_list(
    state: State<'_, AppState>,
    directory: GalleryLocationDto,
) -> Result<GalleryListingDto, IpcError> {
    let listing = state
        .services()?
        .gallery
        .list
        .execute(&directory.into_domain()?)
        .await?;
    Ok(GalleryListingDto::from(&listing))
}

/// Renames inside the same folder; the answer is the new location.
#[tauri::command]
#[specta::specta]
pub async fn gallery_rename(
    state: State<'_, AppState>,
    input: GalleryRenameInput,
) -> Result<GalleryLocationDto, IpcError> {
    let placed = state
        .services()?
        .gallery
        .rename
        .execute(&input.entry.into_domain()?, &input.new_name)
        .await?;
    Ok(GalleryLocationDto::from(&placed))
}

/// Moves every entry into an existing folder, reporting each one separately.
#[tauri::command]
#[specta::specta]
pub async fn gallery_move(
    state: State<'_, AppState>,
    input: GalleryTransferInput,
) -> Result<GalleryBatchReportDto, IpcError> {
    transfer(&state, input, TransferMode::Move).await
}

/// Copies every entry into an existing folder, reporting each one separately.
#[tauri::command]
#[specta::specta]
pub async fn gallery_copy(
    state: State<'_, AppState>,
    input: GalleryTransferInput,
) -> Result<GalleryBatchReportDto, IpcError> {
    transfer(&state, input, TransferMode::Copy).await
}

/// Builds the confirmation (entries and total size) a delete must be bound to.
#[tauri::command]
#[specta::specta]
pub async fn gallery_delete_plan(
    state: State<'_, AppState>,
    items: Vec<GalleryEntryRefDto>,
) -> Result<GalleryDeletePlanDto, IpcError> {
    let items = entries(items)?;
    let plan = state.services()?.gallery.plan_delete.execute(items).await?;
    Ok(GalleryDeletePlanDto::from(&plan))
}

/// Moves the planned entries to the Recycle Bin or Trash, reporting every entry separately.
#[tauri::command]
#[specta::specta]
pub async fn gallery_delete(
    state: State<'_, AppState>,
    plan_id: String,
) -> Result<GalleryBatchReportDto, IpcError> {
    let results = state.services()?.gallery.delete.execute(&plan_id).await?;
    Ok(GalleryBatchReportDto::from(results.as_slice()))
}

/// Shows the entry in the system file manager: a folder opens, a file is selected in its folder.
#[tauri::command]
#[specta::specta]
pub async fn gallery_reveal(
    state: State<'_, AppState>,
    location: GalleryLocationDto,
) -> Result<(), IpcError> {
    let target = state
        .services()?
        .gallery
        .locate
        .execute(&location.into_domain()?)
        .await?;
    open_explorer(&state, target).await
}
