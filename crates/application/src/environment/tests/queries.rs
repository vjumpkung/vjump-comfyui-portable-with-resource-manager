use std::path::PathBuf;

use domain::environment::{
    Eligibility, EnvId, EnvState, NewEnvironment, Ownership, PinSet, PythonEnvironment,
    PythonSourceKind, PythonVersion,
};

use super::{
    fakes::*,
    setup::{execute, register, request},
};
use crate::{comfy_runtime::RuntimeBlock, environment::*, test_support::block_on};

fn status(world: &std::sync::Arc<World>) -> SetupStatus {
    block_on(GetSetupStatus::new(world.deps()).execute()).unwrap()
}

fn manifest_for(world: &std::sync::Arc<World>, env_id: &str) -> InstallManifest {
    InstallManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        install_root: PathBuf::from(ROOT),
        generation: 1,
        env_id: EnvId::new(env_id).unwrap(),
        env_path: PathBuf::from(ROOT).join("envs").join(env_id),
        ownership: Ownership::AppOwned,
        python_version: PythonVersion::new(3, 13, 16),
        profile_id: "win-py313-torch2141-cu130".into(),
        profile_revision: 1,
        comfyui_reference: Some("v0.40.0".into()),
        comfyui_commit: Some("abc".into()),
        run_id: RunId::new("run-1").unwrap(),
        activated_at: world.deps().clock.now(),
        components: Default::default(),
    }
}

#[test]
fn status_follows_the_install_lifecycle() {
    let world = World::new();
    assert_eq!(status(&world).phase, SetupPhase::NeedsSetup);

    let run_id = register(&world, request()).unwrap();
    let snapshot = status(&world);
    assert_eq!(snapshot.phase, SetupPhase::RunInProgress);
    assert_eq!(snapshot.run.as_ref().unwrap().id, run_id);

    world.fail("seed", ProvisionError::Failed("boom".into()));
    execute(&world, &run_id);
    let snapshot = status(&world);
    assert_eq!(snapshot.phase, SetupPhase::RunFailed);
    assert_eq!(snapshot.run.unwrap().failure.unwrap().step, StepId::Seed);

    block_on(StartSetup::new(world.deps()).execute(SetupAction::Retry {
        run_id: run_id.clone(),
        manager_consent: None,
        curated_decisions: Vec::new(),
    }))
    .unwrap();
    execute(&world, &run_id);
    let snapshot = status(&world);
    assert_eq!(snapshot.phase, SetupPhase::Ready);
    assert_eq!(snapshot.active_env.unwrap().state, EnvState::Verified);
}

#[test]
fn status_reports_recovery_instead_of_guessing() {
    let world = World::new();
    *world.manifest.lock().unwrap() = ManifestState::Corrupt;
    assert_eq!(
        status(&world).phase,
        SetupPhase::Recovery(RecoveryReason::ManifestCorrupt)
    );
    *world.manifest.lock().unwrap() = ManifestState::Newer;
    assert_eq!(
        status(&world).phase,
        SetupPhase::Recovery(RecoveryReason::ManifestNewer)
    );

    let world = World::new();
    world.seed_active_env("env-a", PythonVersion::new(3, 13, 16));
    assert_eq!(
        status(&world).phase,
        SetupPhase::Recovery(RecoveryReason::ManifestMissing)
    );
    *world.manifest.lock().unwrap() = ManifestState::Valid(manifest_for(&world, "env-b"));
    assert_eq!(
        status(&world).phase,
        SetupPhase::Recovery(RecoveryReason::ManifestMismatch)
    );
    *world.manifest.lock().unwrap() = ManifestState::Valid(manifest_for(&world, "env-a"));
    assert_eq!(status(&world).phase, SetupPhase::Ready);

    let mut env = world.env("env-a");
    env.mark_repair_required("t").unwrap();
    world.envs.lock().unwrap().insert("env-a".into(), env);
    assert_eq!(status(&world).phase, SetupPhase::RepairRequired);
}

#[test]
fn status_carries_the_wizard_draft() {
    let world = World::new();
    *world.draft.lock().unwrap() = Some("{\"step\":2}".into());
    assert_eq!(status(&world).draft.as_deref(), Some("{\"step\":2}"));
}

#[test]
fn profiles_are_listed_for_this_os_with_eligibility() {
    let world = World::new();
    let offers = block_on(ListProfiles::new(world.deps()).execute(&PathBuf::from(ROOT), None));
    assert_eq!(offers.len(), 2);
    assert!(
        offers
            .iter()
            .all(|offer| offer.eligibility == Eligibility::Eligible)
    );

    world.hardware.lock().unwrap().gpus = domain::environment::GpuProbe::NoNvidiaGpu;
    let offers = block_on(ListProfiles::new(world.deps()).execute(&PathBuf::from(ROOT), None));
    assert!(
        offers
            .iter()
            .all(|offer| matches!(offer.eligibility, Eligibility::Disabled(_)))
    );

    world.hardware.lock().unwrap().os = domain::environment::OsKind::Linux;
    let offers = block_on(ListProfiles::new(world.deps()).execute(&PathBuf::from(ROOT), None));
    assert!(
        offers.is_empty(),
        "Windows profiles are not offered on Linux"
    );
}

fn environment(id: &str, ownership: Ownership) -> PythonEnvironment {
    let mut env = PythonEnvironment::provisioning(NewEnvironment {
        id: EnvId::new(id).unwrap(),
        path: PathBuf::from(ROOT).join("envs").join(id),
        ownership,
        source: PythonSourceKind::Managed,
        base_interpreter: None,
        python_version: PythonVersion::new(3, 13, 16),
        profile_id: "p".into(),
        profile_revision: 1,
        authorized_location: PathBuf::from(ROOT).join("envs").join(id),
        now: "t".into(),
    });
    env.mark_failed("t").unwrap();
    let _ = PinSet::new();
    env
}

#[test]
fn only_inactive_app_owned_environments_are_deleted_under_the_gate() {
    let world = World::new();
    world.seed_active_env("env-active", PythonVersion::new(3, 13, 16));
    for (id, ownership) in [
        ("env-owned", Ownership::AppOwned),
        ("env-adopted", Ownership::Adopted),
    ] {
        world
            .envs
            .lock()
            .unwrap()
            .insert(id.into(), environment(id, ownership));
    }
    let delete =
        |id: &str| block_on(DeleteEnvironment::new(world.deps()).execute(&EnvId::new(id).unwrap()));
    assert!(matches!(
        delete("env-active"),
        Err(EnvironmentAppError::Domain(
            domain::environment::EnvironmentError::ActiveNotDeletable
        ))
    ));
    assert!(matches!(
        delete("env-adopted"),
        Err(EnvironmentAppError::Domain(
            domain::environment::EnvironmentError::AdoptedNotDeletable
        ))
    ));
    assert!(matches!(
        delete("env-missing"),
        Err(EnvironmentAppError::EnvironmentNotFound)
    ));
    assert_eq!(world.count("remove_environment:env-adopted"), 0);

    *world.runtime_block.lock().unwrap() = Some(RuntimeBlock::CannotAssess);
    assert!(matches!(
        delete("env-owned"),
        Err(EnvironmentAppError::Runtime(RuntimeBlock::CannotAssess))
    ));
    *world.runtime_block.lock().unwrap() = None;
    delete("env-owned").unwrap();
    assert_eq!(world.count("remove_environment:env-owned"), 1);
    assert!(!world.envs.lock().unwrap().contains_key("env-owned"));
}

#[test]
fn an_environment_targeted_by_an_unfinished_run_is_not_deleted() {
    let world = World::new();
    let run_id = register(&world, request()).unwrap();
    let target = world.run(&run_id).target_env;
    world.envs.lock().unwrap().insert(
        target.as_str().into(),
        environment(target.as_str(), Ownership::AppOwned),
    );
    world
        .runs
        .lock()
        .unwrap()
        .get_mut(run_id.as_str())
        .unwrap()
        .status = RunStatus::Failed;
    assert!(matches!(
        block_on(DeleteEnvironment::new(world.deps()).execute(&target)),
        Err(EnvironmentAppError::EnvironmentInUse)
    ));
}

#[test]
fn the_install_root_can_only_change_before_setup() {
    let world = World::new();
    let select = |world: &std::sync::Arc<World>| {
        block_on(
            SelectInstallRoot::new(world.deps()).execute("/elsewhere".into(), Some("draft".into())),
        )
    };
    select(&world).unwrap();
    assert_eq!(world.count("switch_root:/elsewhere"), 1);
    assert_eq!(world.draft.lock().unwrap().as_deref(), Some("draft"));

    let busy = World::new();
    register(&busy, request()).unwrap();
    assert!(matches!(
        select(&busy),
        Err(EnvironmentAppError::SetupInProgress)
    ));

    let installed = World::new();
    installed.seed_active_env("env-a", PythonVersion::new(3, 13, 16));
    assert!(matches!(
        select(&installed),
        Err(EnvironmentAppError::RootChangeNotAllowed)
    ));
    assert_eq!(installed.count("switch_root:/elsewhere"), 0);
}

#[test]
fn environment_info_and_packages_describe_the_selected_environment() {
    let world = World::new();
    world.seed_active_env("env-a", PythonVersion::new(3, 13, 16));
    *world.manifest.lock().unwrap() = ManifestState::Valid(manifest_for(&world, "env-a"));
    *world.checkout.lock().unwrap() = Checkout::Repo {
        commit: "abc".into(),
    };
    let info = block_on(GetEnvironmentInfo::new(world.deps()).execute()).unwrap();
    assert_eq!(info.torch_version.as_deref(), Some("2.14.1+cu130"));
    assert_eq!(info.triton_version.as_deref(), Some("3.8.0.post29"));
    assert_eq!(info.cuda_variant.as_deref(), Some("cu130"));
    assert_eq!(info.comfyui_reference.as_deref(), Some("v0.40.0"));
    assert_eq!(info.comfyui_commit.as_deref(), Some("abc"));
    assert_eq!(info.env.ownership, Ownership::AppOwned);

    let packages = block_on(ListPackages::new(world.deps()).execute()).unwrap();
    let names: Vec<_> = packages
        .iter()
        .map(|package| package.name.as_str())
        .collect();
    assert_eq!(names, ["numpy", "torch", "torchvision", "triton-windows"]);

    let empty = World::new();
    assert!(matches!(
        block_on(GetEnvironmentInfo::new(empty.deps()).execute()),
        Err(EnvironmentAppError::NoActiveEnvironment)
    ));
}

#[test]
fn environment_info_reports_recorded_components_and_recovers_legacy_selections() {
    let world = World::new();
    world.profiles.lock().unwrap()[0].accelerators = vec![accelerator("sageattention")];
    let mut req = request();
    req.accelerators = vec!["sageattention".into()];
    let id = register(&world, req).unwrap();
    execute(&world, &id);
    let recorded = world.run(&id).components;
    world.profiles.lock().unwrap()[0].accelerators.clear();
    let info = block_on(GetEnvironmentInfo::new(world.deps()).execute()).unwrap();
    assert_eq!(info.components, recorded);
    world.components.lock().unwrap().clear();
    if let ManifestState::Valid(manifest) = &mut *world.manifest.lock().unwrap() {
        manifest.schema_version = 1;
        manifest.components = Default::default();
    }
    let info = block_on(GetEnvironmentInfo::new(world.deps()).execute()).unwrap();
    assert_eq!(
        info.components, recorded,
        "legacy manifest recovers only its associated run"
    );
}
