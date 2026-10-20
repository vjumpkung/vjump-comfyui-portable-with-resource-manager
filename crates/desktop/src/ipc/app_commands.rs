//! Application-level commands: signed self-update and diagnostics. Plugin access stays on the
//! Rust side; no capability grants the updater or clipboard plugin commands to a webview.

use std::sync::Arc;

use application::settings::updates::{UpdateProgress, UpdateProgressSink};
use tauri::{State, ipc::Channel};

use super::{
    app_dto::{CopyDiagnosticsDto, UpdateOfferDto, UpdateProgressDto, UpdateStatusDto},
    error::IpcError,
};
use crate::{
    diagnostics::{collect_report, copy_report},
    state::AppState,
};

/// Forwards progress to the UI. A channel the UI has lost drops events: the installation goes on.
struct ChannelSink(Channel<UpdateProgressDto>);

impl UpdateProgressSink for ChannelSink {
    fn report(&self, progress: UpdateProgress) {
        let _ = self.0.send(UpdateProgressDto::from(progress));
    }
}

#[tauri::command]
#[specta::specta]
pub async fn app_update_status(state: State<'_, AppState>) -> Result<UpdateStatusDto, IpcError> {
    Ok(UpdateStatusDto {
        configured: state.updater().configured(),
        current_version: env!("CARGO_PKG_VERSION").to_owned(),
    })
}

/// Looks for a newer signed release. The offer it returns replaces every earlier one.
#[tauri::command]
#[specta::specta]
pub async fn app_update_check(
    state: State<'_, AppState>,
) -> Result<Option<UpdateOfferDto>, IpcError> {
    let offer = state.updater().check().await?;
    Ok(offer.map(UpdateOfferDto::from))
}

/// Installs the release the user reviewed, named by the `update_id` of its offer. Success means
/// the application is restarting.
#[tauri::command]
#[specta::specta]
pub async fn app_update_install(
    state: State<'_, AppState>,
    update_id: String,
    on_event: Channel<UpdateProgressDto>,
) -> Result<(), IpcError> {
    let services = state.services()?;
    let updater = state.updater();
    services
        .install_update
        .execute(
            updater.as_ref(),
            &update_id,
            Arc::new(ChannelSink(on_event)),
        )
        .await?;
    Ok(())
}

/// Collects the redacted report, puts it on the clipboard and returns it, so the UI can
/// offer it for manual selection when the clipboard is unavailable.
#[tauri::command]
#[specta::specta]
pub async fn app_copy_diagnostics(
    state: State<'_, AppState>,
) -> Result<CopyDiagnosticsDto, IpcError> {
    let text = collect_report(&state).await;
    let outcome = copy_report(state.clipboard(), text).await;
    Ok(CopyDiagnosticsDto {
        text: outcome.text,
        copied: outcome.copied,
    })
}
