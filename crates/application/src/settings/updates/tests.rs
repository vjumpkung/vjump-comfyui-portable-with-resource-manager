use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use async_trait::async_trait;

use super::*;
use crate::{
    comfy_runtime::{RuntimeBlock, RuntimeMaintenance},
    shared::MaintenanceGate,
    test_support::block_on,
};

#[allow(dead_code, clippy::duplicate_mod)]
#[path = "../../environment/tests/fakes.rs"]
mod env_fakes;
use env_fakes::World;

type Events = Arc<Mutex<Vec<String>>>;

/// A runtime proof that records whether the maintenance gate was already held.
struct Runtime {
    gate_busy: Arc<AtomicBool>,
    events: Events,
    block: Mutex<Option<RuntimeBlock>>,
}

#[async_trait]
impl RuntimeMaintenance for Runtime {
    async fn ensure_stopped(&self, _: &Path) -> Result<(), RuntimeBlock> {
        self.events.lock().unwrap().push(format!(
            "runtime:gate_busy={}",
            self.gate_busy.load(Ordering::SeqCst)
        ));
        match *self.block.lock().unwrap() {
            Some(block) => Err(block),
            None => Ok(()),
        }
    }
}

struct Updater {
    configured: bool,
    gate_busy: Arc<AtomicBool>,
    events: Events,
    outcome: Result<(), UpdateError>,
}

#[async_trait]
impl AppUpdater for Updater {
    fn configured(&self) -> bool {
        self.configured
    }

    async fn check(&self) -> Result<Option<UpdateOffer>, UpdateError> {
        unreachable!("installing never checks for a different release")
    }

    async fn install(
        &self,
        update_id: &str,
        sink: Arc<dyn UpdateProgressSink>,
    ) -> Result<(), UpdateError> {
        let busy = || self.gate_busy.load(Ordering::SeqCst);
        self.events
            .lock()
            .unwrap()
            .push(format!("install:{update_id}:gate_busy={}", busy()));
        sink.report(UpdateProgress::Downloading {
            downloaded: 10,
            total: Some(10),
        });
        // Cleanup and the installer run while the caller still owns the gate.
        sink.report(UpdateProgress::Installing);
        self.events
            .lock()
            .unwrap()
            .push(format!("cleanup:gate_busy={}", busy()));
        self.outcome
    }
}

#[derive(Default)]
struct Recorded(Mutex<Vec<UpdateProgress>>);

impl UpdateProgressSink for Recorded {
    fn report(&self, progress: UpdateProgress) {
        self.0.lock().unwrap().push(progress);
    }
}

/// The consumer of the progress channel is gone: every event is dropped.
struct Disconnected;

impl UpdateProgressSink for Disconnected {
    fn report(&self, _: UpdateProgress) {}
}

struct Fixture {
    world: Arc<World>,
    events: Events,
    runtime: Arc<Runtime>,
    workflow: InstallAppUpdate,
}

impl Fixture {
    fn new() -> Self {
        let world = World::new();
        let events = Events::default();
        let runtime = Arc::new(Runtime {
            gate_busy: world.gate.busy.clone(),
            events: events.clone(),
            block: Mutex::default(),
        });
        let mut deps = world.deps();
        deps.runtime = runtime.clone();
        Self {
            workflow: InstallAppUpdate::new(&deps),
            world,
            events,
            runtime,
        }
    }

    fn updater(&self, configured: bool, outcome: Result<(), UpdateError>) -> Updater {
        Updater {
            configured,
            gate_busy: self.world.gate.busy.clone(),
            events: self.events.clone(),
            outcome,
        }
    }

    fn events(&self) -> Vec<String> {
        self.events.lock().unwrap().clone()
    }
}

#[test]
fn update_install_checks_runtime_after_acquiring_gate() {
    let fixture = Fixture::new();
    let sink = Arc::new(Recorded::default());
    let updater = fixture.updater(true, Ok(()));

    block_on(fixture.workflow.execute(&updater, "reviewed", sink.clone())).unwrap();

    assert_eq!(
        fixture.events(),
        [
            "runtime:gate_busy=true",
            "install:reviewed:gate_busy=true",
            "cleanup:gate_busy=true",
        ]
    );
    assert!(!fixture.world.gate.busy.load(Ordering::SeqCst));
    assert_eq!(fixture.world.gate.try_acquires.load(Ordering::SeqCst), 1);
    assert!(
        !sink
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|event| matches!(event, UpdateProgress::Failed(_)))
    );
}

#[test]
fn running_or_unassessable_runtime_refuses_before_the_updater_is_used() {
    for (block, code) in [
        (RuntimeBlock::Running, "comfyui_running"),
        (RuntimeBlock::CannotAssess, "comfyui_state_unknown"),
    ] {
        let fixture = Fixture::new();
        *fixture.runtime.block.lock().unwrap() = Some(block);
        let sink = Arc::new(Recorded::default());

        let error = block_on(fixture.workflow.execute(
            &fixture.updater(true, Ok(())),
            "reviewed",
            sink.clone(),
        ))
        .unwrap_err();

        assert_eq!(error, UpdateError::Runtime(block));
        assert_eq!(error.code(), code);
        assert_eq!(fixture.events(), ["runtime:gate_busy=true"]);
        assert!(!fixture.world.gate.busy.load(Ordering::SeqCst));
        assert_eq!(*sink.0.lock().unwrap(), [UpdateProgress::Failed(error)]);
    }
}

#[test]
fn busy_gate_refuses_without_inspecting_the_runtime() {
    let fixture = Fixture::new();
    let _other = block_on(fixture.world.gate.try_acquire()).unwrap();
    let sink = Arc::new(Recorded::default());

    let error = block_on(fixture.workflow.execute(
        &fixture.updater(true, Ok(())),
        "reviewed",
        sink.clone(),
    ))
    .unwrap_err();

    assert_eq!(error.code(), "maintenance_busy");
    assert!(fixture.events().is_empty());
    assert_eq!(*sink.0.lock().unwrap(), [UpdateProgress::Failed(error)]);
}

#[test]
fn unconfigured_updater_never_takes_the_gate() {
    let fixture = Fixture::new();
    let error = block_on(fixture.workflow.execute(
        &fixture.updater(false, Ok(())),
        "reviewed",
        Arc::new(Disconnected),
    ))
    .unwrap_err();
    assert_eq!(error.code(), "updater_not_configured");
    assert!(fixture.events().is_empty());
    assert_eq!(fixture.world.gate.try_acquires.load(Ordering::SeqCst), 0);
}

#[test]
fn disconnected_update_channel_does_not_release_ownership() {
    let fixture = Fixture::new();
    let updater = fixture.updater(true, Ok(()));

    block_on(
        fixture
            .workflow
            .execute(&updater, "reviewed", Arc::new(Disconnected)),
    )
    .unwrap();

    // Every progress event was dropped, yet the installation ran to completion with the gate
    // owned throughout, and the gate is only free once the workflow has returned.
    assert_eq!(
        fixture.events(),
        [
            "runtime:gate_busy=true",
            "install:reviewed:gate_busy=true",
            "cleanup:gate_busy=true",
        ]
    );
    assert!(!fixture.world.gate.busy.load(Ordering::SeqCst));
}

#[test]
fn updater_failures_are_reported_with_their_stable_codes() {
    for (error, code) in [
        (UpdateError::Stale, "update_stale"),
        (UpdateError::SignatureInvalid, "update_signature_invalid"),
        (UpdateError::InstallFailed, "update_install_failed"),
        (UpdateError::CheckFailed, "update_check_failed"),
    ] {
        let fixture = Fixture::new();
        let sink = Arc::new(Recorded::default());
        let result = block_on(fixture.workflow.execute(
            &fixture.updater(true, Err(error)),
            "reviewed",
            sink.clone(),
        ));
        assert_eq!(result, Err(error));
        assert_eq!(error.code(), code);
        assert_eq!(
            sink.0.lock().unwrap().last(),
            Some(&UpdateProgress::Failed(error))
        );
        assert!(!fixture.world.gate.busy.load(Ordering::SeqCst));
    }
}
