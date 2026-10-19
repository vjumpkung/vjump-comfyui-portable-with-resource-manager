use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use domain::{
    comfy_runtime::{LaunchCapabilities, Port, RepoState, Revision},
    environment::{PythonEnvironment, PythonVersion},
};

use super::*;
use crate::{
    comfy_runtime::{
        ComfyRepository, ExtraModelPaths, FrozenUpdate, InventoryState, LaunchPreflight, ModelRoot,
        NodeLifecycle, PortProbe, RuntimeBlock, RuntimeError, RuntimeMaintenance, RuntimeSession,
        RuntimeStore, RuntimeSupervisor, UpdateRun, VersionList,
    },
    environment::{
        AcceleratorResult, ComponentStatus, InstallManifest, InstalledComponents, ManagerResult,
        ManifestState, RunId, StepContext,
    },
    shared::{CancelToken, MaintenanceLease},
    test_support::block_on,
};

#[allow(dead_code, clippy::duplicate_mod)]
#[path = "../../environment/tests/fakes.rs"]
mod env_fakes;
use env_fakes::World;

/// Only the queries the report reuses are implemented; anything else is a test bug.
struct Machine {
    env: Arc<World>,
    revision: Mutex<Result<Revision, RuntimeError>>,
}

impl Machine {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            env: World::new(),
            revision: Mutex::new(Err(RuntimeError::Failed("no checkout".into()))),
        })
    }

    fn deps(self: &Arc<Self>) -> RuntimeDeps {
        RuntimeDeps {
            environment: self.env.deps(),
            nodes: self.clone(),
            supervisor: self.clone(),
            store: self.clone(),
            repository: self.clone(),
            ports: self.clone(),
            preflight: self.clone(),
            extra_paths: self.clone(),
        }
    }

    fn collect(
        self: &Arc<Self>,
        app_log: Result<Vec<String>, DiagnosticsError>,
        runtime_log: Vec<String>,
    ) -> DiagnosticsSnapshot {
        block_on(
            CollectDiagnostics::new(
                self.deps(),
                Arc::new(AppLog(app_log)),
                Arc::new(RuntimeLines(runtime_log)),
                "1.2.3",
                vec![
                    PinnedTool {
                        name: "uv".into(),
                        version: "0.13.1".into(),
                    },
                    PinnedTool {
                        name: "hf".into(),
                        version: "2.2.0".into(),
                    },
                ],
            )
            .execute(),
        )
    }
}

#[async_trait]
impl NodeLifecycle for Machine {
    async fn before_start(&self, _: &mut dyn MaintenanceLease) -> Result<(), RuntimeError> {
        unreachable!("diagnostics never start nodes")
    }

    async fn after_stop(&self, _: &mut dyn MaintenanceLease) -> Result<(), RuntimeError> {
        unreachable!("diagnostics never rescan nodes")
    }
}

struct AppLog(Result<Vec<String>, DiagnosticsError>);

#[async_trait]
impl AppLogTail for AppLog {
    async fn last_lines(&self, _: usize) -> Result<Vec<String>, DiagnosticsError> {
        self.0.clone()
    }
}

struct RuntimeLines(Vec<String>);

impl RuntimeLogReader for RuntimeLines {
    fn last_lines(&self, _: usize) -> Vec<String> {
        self.0.clone()
    }
}

#[async_trait]
impl RuntimeMaintenance for Machine {
    async fn ensure_stopped(&self, _: &Path) -> Result<(), RuntimeBlock> {
        Ok(())
    }
}

#[async_trait]
impl RuntimeSupervisor for Machine {
    async fn status(&self) -> RuntimeStatus {
        RuntimeStatus::default()
    }
    async fn inventory(&self, _: &Path) -> Result<InventoryState, RuntimeError> {
        unreachable!("diagnostics never inspect processes")
    }
    async fn start(
        &self,
        _: RuntimeSession,
        _: Vec<String>,
        _: Box<dyn MaintenanceLease>,
    ) -> Result<RuntimeStatus, RuntimeError> {
        unreachable!("diagnostics never start ComfyUI")
    }
    async fn stop(&self) -> Result<RuntimeStatus, RuntimeError> {
        unreachable!("diagnostics never stop ComfyUI")
    }
}

#[async_trait]
impl RuntimeStore for Machine {
    async fn launch_options(&self) -> Result<domain::comfy_runtime::LaunchOptions, RuntimeError> {
        unreachable!()
    }
    async fn save_launch_options(
        &self,
        _: &domain::comfy_runtime::LaunchOptions,
    ) -> Result<(), RuntimeError> {
        unreachable!()
    }
    async fn save_session(&self, _: &RuntimeSession, _: bool) -> Result<(), RuntimeError> {
        unreachable!()
    }
    async fn unfinished_sessions(&self) -> Result<Vec<RuntimeSession>, RuntimeError> {
        unreachable!()
    }
    async fn save_update(&self, _: &UpdateRun) -> Result<(), RuntimeError> {
        unreachable!()
    }
    async fn unfinished_update(&self) -> Result<Option<UpdateRun>, RuntimeError> {
        Ok(None)
    }
}

#[async_trait]
impl ComfyRepository for Machine {
    async fn revision(&self, _: &Path) -> Result<Revision, RuntimeError> {
        self.revision.lock().unwrap().clone()
    }
    async fn status(&self, _: &Path) -> Result<RepoState, RuntimeError> {
        unreachable!()
    }
    async fn fetch(&self, _: &Path, _: &StepContext) -> Result<(), RuntimeError> {
        unreachable!("diagnostics never touch the network")
    }
    async fn freeze(
        &self,
        _: &Path,
        _: &domain::comfy_runtime::UpdateTarget,
    ) -> Result<FrozenUpdate, RuntimeError> {
        unreachable!()
    }
    async fn apply(&self, _: &Path, _: &FrozenUpdate, _: &StepContext) -> Result<(), RuntimeError> {
        unreachable!()
    }
    async fn versions(&self, _: &Path) -> Result<VersionList, RuntimeError> {
        unreachable!()
    }
}

#[async_trait]
impl PortProbe for Machine {
    async fn available(&self, _: Port, _: bool) -> Result<bool, RuntimeError> {
        unreachable!()
    }
    async fn next_free(&self, _: Port, _: bool) -> Result<Option<Port>, RuntimeError> {
        unreachable!()
    }
}

#[async_trait]
impl LaunchPreflight for Machine {
    async fn capabilities(
        &self,
        _: &Path,
        _: &PythonEnvironment,
        _: &CancelToken,
    ) -> Result<LaunchCapabilities, RuntimeError> {
        unreachable!("diagnostics never run the environment's Python")
    }
}

#[async_trait]
impl ExtraModelPaths for Machine {
    async fn read(&self, _: &Path) -> Result<Vec<ModelRoot>, RuntimeError> {
        unreachable!()
    }
}

fn seed_environment_and_manifest(machine: &Machine) {
    let active = machine
        .env
        .seed_active_env("selected", PythonVersion::new(3, 13, 16));
    *machine.env.manifest.lock().unwrap() = ManifestState::Valid(InstallManifest {
        schema_version: 1,
        install_root: env_fakes::ROOT.into(),
        generation: 1,
        env_id: active.id,
        env_path: active.path,
        ownership: active.ownership,
        python_version: active.python_version,
        profile_id: active.profile_id,
        profile_revision: active.profile_revision,
        comfyui_reference: Some("v1".into()),
        comfyui_commit: Some("a".repeat(40)),
        run_id: RunId::new("setup-1").unwrap(),
        activated_at: "t".into(),
        components: InstalledComponents::default(),
    });
}

fn warned(snapshot: &DiagnosticsSnapshot, fragment: &str) -> bool {
    snapshot
        .warnings
        .iter()
        .any(|warning| warning.contains(fragment))
}

#[test]
fn diagnostics_handles_first_run_and_missing_sections() {
    // First run: no environment, no install.json, no checkout, no readable application log.
    let machine = Machine::new();
    let snapshot = machine.collect(Err(DiagnosticsError::Unavailable), Vec::new());

    assert_eq!(snapshot.app_version, "1.2.3");
    assert_eq!(snapshot.os, "Windows 11");
    assert_eq!(snapshot.gpus, ["NVIDIA GeForce RTX 3060 (12288 MiB)"]);
    assert_eq!(snapshot.driver_version.as_deref(), Some("591.86"));
    assert_eq!(snapshot.driver_cuda.as_deref(), Some("13.1"));
    assert!(snapshot.environment.is_none());
    assert!(snapshot.manifest.is_none());
    assert!(snapshot.comfyui.revision.is_none());
    assert_eq!(snapshot.comfyui.status.as_deref(), Some("stopped"));
    for fragment in [
        "first run",
        "install.json",
        "ComfyUI checkout",
        "application log",
    ] {
        assert!(warned(&snapshot, fragment), "{fragment}: {snapshot:?}");
    }
    let text = snapshot.render();
    assert!(text.contains("== Environment ==\nunavailable\n"), "{text}");
    assert!(
        text.contains("== Install manifest ==\nunavailable\n"),
        "{text}"
    );
    assert!(text.contains("Revision: unavailable"), "{text}");
    assert!(text.contains("uv 0.13.1 (pinned by this app, not probed)"));
    assert!(text.contains("hf 2.2.0 (pinned by this app, not probed)"));

    // An environment exists, but its checkout is gone and the logs are unreadable.
    let machine = Machine::new();
    seed_environment_and_manifest(&machine);
    let snapshot = machine.collect(Err(DiagnosticsError::Unavailable), Vec::new());
    assert!(snapshot.environment.is_some());
    assert!(snapshot.manifest.is_some());
    assert!(snapshot.comfyui.revision.is_none());
    assert!(warned(&snapshot, "ComfyUI checkout"));
    assert!(warned(&snapshot, "application log"));
    assert!(!warned(&snapshot, "first run"));
    // A legacy environment has no stored component outcomes: said, not invented.
    assert!(warned(&snapshot, "Accelerator outcomes are not recorded"));
    assert!(snapshot.environment.unwrap().accelerators.is_empty());
}

#[test]
fn complete_installation_fills_every_section() {
    let machine = Machine::new();
    seed_environment_and_manifest(&machine);
    let result =
        |id: &str, selected, status, importable, message: Option<&str>| AcceleratorResult {
            id: id.into(),
            selected,
            status,
            importable,
            message: message.map(Into::into),
        };
    machine.env.components.lock().unwrap().insert(
        "selected".into(),
        InstalledComponents {
            manager: Some(ManagerResult {
                selected: false,
                status: ComponentStatus::Skipped,
                source_commit: None,
                message: None,
            }),
            accelerators: vec![
                result(
                    "sageattention",
                    true,
                    ComponentStatus::Installed,
                    Some(false),
                    Some("sageattention: DLL import failed"),
                ),
                result(
                    "flashattention-3",
                    true,
                    ComponentStatus::Installed,
                    None,
                    Some("Import untested: module unverified"),
                ),
                result(
                    "onnxruntime-gpu",
                    false,
                    ComponentStatus::Skipped,
                    None,
                    None,
                ),
                result(
                    "llama-cpp-python",
                    true,
                    ComponentStatus::Failed,
                    None,
                    Some("pinned SHA-256 mismatch"),
                ),
            ],
        },
    );
    *machine.revision.lock().unwrap() = Ok(Revision {
        commit: "b".repeat(40),
        tag: Some("v1.2".into()),
        branch: None,
    });
    let snapshot = machine.collect(Ok(vec!["app line".into()]), vec!["runtime line".into()]);

    let environment = snapshot.environment.as_ref().unwrap();
    assert_eq!(environment.id, "selected");
    assert_eq!(environment.ownership, "app_owned");
    assert_eq!(environment.state, "verified");
    assert_eq!(environment.python, "3.13.16");
    assert_eq!(environment.profile_id, "win-py313-torch2141-cu130");
    assert_eq!(environment.torch.as_deref(), Some("2.14.1+cu130"));
    assert!(
        environment
            .protected_pins
            .contains(&("torch".into(), "2.14.1+cu130".into()))
    );
    let manifest = snapshot.manifest.as_ref().unwrap();
    assert_eq!(manifest.profile_id, "win-py313-torch2141-cu130");
    assert_eq!(manifest.profile_revision, 1);
    assert_eq!(
        snapshot.comfyui.revision,
        Some(format!("{} (tag v1.2)", "b".repeat(40)))
    );
    assert!(snapshot.warnings.is_empty(), "{:?}", snapshot.warnings);
    assert_eq!(
        environment.accelerators,
        [
            "sageattention: selected, installed, import failed - sageattention: DLL import failed",
            "flashattention-3: selected, installed, import untested - Import untested: module unverified",
            "onnxruntime-gpu: not selected, skipped, import untested",
            "llama-cpp-python: selected, failed, import untested - pinned SHA-256 mismatch",
        ]
    );

    let text = snapshot.render();
    assert!(text.contains("  torch==2.14.1+cu130"), "{text}");
    assert!(
        text.contains("Accelerators:\n  sageattention: selected, installed, import failed"),
        "{text}"
    );
    assert!(
        text.contains("  onnxruntime-gpu: not selected, skipped, import untested\n"),
        "{text}"
    );
    assert!(text.contains("Ownership: app_owned"), "{text}");
    assert!(text.contains("Profile: win-py313-torch2141-cu130 revision 1"));
    assert!(text.contains("== Application log (last 200 lines) ==\napp line\n"));
    assert!(text.contains("== ComfyUI runtime log (last 200 lines) ==\nruntime line\n"));
}

#[test]
fn snapshot_keeps_only_the_newest_log_lines_of_each_section() {
    let machine = Machine::new();
    let lines = |prefix: &str| {
        (0..500)
            .map(|n| format!("{prefix}-{n}"))
            .collect::<Vec<_>>()
    };
    let snapshot = machine.collect(Ok(lines("app")), lines("runtime"));

    for (kept, prefix) in [
        (&snapshot.app_log, "app"),
        (&snapshot.runtime_log, "runtime"),
    ] {
        assert_eq!(kept.len(), LOG_TAIL_LINES);
        assert_eq!(kept.first().unwrap(), &format!("{prefix}-300"));
        assert_eq!(kept.last().unwrap(), &format!("{prefix}-499"));
    }
}
