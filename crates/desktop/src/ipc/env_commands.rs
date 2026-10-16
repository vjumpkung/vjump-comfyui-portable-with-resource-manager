//! Commands of the environment context: first-run setup, re-setup, environment information and
//! package management. Each one is a thin controller over an application use case.

use std::{path::PathBuf, sync::Arc};

use application::environment::{RunId, SetupEvent, SetupProgressSink};
use domain::environment::EnvId;
use infrastructure::environment::sinks::run_sink;
use tauri::{State, ipc::Channel};

use super::{
    env_dto::{
        EnvironmentInfoDto, HardwareDto, PackageDto, PipApplyInput, PipApplyResultDto,
        PipOperationDto, PipPlanDto, ProfileDto, PythonSourceDto, PythonValidationDto,
        RootSelectionDto, RunOutcomeDto, SetupActionDto, SetupEventDto, SetupStartedDto,
        SetupStatusDto,
    },
    error::IpcError,
};
use crate::state::AppState;

/// The largest wizard draft the backend stores.
const MAX_DRAFT_BYTES: usize = 64 * 1024;

fn invalid_identifier() -> IpcError {
    IpcError::new("invalid_identifier", "The identifier is not valid.")
}

/// Forwards setup events to the UI channel. A closed or reloaded UI never stops the work.
struct ChannelSink {
    channel: Channel<SetupEventDto>,
}

impl ChannelSink {
    fn finished(&self, run_id: &RunId, outcome: RunOutcomeDto) {
        let _ = self.channel.send(SetupEventDto::Finished {
            run_id: run_id.to_string(),
            outcome,
        });
    }
}

impl SetupProgressSink for ChannelSink {
    fn emit(&self, event: SetupEvent) {
        let _ = self.channel.send(SetupEventDto::from(&event));
    }
}

#[tauri::command]
#[specta::specta]
pub async fn setup_status(state: State<'_, AppState>) -> Result<SetupStatusDto, IpcError> {
    if let Some(info) = state.recovery() {
        return Ok(SetupStatusDto::recovery(info));
    }
    let status = state.services()?.setup_status.execute().await?;
    Ok(SetupStatusDto::from(&status))
}

#[tauri::command]
#[specta::specta]
pub async fn setup_detect_hardware(
    state: State<'_, AppState>,
    candidate_root: Option<String>,
    env_path: Option<String>,
) -> Result<HardwareDto, IpcError> {
    let services = state.services()?;
    let root = candidate_root.map_or_else(|| services.root.root.clone(), PathBuf::from);
    let env_path = env_path.map(PathBuf::from);
    let hardware = services
        .detect_hardware
        .execute(&root, env_path.as_deref())
        .await;
    Ok(HardwareDto::from(&hardware))
}

#[tauri::command]
#[specta::specta]
pub async fn setup_list_profiles(
    state: State<'_, AppState>,
    candidate_root: Option<String>,
    env_path: Option<String>,
) -> Result<Vec<ProfileDto>, IpcError> {
    let services = state.services()?;
    let root = candidate_root.map_or_else(|| services.root.root.clone(), PathBuf::from);
    let env_path = env_path.map(PathBuf::from);
    let offers = services
        .list_profiles
        .execute(&root, env_path.as_deref())
        .await;
    Ok(offers.iter().map(ProfileDto::from).collect())
}

#[tauri::command]
#[specta::specta]
pub async fn setup_validate_python(
    state: State<'_, AppState>,
    source: PythonSourceDto,
) -> Result<PythonValidationDto, IpcError> {
    let services = state.services()?;
    let report = services
        .validate_python
        .execute(&services.root.root, &source.into())
        .await?;
    Ok(PythonValidationDto::from(&report))
}

#[tauri::command]
#[specta::specta]
pub async fn setup_select_root(
    state: State<'_, AppState>,
    path: String,
    draft: Option<String>,
) -> Result<RootSelectionDto, IpcError> {
    if draft
        .as_ref()
        .is_some_and(|draft| draft.len() > MAX_DRAFT_BYTES)
    {
        return Err(IpcError::new(
            "validation",
            "The wizard draft is too large.",
        ));
    }
    state
        .services()?
        .select_root
        .execute(PathBuf::from(path), draft)
        .await?;
    Ok(RootSelectionDto {
        restart_required: true,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn setup_run(
    state: State<'_, AppState>,
    action: SetupActionDto,
    on_event: Channel<SetupEventDto>,
) -> Result<SetupStartedDto, IpcError> {
    let services = state.services()?;
    let action = action.into_application(&services.root.root)?;
    // The run is durably registered before the task starts, so a closed channel or a reloaded
    // UI never loses required work.
    let run_id = services.start_setup.execute(action).await?;
    let channel_sink = Arc::new(ChannelSink { channel: on_event });
    let (sink, _log_path) = run_sink(&services.logs_dir, run_id.as_str(), channel_sink.clone());
    let executor = services.execute_setup.clone();
    let task_id = run_id.clone();
    tauri::async_runtime::spawn(async move {
        let outcome = match executor.execute(&task_id, sink).await {
            Ok(outcome) => RunOutcomeDto::from(&outcome),
            Err(error) => {
                tracing::error!(%error, "setup run could not execute");
                RunOutcomeDto::Error
            }
        };
        channel_sink.finished(&task_id, outcome);
    });
    Ok(SetupStartedDto {
        run_id: run_id.to_string(),
    })
}

#[tauri::command]
#[specta::specta]
pub async fn setup_cancel(state: State<'_, AppState>, run_id: String) -> Result<(), IpcError> {
    let run_id = RunId::new(run_id).map_err(|_| invalid_identifier())?;
    state.services()?.cancel_setup.execute(&run_id).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn env_info(state: State<'_, AppState>) -> Result<EnvironmentInfoDto, IpcError> {
    let info = state.services()?.env_info.execute().await?;
    Ok(EnvironmentInfoDto::from(&info))
}

#[tauri::command]
#[specta::specta]
pub async fn env_packages(state: State<'_, AppState>) -> Result<Vec<PackageDto>, IpcError> {
    let packages = state.services()?.list_packages.execute().await?;
    Ok(packages.iter().map(PackageDto::from).collect())
}

#[tauri::command]
#[specta::specta]
pub async fn env_pip_preview(
    state: State<'_, AppState>,
    operation: PipOperationDto,
) -> Result<PipPlanDto, IpcError> {
    let plan = state
        .services()?
        .preview_pip
        .execute(operation.into())
        .await?;
    Ok(PipPlanDto::from(&plan))
}

#[tauri::command]
#[specta::specta]
pub async fn env_pip_apply(
    state: State<'_, AppState>,
    input: PipApplyInput,
) -> Result<PipApplyResultDto, IpcError> {
    let plan_id =
        application::environment::PlanId::new(input.plan_id).map_err(|_| invalid_identifier())?;
    let confirmed = input
        .confirmed_changes
        .into_iter()
        .map(|change| change.into_domain())
        .collect::<Result<Vec<_>, _>>()?;
    let outcome = state
        .services()?
        .apply_pip
        .execute(&plan_id, confirmed)
        .await?;
    Ok(PipApplyResultDto::from(&outcome))
}

#[tauri::command]
#[specta::specta]
pub async fn env_delete(state: State<'_, AppState>, env_id: String) -> Result<(), IpcError> {
    let env_id = EnvId::new(env_id).map_err(|_| invalid_identifier())?;
    state.services()?.delete_env.execute(&env_id).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn pick_folder(
    state: State<'_, AppState>,
    title: Option<String>,
) -> Result<Option<String>, IpcError> {
    Ok(state
        .picker
        .pick_folder(title)
        .await
        .map(|path| path.display().to_string()))
}

#[tauri::command]
#[specta::specta]
pub async fn pick_file(
    state: State<'_, AppState>,
    title: Option<String>,
    extensions: Vec<String>,
) -> Result<Option<String>, IpcError> {
    Ok(state
        .picker
        .pick_file(title, extensions)
        .await
        .map(|path| path.display().to_string()))
}
