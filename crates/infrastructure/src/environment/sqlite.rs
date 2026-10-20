//! SQLite persistence of the environment context: environments, provisioning runs, the atomic
//! activation switch and the wizard draft. Tables come from `002_environment.sql`; the
//! migration list and the connection live in `shared::sqlite`.

use std::path::{Path, PathBuf};

use application::environment::{
    AcceleratorResult, ActivationPhase, ActivationStore, ComfyRef, ComponentStatus, ConsentRequest,
    CuratedProgress, CuratedStatus, EnvRepoError, EnvironmentRepository, FailureKind,
    InstalledComponents, ManagerResult, ProvisioningRun, ProvisioningRunRepository,
    PythonSourceInput, PythonTarget, ResolvedGit, ResolvedSources, RunId, RunStatus, SetupRequest,
    StepFailure, StepId, StepPolicy, StepRecord, StepStatus, WizardDraftStore,
};
use async_trait::async_trait;
use domain::environment::{
    EnvId, EnvState, NewEnvironment, Ownership, PinSet, PythonEnvironment, PythonSourceKind,
    PythonVersion, SetupMode, SetupProfile,
};
use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use super::profiles::ProfileDoc;
use crate::shared::sqlite::{Db, DbError};

#[derive(Clone)]
pub struct SqliteEnvironmentStore {
    db: Db,
}

impl SqliteEnvironmentStore {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

fn unavailable(_: DbError) -> EnvRepoError {
    EnvRepoError::Unavailable
}

fn sql(_: rusqlite::Error) -> DbError {
    DbError::Unavailable
}

fn text(path: &Path) -> Result<String, EnvRepoError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or(EnvRepoError::Corrupt)
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StatusDoc {
    Installed,
    Failed,
    Skipped,
}

impl From<ComponentStatus> for StatusDoc {
    fn from(status: ComponentStatus) -> Self {
        match status {
            ComponentStatus::Installed => Self::Installed,
            ComponentStatus::Failed => Self::Failed,
            ComponentStatus::Skipped => Self::Skipped,
        }
    }
}

impl From<StatusDoc> for ComponentStatus {
    fn from(status: StatusDoc) -> Self {
        match status {
            StatusDoc::Installed => Self::Installed,
            StatusDoc::Failed => Self::Failed,
            StatusDoc::Skipped => Self::Skipped,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AcceleratorResultDoc {
    id: String,
    selected: bool,
    status: StatusDoc,
    importable: Option<bool>,
    message: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManagerResultDoc {
    selected: bool,
    status: StatusDoc,
    source_commit: Option<String>,
    message: Option<String>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ComponentsDoc {
    manager: Option<ManagerResultDoc>,
    accelerators: Vec<AcceleratorResultDoc>,
}

impl From<&InstalledComponents> for ComponentsDoc {
    fn from(components: &InstalledComponents) -> Self {
        Self {
            manager: components.manager.as_ref().map(|r| ManagerResultDoc {
                selected: r.selected,
                status: r.status.into(),
                source_commit: r.source_commit.clone(),
                message: r.message.clone(),
            }),
            accelerators: components
                .accelerators
                .iter()
                .map(|r| AcceleratorResultDoc {
                    id: r.id.clone(),
                    selected: r.selected,
                    status: r.status.into(),
                    importable: r.importable,
                    message: r.message.clone(),
                })
                .collect(),
        }
    }
}

impl From<ComponentsDoc> for InstalledComponents {
    fn from(doc: ComponentsDoc) -> Self {
        Self {
            manager: doc.manager.map(|r| ManagerResult {
                selected: r.selected,
                status: r.status.into(),
                source_commit: r.source_commit,
                message: r.message,
            }),
            accelerators: doc
                .accelerators
                .into_iter()
                .map(|r| AcceleratorResult {
                    id: r.id,
                    selected: r.selected,
                    status: r.status.into(),
                    importable: r.importable,
                    message: r.message,
                })
                .collect(),
        }
    }
}

// ------------------------------------------------------------------------ environments

struct EnvRow {
    id: String,
    path: String,
    owned: i64,
    source: String,
    base_interpreter: Option<String>,
    python_version: String,
    profile_id: String,
    profile_revision: u32,
    pins_json: String,
    verification: Option<String>,
    authorized_location: String,
    state: String,
    created_at: String,
    updated_at: String,
}

const ENV_COLUMNS: &str = "id, path, owned, source, base_interpreter, python_version, \
     profile_id, profile_revision, pins_json, verification, authorized_location, state, \
     created_at, updated_at";

fn read_env_row(row: &Row<'_>) -> rusqlite::Result<EnvRow> {
    Ok(EnvRow {
        id: row.get(0)?,
        path: row.get(1)?,
        owned: row.get(2)?,
        source: row.get(3)?,
        base_interpreter: row.get(4)?,
        python_version: row.get(5)?,
        profile_id: row.get(6)?,
        profile_revision: row.get(7)?,
        pins_json: row.get(8)?,
        verification: row.get(9)?,
        authorized_location: row.get(10)?,
        state: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
    })
}

fn decode_pins(json: &str) -> Result<PinSet, EnvRepoError> {
    let map: std::collections::BTreeMap<String, String> =
        serde_json::from_str(json).map_err(|_| EnvRepoError::Corrupt)?;
    let mut pins = PinSet::new();
    for (name, version) in map {
        pins.insert(
            domain::environment::DistName::new(&name).map_err(|_| EnvRepoError::Corrupt)?,
            version,
        );
    }
    Ok(pins)
}

fn encode_pins(pins: &PinSet) -> String {
    let map: std::collections::BTreeMap<String, &str> = pins
        .iter()
        .map(|(name, version)| (name.to_string(), version))
        .collect();
    serde_json::to_string(&map).unwrap_or_else(|_| "{}".to_owned())
}

fn env_from_row(row: EnvRow) -> Result<PythonEnvironment, EnvRepoError> {
    let corrupt = |_| EnvRepoError::Corrupt;
    let ownership = if row.owned == 1 {
        Ownership::AppOwned
    } else {
        Ownership::Adopted
    };
    let new = NewEnvironment {
        id: EnvId::new(row.id).map_err(corrupt)?,
        path: PathBuf::from(row.path),
        ownership,
        source: row.source.parse::<PythonSourceKind>().map_err(corrupt)?,
        base_interpreter: row.base_interpreter.map(PathBuf::from),
        python_version: row
            .python_version
            .parse::<PythonVersion>()
            .map_err(corrupt)?,
        profile_id: row.profile_id,
        profile_revision: row.profile_revision,
        authorized_location: PathBuf::from(row.authorized_location),
        now: row.updated_at,
    };
    Ok(PythonEnvironment::restore(
        new,
        row.state.parse::<EnvState>().map_err(corrupt)?,
        decode_pins(&row.pins_json)?,
        row.verification,
        row.created_at,
    ))
}

#[async_trait]
impl EnvironmentRepository for SqliteEnvironmentStore {
    async fn list(&self) -> Result<Vec<PythonEnvironment>, EnvRepoError> {
        let rows = self
            .db
            .run(|connection| {
                let mut statement = connection
                    .prepare(&format!(
                        "SELECT {ENV_COLUMNS} FROM envs ORDER BY created_at, rowid"
                    ))
                    .map_err(sql)?;
                let rows = statement
                    .query_map([], read_env_row)
                    .map_err(sql)?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(sql)?;
                Ok(rows)
            })
            .await
            .map_err(unavailable)?;
        rows.into_iter().map(env_from_row).collect()
    }

    async fn get(&self, id: &EnvId) -> Result<Option<PythonEnvironment>, EnvRepoError> {
        let id = id.to_string();
        let row = self
            .db
            .run(move |connection| {
                connection
                    .query_row(
                        &format!("SELECT {ENV_COLUMNS} FROM envs WHERE id = ?1"),
                        [id],
                        read_env_row,
                    )
                    .optional()
                    .map_err(sql)
            })
            .await
            .map_err(unavailable)?;
        row.map(env_from_row).transpose()
    }

    async fn active(&self) -> Result<Option<PythonEnvironment>, EnvRepoError> {
        let row = self
            .db
            .run(|connection| {
                connection
                    .query_row(
                        &format!("SELECT {ENV_COLUMNS} FROM envs WHERE active = 1"),
                        [],
                        read_env_row,
                    )
                    .optional()
                    .map_err(sql)
            })
            .await
            .map_err(unavailable)?;
        row.map(env_from_row).transpose()
    }

    async fn save(&self, env: &PythonEnvironment) -> Result<(), EnvRepoError> {
        let torch_version = env
            .pins
            .get(&domain::environment::DistName::new("torch").expect("static name"))
            .map(str::to_owned);
        let cuda_variant = torch_version
            .as_deref()
            .and_then(|version| version.split_once('+'))
            .map(|(_, variant)| variant.to_owned());
        let triton_dist = ["triton-windows", "triton"].iter().find_map(|name| {
            domain::environment::DistName::new(name)
                .ok()
                .filter(|name| env.pins.contains(name))
                .map(|name| name.to_string())
        });
        let values = (
            env.id.to_string(),
            text(&env.path)?,
            i64::from(env.ownership == Ownership::AppOwned),
            env.source.as_str().to_owned(),
            env.base_interpreter.as_deref().map(text).transpose()?,
            env.python_version.to_string(),
            torch_version,
            cuda_variant,
            env.profile_id.clone(),
            env.profile_revision,
            triton_dist,
            encode_pins(&env.pins),
            env.verification.clone(),
            text(&env.authorized_location)?,
            env.state().as_str().to_owned(),
            env.created_at.clone(),
            env.updated_at.clone(),
        );
        self.db
            .run(move |connection| {
                connection
                    .execute(
                        "INSERT INTO envs (id, path, owned, source, base_interpreter, \
                         python_version, torch_version, cuda_variant, profile_id, profile_revision, \
                         triton_dist, pins_json, verification, authorized_location, state, \
                         created_at, updated_at, active) \
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, \
                         ?16, ?17, 0) \
                         ON CONFLICT(id) DO UPDATE SET path = excluded.path, owned = excluded.owned, \
                         source = excluded.source, base_interpreter = excluded.base_interpreter, \
                         python_version = excluded.python_version, \
                         torch_version = excluded.torch_version, \
                         cuda_variant = excluded.cuda_variant, profile_id = excluded.profile_id, \
                         profile_revision = excluded.profile_revision, \
                         triton_dist = excluded.triton_dist, pins_json = excluded.pins_json, \
                         verification = excluded.verification, \
                         authorized_location = excluded.authorized_location, \
                         state = excluded.state, updated_at = excluded.updated_at",
                        params![
                            values.0, values.1, values.2, values.3, values.4, values.5, values.6,
                            values.7, values.8, values.9, values.10, values.11, values.12,
                            values.13, values.14, values.15, values.16
                        ],
                    )
                    .map_err(sql)?;
                Ok(())
            })
            .await
            .map_err(unavailable)
    }

    async fn load_components(&self, id: &EnvId) -> Result<InstalledComponents, EnvRepoError> {
        let id = id.to_string();
        let json: Option<String> = self
            .db
            .run(move |connection| {
                connection
                    .query_row(
                        "SELECT components_json FROM envs WHERE id = ?1",
                        [id],
                        |row| row.get(0),
                    )
                    .optional()
                    .map(|row: Option<Option<String>>| row.flatten())
                    .map_err(sql)
            })
            .await
            .map_err(unavailable)?;
        json.map(|json| {
            serde_json::from_str::<ComponentsDoc>(&json)
                .map(InstalledComponents::from)
                .map_err(|_| EnvRepoError::Corrupt)
        })
        .transpose()
        .map(Option::unwrap_or_default)
    }

    async fn save_components(
        &self,
        id: &EnvId,
        components: &InstalledComponents,
    ) -> Result<(), EnvRepoError> {
        let id = id.to_string();
        let json = serde_json::to_string(&ComponentsDoc::from(components))
            .map_err(|_| EnvRepoError::Corrupt)?;
        let updated = self
            .db
            .run(move |connection| {
                connection
                    .execute(
                        "UPDATE envs SET components_json = ?2 WHERE id = ?1",
                        params![id, json],
                    )
                    .map_err(sql)
            })
            .await
            .map_err(unavailable)?;
        if updated == 1 {
            Ok(())
        } else {
            Err(EnvRepoError::Conflict)
        }
    }

    async fn delete(&self, id: &EnvId) -> Result<(), EnvRepoError> {
        let id = id.to_string();
        self.db
            .run(move |connection| {
                let active: Option<i64> = connection
                    .query_row("SELECT active FROM envs WHERE id = ?1", [&id], |row| {
                        row.get(0)
                    })
                    .optional()
                    .map_err(sql)?;
                if active == Some(1) {
                    return Ok(false);
                }
                connection
                    .execute("DELETE FROM envs WHERE id = ?1", [&id])
                    .map_err(sql)?;
                Ok(true)
            })
            .await
            .map_err(unavailable)
            .and_then(|deleted| {
                if deleted {
                    Ok(())
                } else {
                    Err(EnvRepoError::Conflict)
                }
            })
    }
}

// ----------------------------------------------------------------------------------- runs

#[derive(Serialize, Deserialize)]
struct SourceDoc {
    kind: String,
    path: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct RefDoc {
    kind: String,
    value: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct RequestDoc {
    mode: String,
    install_root: String,
    python_source: SourceDoc,
    profile_id: String,
    comfyui_ref: RefDoc,
    install_manager: bool,
    #[serde(default)]
    accelerators: Vec<String>,
    #[serde(default)]
    curated_nodes: Vec<String>,
    clean_cache: bool,
    confirm_adopted_changes: bool,
    allow_source_builds: bool,
    #[serde(default)]
    third_party_consent: bool,
}

#[derive(Serialize, Deserialize)]
struct TargetDoc {
    ownership: String,
    source: String,
    env_path: String,
    interpreter: Option<String>,
    create_venv: bool,
    development_files_ok: Option<bool>,
}

#[derive(Serialize, Deserialize)]
struct GitDoc {
    url: String,
    reference: String,
    branch: Option<String>,
    commit: String,
}

#[derive(Serialize, Deserialize, Default)]
struct ResolvedDoc {
    comfyui: Option<GitDoc>,
    manager: Option<GitDoc>,
}

#[derive(Serialize, Deserialize)]
struct StepDoc {
    step: String,
    status: String,
    attempts: u32,
    message: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct ConsentDoc {
    source: String,
    commit: String,
}

#[derive(Serialize, Deserialize)]
struct FailureDoc {
    step: String,
    kind: String,
    message: String,
    command: Option<String>,
    exit_code: Option<i32>,
    tail: Vec<String>,
    consent: Option<ConsentDoc>,
    /// Absent in runs saved before the backend decided it: derived from the step's policy.
    #[serde(default)]
    can_skip: Option<bool>,
}

#[derive(Serialize, Deserialize)]
struct CuratedDoc {
    id: String,
    status: String,
    plan: Option<String>,
    operation: Option<String>,
    message: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct RunDoc {
    request: RequestDoc,
    profile: ProfileDoc,
    python_target: TargetDoc,
    resolved: ResolvedDoc,
    steps: Vec<StepDoc>,
    failure: Option<FailureDoc>,
    manager_consent: Option<String>,
    #[serde(default)]
    components: Option<ComponentsDoc>,
    #[serde(default)]
    curated: Vec<CuratedDoc>,
}

fn git_doc(git: &ResolvedGit) -> GitDoc {
    GitDoc {
        url: git.url.clone(),
        reference: git.reference.clone(),
        branch: git.branch.clone(),
        commit: git.commit.clone(),
    }
}

fn git_from(doc: GitDoc) -> ResolvedGit {
    ResolvedGit {
        url: doc.url,
        reference: doc.reference,
        branch: doc.branch,
        commit: doc.commit,
    }
}

fn encode_run(run: &ProvisioningRun) -> Result<String, EnvRepoError> {
    let (source_kind, source_path) = match &run.request.python_source {
        PythonSourceInput::Managed => ("managed", None),
        PythonSourceInput::CustomVenv(path) => ("custom_venv", Some(text(path)?)),
        PythonSourceInput::CustomInterpreter(path) => ("custom_interpreter", Some(text(path)?)),
    };
    let (ref_kind, ref_value) = match &run.request.comfyui_ref {
        ComfyRef::Latest => ("latest", None),
        ComfyRef::Tag(tag) => ("tag", Some(tag.clone())),
        ComfyRef::Branch(branch) => ("branch", Some(branch.clone())),
        ComfyRef::KeepCurrent => ("keep_current", None),
    };
    let doc = RunDoc {
        request: RequestDoc {
            mode: run.request.mode.as_str().to_owned(),
            install_root: text(&run.request.install_root)?,
            python_source: SourceDoc {
                kind: source_kind.to_owned(),
                path: source_path,
            },
            profile_id: run.request.profile_id.clone(),
            comfyui_ref: RefDoc {
                kind: ref_kind.to_owned(),
                value: ref_value,
            },
            install_manager: run.request.install_manager,
            accelerators: run.request.accelerators.clone(),
            curated_nodes: run.request.curated_nodes.clone(),
            clean_cache: run.request.clean_cache,
            confirm_adopted_changes: run.request.confirm_adopted_changes,
            allow_source_builds: run.request.allow_source_builds,
            third_party_consent: run.request.third_party_consent,
        },
        profile: ProfileDoc::from(&run.profile),
        python_target: TargetDoc {
            ownership: run.python_target.ownership.as_str().to_owned(),
            source: run.python_target.source.as_str().to_owned(),
            env_path: text(&run.python_target.env_path)?,
            interpreter: run
                .python_target
                .interpreter
                .as_deref()
                .map(text)
                .transpose()?,
            create_venv: run.python_target.create_venv,
            development_files_ok: run.python_target.development_files_ok,
        },
        resolved: ResolvedDoc {
            comfyui: run.resolved.comfyui.as_ref().map(git_doc),
            manager: run.resolved.manager.as_ref().map(git_doc),
        },
        steps: run
            .steps
            .iter()
            .map(|record| StepDoc {
                step: record.step.as_str().to_owned(),
                status: record.status.as_str().to_owned(),
                attempts: record.attempts,
                message: record.message.clone(),
            })
            .collect(),
        failure: run.failure.as_ref().map(|failure| FailureDoc {
            step: failure.step.as_str().to_owned(),
            kind: failure.kind.as_str().to_owned(),
            message: failure.message.clone(),
            command: failure.command.clone(),
            exit_code: failure.exit_code,
            tail: failure.tail.clone(),
            consent: failure.consent.as_ref().map(|consent| ConsentDoc {
                source: consent.source.clone(),
                commit: consent.commit.clone(),
            }),
            can_skip: Some(failure.can_skip),
        }),
        manager_consent: run.manager_consent.clone(),
        components: Some(ComponentsDoc::from(&run.components)),
        curated: run
            .curated
            .iter()
            .map(|entry| CuratedDoc {
                id: entry.id.clone(),
                status: entry.status.as_str().to_owned(),
                plan: entry.plan.clone(),
                operation: entry.operation.clone(),
                message: entry.message.clone(),
            })
            .collect(),
    };
    serde_json::to_string(&doc).map_err(|_| EnvRepoError::Corrupt)
}

struct RunRow {
    id: String,
    status: String,
    target_env_id: String,
    previous_env_id: Option<String>,
    doc_json: String,
    cancel_requested: i64,
    activation_phase: String,
    created_at: String,
    updated_at: String,
}

const RUN_COLUMNS: &str = "id, status, target_env_id, previous_env_id, doc_json, \
     cancel_requested, activation_phase, created_at, updated_at";

fn read_run_row(row: &Row<'_>) -> rusqlite::Result<RunRow> {
    Ok(RunRow {
        id: row.get(0)?,
        status: row.get(1)?,
        target_env_id: row.get(2)?,
        previous_env_id: row.get(3)?,
        doc_json: row.get(4)?,
        cancel_requested: row.get(5)?,
        activation_phase: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

fn run_from_row(row: RunRow) -> Result<ProvisioningRun, EnvRepoError> {
    let corrupt = |_| EnvRepoError::Corrupt;
    let doc: RunDoc = serde_json::from_str(&row.doc_json).map_err(|_| EnvRepoError::Corrupt)?;
    let python_source = match (
        doc.request.python_source.kind.as_str(),
        doc.request.python_source.path,
    ) {
        ("managed", _) => PythonSourceInput::Managed,
        ("custom_venv", Some(path)) => PythonSourceInput::CustomVenv(PathBuf::from(path)),
        ("custom_interpreter", Some(path)) => {
            PythonSourceInput::CustomInterpreter(PathBuf::from(path))
        }
        _ => return Err(EnvRepoError::Corrupt),
    };
    let comfyui_ref = match (
        doc.request.comfyui_ref.kind.as_str(),
        doc.request.comfyui_ref.value,
    ) {
        ("latest", _) => ComfyRef::Latest,
        ("tag", Some(tag)) => ComfyRef::Tag(tag),
        ("branch", Some(branch)) => ComfyRef::Branch(branch),
        ("keep_current", _) => ComfyRef::KeepCurrent,
        _ => return Err(EnvRepoError::Corrupt),
    };
    let steps = doc
        .steps
        .into_iter()
        .map(|step| {
            Ok(StepRecord {
                step: step
                    .step
                    .parse::<StepId>()
                    .map_err(|_| EnvRepoError::Corrupt)?,
                status: step
                    .status
                    .parse::<StepStatus>()
                    .map_err(|_| EnvRepoError::Corrupt)?,
                attempts: step.attempts,
                message: step.message,
            })
        })
        .collect::<Result<Vec<_>, EnvRepoError>>()?;
    if steps.len() != StepId::ALL.len() {
        return Err(EnvRepoError::Corrupt);
    }
    let failure = doc
        .failure
        .map(|failure| {
            let step = failure
                .step
                .parse::<StepId>()
                .map_err(|_| EnvRepoError::Corrupt)?;
            Ok(StepFailure {
                step,
                can_skip: failure
                    .can_skip
                    .unwrap_or(step.policy() == StepPolicy::Optional),
                kind: failure
                    .kind
                    .parse::<FailureKind>()
                    .map_err(|_| EnvRepoError::Corrupt)?,
                message: failure.message,
                command: failure.command,
                exit_code: failure.exit_code,
                tail: failure.tail,
                consent: failure.consent.map(|consent| ConsentRequest {
                    source: consent.source,
                    commit: consent.commit,
                }),
            })
        })
        .transpose()?;
    let profile = SetupProfile::try_from(doc.profile).map_err(|_| EnvRepoError::Corrupt)?;
    let legacy_components = doc.components.is_none();
    let mut run = ProvisioningRun {
        id: RunId::new(row.id).map_err(|_| EnvRepoError::Corrupt)?,
        request: SetupRequest {
            mode: doc.request.mode.parse::<SetupMode>().map_err(corrupt)?,
            install_root: PathBuf::from(doc.request.install_root),
            python_source,
            profile_id: doc.request.profile_id,
            comfyui_ref,
            install_manager: doc.request.install_manager,
            accelerators: doc.request.accelerators,
            curated_nodes: doc.request.curated_nodes,
            clean_cache: doc.request.clean_cache,
            confirm_adopted_changes: doc.request.confirm_adopted_changes,
            allow_source_builds: doc.request.allow_source_builds,
            third_party_consent: doc.request.third_party_consent,
        },
        profile,
        target_env: EnvId::new(row.target_env_id).map_err(corrupt)?,
        previous_env: row
            .previous_env_id
            .map(|id| EnvId::new(id).map_err(corrupt))
            .transpose()?,
        python_target: PythonTarget {
            ownership: doc
                .python_target
                .ownership
                .parse::<Ownership>()
                .map_err(corrupt)?,
            source: doc
                .python_target
                .source
                .parse::<PythonSourceKind>()
                .map_err(corrupt)?,
            env_path: PathBuf::from(doc.python_target.env_path),
            interpreter: doc.python_target.interpreter.map(PathBuf::from),
            create_venv: doc.python_target.create_venv,
            development_files_ok: doc.python_target.development_files_ok,
        },
        resolved: ResolvedSources {
            comfyui: doc.resolved.comfyui.map(git_from),
            manager: doc.resolved.manager.map(git_from),
        },
        steps,
        status: row
            .status
            .parse::<RunStatus>()
            .map_err(|_| EnvRepoError::Corrupt)?,
        cancel_requested: row.cancel_requested == 1,
        failure,
        activation: row
            .activation_phase
            .parse::<ActivationPhase>()
            .map_err(|_| EnvRepoError::Corrupt)?,
        manager_consent: doc.manager_consent,
        curated: doc
            .curated
            .into_iter()
            .map(|entry| {
                Ok(CuratedProgress {
                    id: entry.id,
                    status: entry
                        .status
                        .parse::<CuratedStatus>()
                        .map_err(|_| EnvRepoError::Corrupt)?,
                    plan: entry.plan,
                    operation: entry.operation,
                    message: entry.message,
                })
            })
            .collect::<Result<Vec<_>, EnvRepoError>>()?,
        components: doc
            .components
            .map(InstalledComponents::from)
            .unwrap_or_default(),
        created_at: row.created_at,
        updated_at: row.updated_at,
    };
    if legacy_components {
        run.components = InstalledComponents::selected(&run.request, &run.profile);
        let manager_status = run.step(StepId::Manager).status;
        if let Some(manager) = &mut run.components.manager {
            manager.source_commit = run.resolved.manager.as_ref().map(|git| git.commit.clone());
            manager.status = match manager_status {
                StepStatus::Succeeded => ComponentStatus::Installed,
                StepStatus::Failed | StepStatus::Warning | StepStatus::Interrupted => {
                    ComponentStatus::Failed
                }
                _ => ComponentStatus::Skipped,
            };
        }
    }
    Ok(run)
}

struct RunValues {
    id: String,
    status: String,
    mode: String,
    target: String,
    previous: Option<String>,
    doc: String,
    cancel: i64,
    phase: String,
    created: String,
    updated: String,
}

fn run_values(run: &ProvisioningRun) -> Result<RunValues, EnvRepoError> {
    Ok(RunValues {
        id: run.id.to_string(),
        status: run.status.as_str().to_owned(),
        mode: run.request.mode.as_str().to_owned(),
        target: run.target_env.to_string(),
        previous: run.previous_env.as_ref().map(ToString::to_string),
        doc: encode_run(run)?,
        cancel: i64::from(run.cancel_requested),
        phase: run.activation.as_str().to_owned(),
        created: run.created_at.clone(),
        updated: run.updated_at.clone(),
    })
}

fn update_run(connection: &rusqlite::Connection, values: &RunValues) -> rusqlite::Result<usize> {
    connection.execute(
        "UPDATE provisioning_runs SET status = ?2, mode = ?3, target_env_id = ?4, \
         previous_env_id = ?5, doc_json = ?6, cancel_requested = ?7, activation_phase = ?8, \
         updated_at = ?9 WHERE id = ?1",
        params![
            values.id,
            values.status,
            values.mode,
            values.target,
            values.previous,
            values.doc,
            values.cancel,
            values.phase,
            values.updated
        ],
    )
}

#[async_trait]
impl ProvisioningRunRepository for SqliteEnvironmentStore {
    async fn insert(&self, run: &ProvisioningRun) -> Result<(), EnvRepoError> {
        let values = run_values(run)?;
        self.db
            .run(move |connection| {
                connection
                    .execute(
                        "INSERT INTO provisioning_runs (id, status, mode, target_env_id, \
                         previous_env_id, doc_json, cancel_requested, activation_phase, \
                         created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                        params![
                            values.id,
                            values.status,
                            values.mode,
                            values.target,
                            values.previous,
                            values.doc,
                            values.cancel,
                            values.phase,
                            values.created,
                            values.updated
                        ],
                    )
                    .map_err(sql)?;
                Ok(())
            })
            .await
            .map_err(unavailable)
    }

    async fn save(&self, run: &ProvisioningRun) -> Result<(), EnvRepoError> {
        let values = run_values(run)?;
        let updated = self
            .db
            .run(move |connection| update_run(connection, &values).map_err(sql))
            .await
            .map_err(unavailable)?;
        if updated == 1 {
            Ok(())
        } else {
            Err(EnvRepoError::Conflict)
        }
    }

    async fn get(&self, id: &RunId) -> Result<Option<ProvisioningRun>, EnvRepoError> {
        let id = id.to_string();
        let row = self
            .db
            .run(move |connection| {
                connection
                    .query_row(
                        &format!("SELECT {RUN_COLUMNS} FROM provisioning_runs WHERE id = ?1"),
                        [id],
                        read_run_row,
                    )
                    .optional()
                    .map_err(sql)
            })
            .await
            .map_err(unavailable)?;
        row.map(run_from_row).transpose()
    }

    async fn latest(&self) -> Result<Option<ProvisioningRun>, EnvRepoError> {
        let row = self
            .db
            .run(|connection| {
                connection
                    .query_row(
                        &format!(
                            "SELECT {RUN_COLUMNS} FROM provisioning_runs \
                             ORDER BY created_at DESC, rowid DESC LIMIT 1"
                        ),
                        [],
                        read_run_row,
                    )
                    .optional()
                    .map_err(sql)
            })
            .await
            .map_err(unavailable)?;
        row.map(run_from_row).transpose()
    }

    async fn unfinished(&self) -> Result<Vec<ProvisioningRun>, EnvRepoError> {
        let rows = self
            .db
            .run(|connection| {
                let mut statement = connection
                    .prepare(&format!(
                        "SELECT {RUN_COLUMNS} FROM provisioning_runs \
                         WHERE status IN ('registered', 'running', 'failed', 'interrupted') \
                         OR activation_phase IN ('intent', 'manifest_published') \
                         ORDER BY created_at, id"
                    ))
                    .map_err(sql)?;
                let rows = statement
                    .query_map([], read_run_row)
                    .map_err(sql)?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(sql)?;
                Ok(rows)
            })
            .await
            .map_err(unavailable)?;
        rows.into_iter().map(run_from_row).collect()
    }
}

#[async_trait]
impl ActivationStore for SqliteEnvironmentStore {
    async fn commit_activation(
        &self,
        run: &ProvisioningRun,
        new_active: &EnvId,
    ) -> Result<(), EnvRepoError> {
        let values = run_values(run)?;
        let new_active = new_active.to_string();
        self.db
            .run(move |connection| {
                let transaction = connection.transaction().map_err(sql)?;
                transaction
                    .execute("UPDATE envs SET active = 0 WHERE active = 1", [])
                    .map_err(sql)?;
                let activated = transaction
                    .execute("UPDATE envs SET active = 1 WHERE id = ?1", [&new_active])
                    .map_err(sql)?;
                if activated != 1 {
                    return Err(DbError::Unavailable);
                }
                if update_run(&transaction, &values).map_err(sql)? != 1 {
                    return Err(DbError::Unavailable);
                }
                transaction.commit().map_err(sql)
            })
            .await
            .map_err(unavailable)
    }
}

#[async_trait]
impl WizardDraftStore for SqliteEnvironmentStore {
    async fn load(&self) -> Result<Option<String>, EnvRepoError> {
        self.db
            .run(|connection| {
                connection
                    .query_row("SELECT json FROM wizard_draft WHERE id = 1", [], |row| {
                        row.get(0)
                    })
                    .optional()
                    .map_err(sql)
            })
            .await
            .map_err(unavailable)
    }

    async fn save(&self, draft: &str) -> Result<(), EnvRepoError> {
        let draft = draft.to_owned();
        self.db
            .run(move |connection| {
                connection
                    .execute(
                        "INSERT INTO wizard_draft (id, json, updated_at) \
                         VALUES (1, ?1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) \
                         ON CONFLICT(id) DO UPDATE SET json = excluded.json, \
                         updated_at = excluded.updated_at",
                        [draft],
                    )
                    .map_err(sql)?;
                Ok(())
            })
            .await
            .map_err(unavailable)
    }
}

#[cfg(test)]
mod tests;
