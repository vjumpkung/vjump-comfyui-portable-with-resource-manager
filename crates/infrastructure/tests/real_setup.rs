//! A real first-run setup: real network, real uv, git and Python, real PyTorch. It is ignored
//! by default and meant for a developer machine with an NVIDIA GPU:
//!
//! ```text
//! VJ_REAL_ROOT=D:\vj-test-root cargo test -p infrastructure --test real_setup -- --ignored --nocapture
//! ```
//!
//! The root must not exist yet (or be empty). Set `VJ_REAL_PROFILE` to pick another profile.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};

use application::{
    custom_nodes::{NodeRequirements, NodeService},
    environment::*,
};
use domain::environment::SetupMode;
use infrastructure::{
    custom_nodes::stack::{NodeAdapters, NodeOptions},
    environment::{
        curated::BundledCuratedNodes, hardware::SystemHardware,
        manager_config::FsManagerConfigWriter, manifest::FsManifestStore, packages::UvPackages,
        profiles::BundledProfiles, python::PythonAdapter, runtime_guard::ProcessRuntimeGuard,
        sinks::run_sink, source::GitSource, sqlite::SqliteEnvironmentStore,
        switcher::FsRootSwitcher, tools::Tools, verify::PythonVerifier,
    },
    settings::sqlite::SqliteSettingsRepository,
    shared::{
        clock::{RandomIds, SystemClock},
        gate::TokioMaintenanceGate,
        paths::AppPaths,
        sqlite::Db,
    },
};

struct Printer(Mutex<Instant>);

impl SetupProgressSink for Printer {
    fn emit(&self, event: SetupEvent) {
        let elapsed = self.0.lock().unwrap().elapsed().as_secs();
        match event {
            SetupEvent::Step {
                step,
                status,
                message,
                ..
            } => {
                println!(
                    "[{elapsed:>5}s] STEP {} -> {} {}",
                    step.as_str(),
                    status.as_str(),
                    message.unwrap_or_default()
                );
            }
            SetupEvent::Log { step, line, .. } => {
                println!("[{elapsed:>5}s]   {}: {line}", step.as_str())
            }
            SetupEvent::Failure { failure, .. } => println!("FAILURE {failure:#?}"),
            SetupEvent::RunStatus { status, .. } => {
                println!("[{elapsed:>5}s] RUN {}", status.as_str())
            }
            SetupEvent::Progress { .. } => {}
        }
    }
}

async fn compose(root: &std::path::Path) -> (EnvironmentDeps, Arc<dyn SetupNodes>) {
    let paths = AppPaths::from_root(root);
    paths.ensure_dirs().unwrap();
    let db = Db::open(&paths.db_path).await.unwrap();
    let store = Arc::new(SqliteEnvironmentStore::new(db.clone()));
    let node_adapters = NodeAdapters::open(root, db.clone(), NodeOptions::default());
    let settings = Arc::new(SqliteSettingsRepository::new(db.clone()));
    let current = RootInfo {
        root: root.to_owned(),
        source: RootSource::Default,
    };
    let deps = EnvironmentDeps {
        installation: Arc::new(
            infrastructure::environment::relocation::probe::FsInstallationProbe::new(),
        ),
        relocation: Arc::new(
            infrastructure::environment::relocation::persistence::SqliteRelocationStore::new(db),
        ),
        relocation_runtime: Arc::new(
            infrastructure::environment::relocation::opener::RelocationRuntimeGuard,
        ),
        hardware: Arc::new(SystemHardware::new()),
        catalog: Arc::new(BundledProfiles::load().unwrap()),
        curated: Arc::new(BundledCuratedNodes::load().unwrap()),
        tools: Arc::new(Tools::new()),
        python: Arc::new(PythonAdapter::new()),
        packages: Arc::new(UvPackages::new()),
        wheels: Arc::new(infrastructure::environment::wheels::Wheels::new()),
        verifier: Arc::new(PythonVerifier::new()),
        source: Arc::new(GitSource::new()),
        manager_config: Arc::new(FsManagerConfigWriter),
        node_requirements: Arc::new(NodeRequirements::new(
            node_adapters.store.clone(),
            node_adapters.files.clone(),
        )),
        envs: store.clone(),
        runs: store.clone(),
        activation: store.clone(),
        manifests: Arc::new(FsManifestStore::new()),
        drafts: store,
        root: Arc::new(FsRootSwitcher::new(
            current,
            root.join("..").join("real-bootstrap.json"),
            settings,
        )),
        gate: Arc::new(TokioMaintenanceGate::new()),
        runtime: Arc::new(ProcessRuntimeGuard::new()),
        clock: Arc::new(SystemClock),
        ids: Arc::new(RandomIds::default()),
        active_runs: Arc::new(ActiveRuns::default()),
        plans: Arc::new(PlanStore::default()),
    };
    let nodes = Arc::new(NodeService::new(
        node_adapters.bind(deps.clone(), "real".into()).deps,
    ));
    (deps, nodes)
}

#[tokio::test]
#[ignore = "downloads several gigabytes and needs an NVIDIA GPU"]
async fn real_first_run_setup() {
    let Some(root) = std::env::var_os("VJ_REAL_ROOT").map(PathBuf::from) else {
        println!("set VJ_REAL_ROOT to run the real setup");
        return;
    };
    let profile = std::env::var("VJ_REAL_PROFILE").unwrap_or_else(|_| {
        if cfg!(windows) {
            "win-py313-torch2141-cu130"
        } else {
            "linux-py313-torch2141-cu130"
        }
        .to_owned()
    });
    let (deps, nodes) = compose(&root).await;
    let started = StartSetup::new(deps.clone())
        .execute(SetupAction::New(Box::new(SetupRequest {
            mode: SetupMode::FirstRun,
            install_root: root.clone(),
            python_source: PythonSourceInput::Managed,
            profile_id: profile,
            comfyui_ref: ComfyRef::Latest,
            install_manager: false,
            accelerators: Vec::new(),
            curated_nodes: Vec::new(),
            clean_cache: false,
            confirm_adopted_changes: false,
            allow_source_builds: false,
            third_party_consent: false,
        })))
        .await
        .unwrap();
    let printer: Arc<dyn SetupProgressSink> = Arc::new(Printer(Mutex::new(Instant::now())));
    let (sink, log) = run_sink(
        &AppPaths::from_root(&root).logs_dir,
        started.as_str(),
        printer,
    );
    println!("setup log: {}", log.display());
    let outcome = ExecuteSetup::new(deps.clone(), nodes.clone())
        .execute(&started, sink)
        .await
        .unwrap();
    println!("OUTCOME {outcome:?}");
    assert_eq!(outcome, RunOutcome::Succeeded);

    let info = GetEnvironmentInfo::new(deps.clone())
        .execute()
        .await
        .unwrap();
    println!("{info:#?}");
    let packages = ListPackages::new(deps.clone()).execute().await.unwrap();
    println!("{} packages installed", packages.len());
    let status = GetSetupStatus::new(deps.clone()).execute().await.unwrap();
    assert_eq!(status.phase, SetupPhase::Ready);

    // The pin guard: a harmless package resolves under the constraints and needs no confirmation.
    let plan = PreviewPackageChange::new(deps.clone())
        .execute(PackageOperation::Install(vec!["tqdm".into()]))
        .await
        .unwrap();
    println!("{plan:#?}");
    assert!(!plan.requires_confirmation());
    // A conflicting torch request is previewed with the exact protected change.
    let conflict = PreviewPackageChange::new(deps.clone())
        .execute(PackageOperation::Install(vec!["torch==2.15.0".into()]))
        .await;
    println!("conflicting torch request: {conflict:#?}");

    // Repair of a healthy environment rebuilds nothing.
    let repair = StartSetup::new(deps.clone())
        .execute(SetupAction::New(Box::new(SetupRequest {
            mode: SetupMode::Repair,
            install_root: root.clone(),
            python_source: PythonSourceInput::Managed,
            profile_id: status.active_env.unwrap().profile_id,
            comfyui_ref: ComfyRef::KeepCurrent,
            install_manager: false,
            accelerators: Vec::new(),
            curated_nodes: Vec::new(),
            clean_cache: false,
            confirm_adopted_changes: false,
            allow_source_builds: false,
            third_party_consent: false,
        })))
        .await
        .unwrap();
    let (repair_sink, _) = run_sink(
        &AppPaths::from_root(&root).logs_dir,
        repair.as_str(),
        Arc::new(Printer(Mutex::new(Instant::now()))),
    );
    let outcome = ExecuteSetup::new(deps, nodes)
        .execute(&repair, repair_sink)
        .await
        .unwrap();
    println!("REPAIR OUTCOME {outcome:?}");
    assert_eq!(outcome, RunOutcome::NothingToRepair);
}
