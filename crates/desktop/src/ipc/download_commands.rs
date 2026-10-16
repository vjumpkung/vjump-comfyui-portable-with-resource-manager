//! Commands of the model acquisition context. Each one is a thin controller over an application
//! use case; the queue is owned by the Rust core, so closing the window or dropping a
//! subscription never cancels a download.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use application::{
    models::acquisition::{BoundConflictDecision, EnqueueItem},
    shared::IdGenerator,
};
use domain::{
    models::{
        acquisition::{DownloadEngine, DownloadState},
        library::ModelCategory,
    },
    settings::{CredentialKind, HfEngine},
    shared::model_values::{JobId, RelativeModelPath, SafeFileName},
};
use infrastructure::models::acquisition::stack::DEFAULT_MODEL_ROOT_ID;
use tauri::{State, ipc::Channel};

use super::{
    download_dto::{
        ConflictChoiceDto, CredentialTestDto, DownloadCursorDto, DownloadEventDto, DownloadJobDto,
        EnqueueItemInput, EnqueueOutcomeDto, ExportInput, ImportPackInput, PackExportDto,
        PackPreviewRowDto, PackSourceDto, ResolutionDto, ResolveInput, ResumeDecisionDto,
    },
    dto::CredentialKindDto,
    error::IpcError,
};
use crate::state::AppState;

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn validation(message: &str) -> IpcError {
    IpcError::new("validation", message)
}

fn parse_job_id(value: &str) -> Result<JobId, IpcError> {
    JobId::new(value)
        .map_err(|_| IpcError::new("invalid_identifier", "The identifier is not valid."))
}

#[tauri::command]
#[specta::specta]
pub async fn dl_resolve(
    state: State<'_, AppState>,
    input: ResolveInput,
) -> Result<ResolutionDto, IpcError> {
    let downloads = &state.services()?.downloads;
    let source = downloads
        .stack
        .ingress
        .stage(input.url.trim(), input.http_confirmed)
        .map_err(IpcError::from)?;
    let resolution = downloads.resolve.execute(&source).await?;
    Ok(ResolutionDto::from(&resolution))
}

struct Prepared {
    position: usize,
    item: EnqueueItem,
}

fn prepare(
    position: usize,
    input: EnqueueItemInput,
    id: JobId,
    engine: DownloadEngine,
) -> Result<Prepared, IpcError> {
    if !input.resolution_id.is_finite() || input.resolution_id < 0.0 {
        return Err(validation("The resolution is not valid."));
    }
    let category = ModelCategory::new(input.category.trim())
        .map_err(|_| validation("The model folder name is not valid."))?;
    let subdirectory = RelativeModelPath::new(input.subdirectory.trim().trim_matches('/'))
        .map_err(|_| validation("The sub-folder is not valid."))?;
    let filename = input
        .filename
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(SafeFileName::new)
        .transpose()
        .map_err(|_| validation("The file name is not valid."))?;
    let conflict = match input.conflict {
        None => None,
        Some(ConflictChoiceDto::Overwrite { approved: None }) => {
            return Err(validation(
                "Overwriting needs the identity of the existing file.",
            ));
        }
        Some(choice) => choice.into_domain()?,
    };
    Ok(Prepared {
        position,
        item: EnqueueItem {
            resolution_id: input.resolution_id as u64,
            file_index: input.file_index as usize,
            id,
            root_id: DEFAULT_MODEL_ROOT_ID.to_owned(),
            category,
            subdirectory,
            filename,
            engine,
            conflict,
            force: input.force,
        },
    })
}

#[tauri::command]
#[specta::specta]
pub async fn dl_enqueue(
    state: State<'_, AppState>,
    items: Vec<EnqueueItemInput>,
) -> Result<Vec<EnqueueOutcomeDto>, IpcError> {
    if items.len() > 10_000 {
        return Err(validation("Too many downloads were requested at once."));
    }
    let services = state.services()?;
    let downloads = &services.downloads;
    let settings = services.get_settings.execute().await?;
    let engine = match settings.hf_engine {
        HfEngine::HfCli => DownloadEngine::HfCli,
        HfEngine::Native => DownloadEngine::Native,
    };
    let mut outcomes: Vec<Option<EnqueueOutcomeDto>> = vec![None; items.len()];
    let mut prepared = Vec::new();
    for (position, input) in items.into_iter().enumerate() {
        let id = JobId::new(downloads.ids.new_id())
            .map_err(|_| IpcError::new("invalid_identifier", "The identifier is not valid."))?;
        match prepare(position, input, id, engine) {
            Ok(item) => prepared.push(item),
            Err(error) => {
                outcomes[position] = Some(EnqueueOutcomeDto::Invalid {
                    error: super::download_dto::DownloadErrorDto {
                        code: error.code,
                        message: error.message,
                    },
                });
            }
        }
    }
    let positions: Vec<usize> = prepared.iter().map(|p| p.position).collect();
    let results = downloads
        .enqueue
        .execute(prepared.into_iter().map(|p| p.item).collect(), now())
        .await;
    for (position, outcome) in positions.into_iter().zip(results.iter()) {
        outcomes[position] = Some(EnqueueOutcomeDto::from(outcome));
    }
    Ok(outcomes.into_iter().flatten().collect())
}

#[tauri::command]
#[specta::specta]
pub async fn dl_pause(
    state: State<'_, AppState>,
    job_id: String,
) -> Result<DownloadJobDto, IpcError> {
    let job = state
        .services()?
        .downloads
        .pause
        .execute(&parse_job_id(&job_id)?)
        .await?;
    Ok(DownloadJobDto::from(&job))
}

#[tauri::command]
#[specta::specta]
pub async fn dl_cancel(
    state: State<'_, AppState>,
    job_id: String,
) -> Result<DownloadJobDto, IpcError> {
    let job = state
        .services()?
        .downloads
        .cancel
        .execute(&parse_job_id(&job_id)?)
        .await?;
    Ok(DownloadJobDto::from(&job))
}

#[tauri::command]
#[specta::specta]
pub async fn dl_resume(
    state: State<'_, AppState>,
    job_id: String,
    decision: Option<ResumeDecisionDto>,
) -> Result<DownloadJobDto, IpcError> {
    let downloads = &state.services()?.downloads;
    let id = parse_job_id(&job_id)?;
    let decision = match decision {
        None => None,
        Some(ResumeDecisionDto {
            decision_id,
            choice,
        }) => {
            // An overwrite must echo the identity the user was shown; the backend never
            // approves a file it observed after the prompt.
            let choice = choice.into_domain()?.ok_or_else(|| {
                validation("Overwriting needs the identity of the file that was shown.")
            })?;
            Some(BoundConflictDecision {
                decision_id,
                choice,
            })
        }
    };
    let job = downloads.resume.execute(&id, decision).await?;
    Ok(DownloadJobDto::from(&job))
}

#[tauri::command]
#[specta::specta]
pub async fn dl_retry(
    state: State<'_, AppState>,
    job_id: String,
) -> Result<DownloadJobDto, IpcError> {
    let job = state
        .services()?
        .downloads
        .retry
        .execute(&parse_job_id(&job_id)?)
        .await?;
    Ok(DownloadJobDto::from(&job))
}

#[tauri::command]
#[specta::specta]
pub async fn dl_list(
    state: State<'_, AppState>,
    before: Option<f64>,
    limit: u32,
) -> Result<Vec<DownloadJobDto>, IpcError> {
    let before = before
        .filter(|b| b.is_finite() && *b >= 0.0)
        .map(|b| b as u64);
    let jobs = state
        .services()?
        .downloads
        .list
        .execute(before, limit)
        .await?;
    Ok(jobs.iter().map(DownloadJobDto::from).collect())
}

/// Registers a per-subscription channel. The first event is an authoritative snapshot; closing
/// the channel or reloading the UI never stops a download.
#[tauri::command]
#[specta::specta]
pub async fn dl_subscribe(
    state: State<'_, AppState>,
    after_cursor: Option<DownloadCursorDto>,
    on_event: Channel<DownloadEventDto>,
) -> Result<u32, IpcError> {
    let downloads = &state.services()?.downloads;
    let after = after_cursor.and_then(DownloadCursorDto::into_domain);
    let hub = downloads.stack.hub.clone();
    let subscription = hub.subscribe(after.as_ref());
    let id = u32::try_from(subscription.id).map_err(|_| {
        IpcError::new(
            "subscription_exhausted",
            "Too many subscriptions were opened.",
        )
    })?;
    let hub_id = subscription.id;
    let mut replay_gap = subscription.replay_gap;
    let handle = tauri::async_runtime::spawn(async move {
        while let Some(events) = hub.drain(hub_id, 64) {
            if events.is_empty() {
                tokio::select! {
                    () = hub.changed() => {}
                    () = tokio::time::sleep(Duration::from_millis(250)) => {}
                }
                continue;
            }
            for event in events {
                let dto = DownloadEventDto::from_event(&event, replay_gap);
                replay_gap = false;
                let gap = matches!(dto, DownloadEventDto::Gap { .. });
                if on_event.send(dto).is_err() || gap {
                    hub.unsubscribe(hub_id);
                    return;
                }
            }
        }
    });
    if let Some(previous) = downloads
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
pub async fn dl_unsubscribe(
    state: State<'_, AppState>,
    subscription_id: u32,
) -> Result<(), IpcError> {
    let downloads = &state.services()?.downloads;
    let handle = downloads
        .subscriptions
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(&subscription_id);
    if let Some(handle) = handle {
        handle.abort();
    }
    downloads.stack.hub.unsubscribe(u64::from(subscription_id));
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn dl_import_pack(
    state: State<'_, AppState>,
    input: ImportPackInput,
) -> Result<Vec<PackPreviewRowDto>, IpcError> {
    let downloads = &state.services()?.downloads;
    let remote = input.source == PackSourceDto::Url;
    let preview = downloads.import.execute(input.value.trim(), remote).await?;
    Ok(preview.iter().map(PackPreviewRowDto::from).collect())
}

#[tauri::command]
#[specta::specta]
pub async fn dl_export(
    state: State<'_, AppState>,
    input: ExportInput,
) -> Result<PackExportDto, IpcError> {
    let downloads = &state.services()?.downloads;
    let history = downloads.list.execute(None, 500).await?;
    let jobs: Vec<_> = match input.job_ids {
        Some(ids) => history
            .into_iter()
            .filter(|job| ids.iter().any(|id| id == job.id.as_str()))
            .collect(),
        None => history
            .into_iter()
            .filter(|job| job.state == DownloadState::Completed)
            .collect(),
    };
    let export = downloads.export.execute(&jobs)?;
    Ok(PackExportDto::from(&export))
}

/// Tests the saved token against the provider. The result carries no profile data.
#[tauri::command]
#[specta::specta]
pub async fn secret_test(
    state: State<'_, AppState>,
    kind: CredentialKindDto,
) -> Result<CredentialTestDto, IpcError> {
    let kind = CredentialKind::from(kind);
    let status = state
        .services()?
        .downloads
        .test_credential
        .execute(kind, now())
        .await?;
    Ok(CredentialTestDto::from(status))
}
