use application::settings::UpdateSettingsInput;
use domain::settings::{CloseBehavior, CredentialKind, HfEngine, Theme};
use tauri::State;

use super::{
    dto::{
        AppInfoDto, CredentialKindDto, CredentialStatusDto, SecretSetInput, SettingsDto,
        UpdateSettingsDto,
    },
    error::IpcError,
};
use crate::state::AppState;

#[tauri::command]
#[specta::specta]
pub async fn app_info(state: State<'_, AppState>) -> Result<AppInfoDto, IpcError> {
    Ok(AppInfoDto {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        data_dir: state.data_dir.display().to_string(),
        credential_backend: state.credentials.backend().into(),
    })
}

#[tauri::command]
#[specta::specta]
pub async fn settings_get(state: State<'_, AppState>) -> Result<SettingsDto, IpcError> {
    let settings = state.services()?.get_settings.execute().await?;
    Ok(SettingsDto::from(&settings))
}

#[tauri::command]
#[specta::specta]
pub async fn settings_set(
    state: State<'_, AppState>,
    input: UpdateSettingsDto,
) -> Result<SettingsDto, IpcError> {
    let settings = state
        .services()?
        .update_settings
        .execute(UpdateSettingsInput {
            download_concurrency: input.download_concurrency,
            hf_engine: HfEngine::from(input.hf_engine).as_str().to_owned(),
            close_behavior: CloseBehavior::from(input.close_behavior)
                .as_str()
                .to_owned(),
            theme: Theme::from(input.theme).as_str().to_owned(),
            registry_cache_ttl: input.registry_cache_ttl,
            bandwidth_cap_kib_s: input.bandwidth_cap_kib_s,
        })
        .await?;
    // The queue applies a lowered limit to new admissions only; running transfers continue.
    state
        .services()?
        .downloads
        .stack
        .supervisor
        .set_concurrency(settings.download_concurrency.get() as usize);
    state
        .services()?
        .downloads
        .stack
        .bandwidth
        .set_limit(settings.bandwidth_cap_kib_s);
    Ok(SettingsDto::from(&settings))
}

#[tauri::command]
#[specta::specta]
pub async fn secret_set(state: State<'_, AppState>, input: SecretSetInput) -> Result<(), IpcError> {
    let SecretSetInput { kind, value } = input;
    let kind = CredentialKind::from(kind);
    let lease = state.ingress.stage(kind, value);
    state
        .set_credential
        .execute(kind, lease.reference())
        .await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn secret_clear(
    state: State<'_, AppState>,
    kind: CredentialKindDto,
) -> Result<(), IpcError> {
    state.clear_credential.execute(kind.into()).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn secret_status(
    state: State<'_, AppState>,
    kind: CredentialKindDto,
) -> Result<CredentialStatusDto, IpcError> {
    let kind = CredentialKind::from(kind);
    let status = state.get_credential_status.execute(kind).await?;
    let mut dto = CredentialStatusDto::from(status);
    if let Ok(services) = state.services() {
        dto.last_test = services
            .downloads
            .last_test
            .last_test(kind)
            .await
            .ok()
            .flatten()
            .map(Into::into);
    }
    Ok(dto)
}
