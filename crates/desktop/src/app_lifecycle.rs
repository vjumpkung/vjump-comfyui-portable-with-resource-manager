//! Window and exit handling around the ComfyUI runtime. The close policy lives in the
//! application layer; this module only performs the window and exit actions.
//!
//! Keep-running-headless closes every visible window while the Rust core, the supervisor, the
//! Job Object/process-group ownership and the log readers stay alive. Reopening the app
//! (a second launch) recreates the manager window.

use std::{
    future::Future,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use application::{
    comfy_runtime::{
        CloseAction, InventoryState, RuntimeError, RuntimeEvent, RuntimeStatus, RuntimeSupervisor,
        close_action,
    },
    shared::{ExplorerTarget, MaintenanceGate, MaintenanceLease},
};
use domain::comfy_runtime::Lifecycle;
use tauri::{AppHandle, Manager, Runtime, WebviewWindowBuilder};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tauri_plugin_opener::OpenerExt;
use tokio::sync::watch;

use crate::{MANAGER_WINDOW, comfy_window::COMFYUI_WINDOW, ipc::error::IpcError, state::AppState};

/// Window and browser actions behind a runtime-independent interface, so the commands stay
/// non-generic. Installed once the application handle exists.
pub trait ShellActions: Send + Sync {
    fn open_comfy_window(&self, url: tauri::Url) -> Result<(), IpcError>;
    fn open_browser(&self, url: tauri::Url);
    fn perform(&self, action: CloseAction);
    fn request_restart(&self);
    /// Shows a folder, or a folder with one item selected, in the system file manager. Blocks
    /// until the launch returns, so callers run it off the async worker threads.
    fn open_explorer(&self, target: ExplorerTarget) -> Result<(), IpcError>;
}

fn explorer_failed() -> IpcError {
    IpcError::new("explorer_failed", "The file manager could not be opened.")
}

/// Pinned model roots are canonical, so their paths carry the Windows verbatim prefix. The file
/// manager only understands plain drive and UNC paths.
fn without_verbatim_prefix(path: &Path) -> PathBuf {
    let Some(text) = path.to_str() else {
        return path.to_path_buf();
    };
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    }
}

/// Hands the target to the shell on a blocking thread.
pub(crate) async fn open_explorer(
    state: &AppState,
    target: ExplorerTarget,
) -> Result<(), IpcError> {
    let shell = Arc::clone(state.shell()?);
    tauri::async_runtime::spawn_blocking(move || shell.open_explorer(target))
        .await
        .map_err(|_| explorer_failed())?
}

pub struct TauriShell<R: Runtime>(pub AppHandle<R>);

impl<R: Runtime> ShellActions for TauriShell<R> {
    fn open_comfy_window(&self, url: tauri::Url) -> Result<(), IpcError> {
        crate::comfy_window::open_comfyui_window(&self.0, url)
            .map(|_| ())
            .map_err(|_| {
                IpcError::new(
                    "window_unavailable",
                    "The ComfyUI window could not be opened.",
                )
            })
    }

    fn open_browser(&self, url: tauri::Url) {
        crate::comfy_window::open_in_browser(&self.0, url);
    }

    fn perform(&self, action: CloseAction) {
        tauri::async_runtime::spawn(perform(self.0.clone(), action));
    }

    fn request_restart(&self) {
        let app = self.0.clone();
        tauri::async_runtime::spawn(async move {
            // Quiesce only once the restart is certain: a failed rebuild or refused switch keeps
            // the session's workers running. `AppHandle::restart` skips the `RunEvent::Exit`
            // handler that would otherwise stop them.
            let state = app.state::<AppState>();
            state.shutdown_downloads().await;
            state.shutdown_library().await;
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            app.restart();
        });
    }

    fn open_explorer(&self, target: ExplorerTarget) -> Result<(), IpcError> {
        let opener = self.0.opener();
        let result = match target {
            ExplorerTarget::OpenFolder(path) => opener.open_path(
                without_verbatim_prefix(&path).to_string_lossy(),
                None::<&str>,
            ),
            ExplorerTarget::RevealItem(path) => {
                opener.reveal_item_in_dir(without_verbatim_prefix(&path))
            }
        };
        result.map_err(|_| {
            tracing::warn!("the file manager could not be opened");
            explorer_failed()
        })
    }
}

/// Tracks the pending manager close request and whether the app runs without windows.
pub struct CloseCoordinator {
    exit_lease: Mutex<Option<Box<dyn MaintenanceLease>>>,
    pending: Mutex<Option<String>>,
    counter: AtomicU64,
    headless: AtomicBool,
    changes: watch::Sender<Option<String>>,
}

impl Default for CloseCoordinator {
    fn default() -> Self {
        Self {
            exit_lease: Mutex::new(None),
            pending: Mutex::new(None),
            counter: AtomicU64::new(0),
            headless: AtomicBool::new(false),
            changes: watch::channel(None).0,
        }
    }
}

impl CloseCoordinator {
    pub(crate) async fn prepare_recovery(
        &self,
        gate: &dyn MaintenanceGate,
    ) -> Result<CloseAction, RuntimeError> {
        if self
            .exit_lease
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some()
        {
            return Ok(CloseAction::Exit);
        }
        let lease = gate.try_acquire().await.map_err(|_| RuntimeError::Busy)?;
        *self.exit_lease.lock().unwrap_or_else(|p| p.into_inner()) = Some(lease);
        Ok(CloseAction::Exit)
    }
    async fn guard_exit(
        &self,
        gate: &dyn MaintenanceGate,
        supervisor: &dyn RuntimeSupervisor,
        root: &Path,
    ) -> Result<CloseAction, RuntimeError> {
        let lease = gate.try_acquire().await.map_err(|_| RuntimeError::Busy)?;
        // Inventory detects surviving/unowned processes even when the hub says Crashed.
        if !matches!(supervisor.inventory(root).await?, InventoryState::Stopped)
            || !matches!(
                supervisor.status().await.lifecycle,
                Lifecycle::Stopped | Lifecycle::Crashed
            )
        {
            return Err(RuntimeError::NeedsStop);
        }
        *self.exit_lease.lock().unwrap_or_else(|p| p.into_inner()) = Some(lease);
        Ok(CloseAction::Exit)
    }

    async fn prepare(
        &self,
        action: CloseAction,
        gate: &dyn MaintenanceGate,
        supervisor: &dyn RuntimeSupervisor,
        root: &Path,
        stop: impl Future<Output = Result<RuntimeStatus, RuntimeError>>,
    ) -> Result<CloseAction, RuntimeError> {
        if self
            .exit_lease
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some()
        {
            return Ok(CloseAction::Exit);
        }
        if matches!(
            supervisor.status().await.lifecycle,
            Lifecycle::Stopped | Lifecycle::Crashed
        ) {
            return self.guard_exit(gate, supervisor, root).await;
        }
        match action {
            CloseAction::StopThenExit => {
                match stop.await {
                    Ok(_) => {}
                    Err(RuntimeError::NodeRescanFailed) => {
                        tracing::warn!(
                            "ComfyUI stopped, but custom nodes could not be rescanned before exit"
                        );
                    }
                    Err(error) => return Err(error),
                }
                self.guard_exit(gate, supervisor, root).await
            }
            CloseAction::KeepRunningHeadless => {
                if matches!(
                    supervisor.inventory(root).await?,
                    InventoryState::OwnedRunning
                ) && !matches!(
                    supervisor.status().await.lifecycle,
                    Lifecycle::Stopped | Lifecycle::Crashed
                ) {
                    Ok(action)
                } else {
                    // A stale keep-running response must use the same guarded stop check.
                    self.guard_exit(gate, supervisor, root).await
                }
            }
            CloseAction::Exit => self.guard_exit(gate, supervisor, root).await,
            CloseAction::Ask => Ok(action),
        }
    }

    pub fn pending(&self) -> Option<String> {
        self.pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn watch(&self) -> watch::Receiver<Option<String>> {
        self.changes.subscribe()
    }

    pub fn headless(&self) -> bool {
        self.headless.load(Ordering::SeqCst)
    }

    pub fn set_headless(&self, value: bool) {
        self.headless.store(value, Ordering::SeqCst);
    }

    /// The pending request, or a new one. Subscribers are notified of a new request only.
    pub fn open_request(&self) -> String {
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(existing) = pending.as_ref() {
            return existing.clone();
        }
        let id = format!("close-{}", self.counter.fetch_add(1, Ordering::SeqCst) + 1);
        *pending = Some(id.clone());
        self.changes.send_replace(Some(id.clone()));
        id
    }

    /// Resolves the pending request. False when `id` is not the pending one.
    pub fn resolve(&self, id: &str) -> bool {
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if pending.as_deref() != Some(id) {
            return false;
        }
        *pending = None;
        self.changes.send_replace(None);
        true
    }
}

const MAINTENANCE_MESSAGE: &str =
    "Maintenance is in progress. Wait for it to finish or cancel it before closing.";

fn lifecycle_of<R: Runtime>(app: &AppHandle<R>) -> Option<Lifecycle> {
    let state = app.state::<AppState>();
    state
        .services()
        .ok()
        .map(|services| services.comfy.hub.status().lifecycle)
}

/// True while the core must outlive its last window.
pub fn keep_core_alive<R: Runtime>(app: &AppHandle<R>) -> bool {
    let Some(state) = app.try_state::<AppState>() else {
        return false;
    };
    state.close.headless()
        && !matches!(
            lifecycle_of(app),
            None | Some(Lifecycle::Stopped | Lifecycle::Crashed)
        )
}

/// Handles `WindowEvent::CloseRequested` of the manager. The close is always deferred; the
/// decision runs outside the synchronous event handler.
pub fn on_manager_close_requested<R: Runtime>(app: &AppHandle<R>, api: &tauri::CloseRequestApi) {
    api.prevent_close();
    if app.try_state::<AppState>().is_none() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let action = decide(&app).await;
        perform(app, action).await;
    });
}

async fn decide<R: Runtime>(app: &AppHandle<R>) -> CloseAction {
    let state = app.state::<AppState>();
    let Ok(services) = state.services() else {
        return CloseAction::Exit;
    };
    let lifecycle = services.comfy.hub.status().lifecycle;
    let behavior = match services.get_settings.execute().await {
        Ok(settings) => settings.close_behavior,
        Err(error) => {
            tracing::warn!(%error, "close behavior unavailable; asking");
            domain::settings::CloseBehavior::Ask
        }
    };
    close_action(behavior, lifecycle)
}

pub async fn perform<R: Runtime>(app: AppHandle<R>, action: CloseAction) {
    let state = app.state::<AppState>();
    let prepared = if let Ok(services) = state.services() {
        state
            .close
            .prepare(
                action,
                services.gate.as_ref(),
                services.comfy.supervisor.as_ref(),
                &services.root.root,
                services.comfy.stop.execute(),
            )
            .await
    } else {
        state.close.prepare_recovery(state.gate.as_ref()).await
    };
    let action = match prepared {
        Ok(action) => action,
        Err(error) => {
            tracing::warn!(%error, "close refused; staying open");
            restore_manager(&app);
            if matches!(error, RuntimeError::Busy) {
                app.dialog()
                    .message(MAINTENANCE_MESSAGE)
                    .kind(MessageDialogKind::Info)
                    .show(|_| {});
            }
            return;
        }
    };
    match action {
        CloseAction::Exit => app.exit(0),
        CloseAction::StopThenExit => unreachable!("prepare resolves stop before exit"),
        CloseAction::KeepRunningHeadless => {
            state.close.set_headless(true);
            for label in [COMFYUI_WINDOW, MANAGER_WINDOW] {
                if let Some(window) = app.get_webview_window(label) {
                    let _ = window.destroy();
                }
            }
        }
        CloseAction::Ask => {
            state.close.open_request();
            if let Some(window) = app.get_webview_window(MANAGER_WINDOW) {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
    }
}

/// Second launch: restores the manager window, which is absent in background mode.
pub fn restore_manager<R: Runtime>(app: &AppHandle<R>) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let close = state.close.clone();
    if let Some(window) = app.get_webview_window(MANAGER_WINDOW) {
        close.set_headless(false);
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    let app = app.clone();
    // Windows are created outside synchronous event handlers to avoid a deadlock.
    tauri::async_runtime::spawn(async move {
        let Some(config) = app
            .config()
            .app
            .windows
            .iter()
            .find(|window| window.label == MANAGER_WINDOW)
            .cloned()
        else {
            return;
        };
        match WebviewWindowBuilder::from_config(&app, &config).and_then(|builder| builder.build()) {
            Ok(window) => {
                close.set_headless(false);
                let _ = window.set_focus();
            }
            Err(error) => tracing::error!(%error, "the manager window could not be recreated"),
        }
    });
}

/// Keeps the ComfyUI window consistent with the runtime and ends background mode when the
/// runtime stops. The subscription is retaken when the hub reports a gap.
pub fn spawn_runtime_watch<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        loop {
            let subscription = {
                let state = app.state::<AppState>();
                let Ok(services) = state.services() else {
                    return;
                };
                services.comfy.events.subscribe(None)
            };
            let mut receiver = subscription.receiver;
            while let Some(event) = receiver.next().await {
                let status = match event {
                    RuntimeEvent::Snapshot { status, .. } | RuntimeEvent::State { status, .. } => {
                        status
                    }
                    RuntimeEvent::Log(_) => continue,
                    RuntimeEvent::Gap { .. } => break,
                };
                reconcile(&app, &status);
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    });
}

fn reconcile<R: Runtime>(app: &AppHandle<R>, status: &application::comfy_runtime::RuntimeStatus) {
    match status.origin().and_then(|origin| origin.parse().ok()) {
        Some(url) => {
            if let Some(window) = app.get_webview_window(COMFYUI_WINDOW) {
                // Only an origin change navigates; repeated Running updates must not reload it.
                let same = window
                    .url()
                    .is_ok_and(|current| current.origin() == tauri::Url::origin(&url));
                if !same {
                    let _ = crate::comfy_window::open_comfyui_window(app, url);
                }
            }
        }
        None => {
            if let Some(window) = app.get_webview_window(COMFYUI_WINDOW) {
                let _ = window.destroy();
            }
            if matches!(status.lifecycle, Lifecycle::Stopped | Lifecycle::Crashed)
                && app.state::<AppState>().close.headless()
            {
                let app = app.clone();
                tauri::async_runtime::spawn(perform(app, CloseAction::Exit));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use application::{
        comfy_runtime::{RuntimeBlock, RuntimeMaintenance, RuntimeSession},
        shared::GateError,
    };
    use infrastructure::shared::gate::TokioMaintenanceGate;
    use std::sync::{Arc, atomic::AtomicUsize};

    #[test]
    fn restore_manager_before_setup_is_safe_with_or_without_a_window() {
        let app = tauri::test::mock_app();
        assert!(app.try_state::<AppState>().is_none());
        restore_manager(app.handle());
        assert!(app.webview_windows().is_empty());

        WebviewWindowBuilder::new(&app, MANAGER_WINDOW, Default::default())
            .build()
            .unwrap();
        restore_manager(app.handle());
        assert_eq!(app.webview_windows().len(), 1);
        assert!(app.try_state::<AppState>().is_none());
    }

    #[test]
    fn keep_core_alive_before_setup_is_false() {
        let app = tauri::test::mock_app();
        assert!(app.try_state::<AppState>().is_none());
        assert!(!keep_core_alive(app.handle()));
    }

    struct Supervisor {
        gate: TokioMaintenanceGate,
        lifecycle: Mutex<Lifecycle>,
        inventory: Mutex<Result<InventoryState, RuntimeError>>,
        inspections: AtomicUsize,
        require_gate: AtomicBool,
    }

    impl Supervisor {
        fn new(gate: &TokioMaintenanceGate, lifecycle: Lifecycle) -> Self {
            Self {
                gate: gate.clone(),
                lifecycle: Mutex::new(lifecycle),
                inventory: Mutex::new(Ok(InventoryState::Stopped)),
                inspections: AtomicUsize::new(0),
                require_gate: AtomicBool::new(true),
            }
        }
        async fn stopped(
            &self,
            result: Result<(), RuntimeError>,
        ) -> Result<RuntimeStatus, RuntimeError> {
            *self.lifecycle.lock().unwrap() = Lifecycle::Stopped;
            result?;
            Ok(self.status().await)
        }
    }

    #[async_trait::async_trait]
    impl RuntimeMaintenance for Supervisor {
        async fn ensure_stopped(&self, _: &Path) -> Result<(), RuntimeBlock> {
            unreachable!()
        }
    }

    #[async_trait::async_trait]
    impl RuntimeSupervisor for Supervisor {
        async fn status(&self) -> RuntimeStatus {
            RuntimeStatus {
                lifecycle: *self.lifecycle.lock().unwrap(),
                ..Default::default()
            }
        }
        async fn inventory(&self, _: &Path) -> Result<InventoryState, RuntimeError> {
            if self.require_gate.load(Ordering::SeqCst) {
                assert_eq!(
                    self.gate.try_acquire().await.err(),
                    Some(GateError::Busy),
                    "exit inspection must hold the shared gate"
                );
            }
            self.inspections.fetch_add(1, Ordering::SeqCst);
            self.inventory.lock().unwrap().clone()
        }
        async fn start(
            &self,
            _: RuntimeSession,
            _: Vec<String>,
            _: Box<dyn MaintenanceLease>,
        ) -> Result<RuntimeStatus, RuntimeError> {
            unreachable!()
        }
        async fn stop(&self) -> Result<RuntimeStatus, RuntimeError> {
            unreachable!()
        }
    }

    #[tokio::test]
    async fn stop_then_exit_accepts_only_rescan_failure_after_verified_stop() {
        for result in [
            Ok(()),
            Err(RuntimeError::NodeRescanFailed),
            Err(RuntimeError::Failed("termination failed".into())),
            Err(RuntimeError::RecoveryRequired("ownership unknown".into())),
            Err(RuntimeError::Storage),
            Err(RuntimeError::Busy),
        ] {
            let gate = TokioMaintenanceGate::new();
            let supervisor = Supervisor::new(&gate, Lifecycle::Running);
            let close = CloseCoordinator::default();
            let allowed = matches!(result, Ok(()) | Err(RuntimeError::NodeRescanFailed));
            let action = close
                .prepare(
                    CloseAction::StopThenExit,
                    &gate,
                    &supervisor,
                    Path::new("root"),
                    supervisor.stopped(result),
                )
                .await;
            assert_eq!(action.is_ok(), allowed);
            assert_eq!(
                supervisor.inspections.load(Ordering::SeqCst),
                usize::from(allowed)
            );
            assert_eq!(close.exit_lease.lock().unwrap().is_some(), allowed);
        }
        for inventory in [
            Ok(InventoryState::OwnedRunning),
            Ok(InventoryState::Unknown("ownership unknown".into())),
            Ok(InventoryState::VerifiedSurvivor(
                application::comfy_runtime::ProcessIdentity {
                    pid: 1,
                    created: "t".into(),
                    scope: "scope".into(),
                },
            )),
            Err(RuntimeError::Storage),
        ] {
            let gate = TokioMaintenanceGate::new();
            let supervisor = Supervisor::new(&gate, Lifecycle::Running);
            *supervisor.inventory.lock().unwrap() = inventory;
            let close = CloseCoordinator::default();
            assert!(
                close
                    .prepare(
                        CloseAction::StopThenExit,
                        &gate,
                        &supervisor,
                        Path::new("root"),
                        supervisor.stopped(Err(RuntimeError::NodeRescanFailed))
                    )
                    .await
                    .is_err()
            );
            assert!(close.exit_lease.lock().unwrap().is_none());
            assert!(gate.try_acquire().await.is_ok());
        }
        // A successful Stop response alone cannot authorize exit while still running.
        let gate = TokioMaintenanceGate::new();
        let supervisor = Supervisor::new(&gate, Lifecycle::Running);
        assert!(
            CloseCoordinator::default()
                .prepare(
                    CloseAction::StopThenExit,
                    &gate,
                    &supervisor,
                    Path::new("root"),
                    async { Ok(RuntimeStatus::default()) }
                )
                .await
                .is_err()
        );
        // Maintenance can win the interval between Stop/rescan and the final check.
        let close = CloseCoordinator::default();
        let maintenance = Mutex::new(None);
        let action = close
            .prepare(
                CloseAction::StopThenExit,
                &gate,
                &supervisor,
                Path::new("root"),
                async {
                    let lease = gate.acquire().await.unwrap();
                    *maintenance.lock().unwrap() = Some(lease);
                    supervisor
                        .stopped(Err(RuntimeError::NodeRescanFailed))
                        .await
                },
            )
            .await;
        assert_eq!(action, Err(RuntimeError::Busy));
        assert!(close.exit_lease.lock().unwrap().is_none());
        assert_eq!(gate.try_acquire().await.err(), Some(GateError::Busy));
    }

    #[tokio::test]
    async fn close_during_maintenance_keeps_the_manager_open() {
        for work in [
            "node preparation",
            "hook",
            "environment work",
            "retained cleanup failure",
        ] {
            let gate = TokioMaintenanceGate::new();
            let lease = gate.acquire().await.unwrap();
            let retained = lease.retained();
            let lease = if work == "retained cleanup failure" {
                drop(lease);
                None
            } else {
                Some(lease)
            };
            let close = CloseCoordinator::default();
            let supervisor = Supervisor::new(&gate, Lifecycle::Stopped);
            let action = close
                .prepare(
                    CloseAction::Exit,
                    &gate,
                    &supervisor,
                    Path::new("root"),
                    async { panic!("close must not stop/cancel {work}") },
                )
                .await;
            assert!(matches!(action, Err(RuntimeError::Busy)), "{work}");
            assert_eq!(
                MAINTENANCE_MESSAGE,
                "Maintenance is in progress. Wait for it to finish or cancel it before closing."
            );
            assert!(!close.headless(), "no window destruction for {work}");
            assert!(
                close.exit_lease.lock().unwrap().is_none(),
                "no exit for {work}"
            );
            assert_eq!(supervisor.inspections.load(Ordering::SeqCst), 0);
            drop(retained);
            drop(lease);
            assert!(
                gate.try_acquire().await.is_ok(),
                "refused close must not queue an exit"
            );
            assert!(close.exit_lease.lock().unwrap().is_none());
        }
    }

    #[tokio::test]
    async fn exit_lease_prevents_new_maintenance_before_process_exit() {
        let gate = TokioMaintenanceGate::new();
        let supervisor = Supervisor::new(&gate, Lifecycle::Stopped);
        let close = Arc::new(CloseCoordinator::default());
        assert_eq!(
            close
                .prepare(
                    CloseAction::Exit,
                    &gate,
                    &supervisor,
                    Path::new("root"),
                    async { unreachable!() }
                )
                .await
                .unwrap(),
            CloseAction::Exit
        );
        assert_eq!(gate.try_acquire().await.err(), Some(GateError::Busy));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(30), gate.acquire())
                .await
                .is_err()
        );
        // Repeated exit events keep the original lease through actual process teardown.
        assert_eq!(
            close
                .prepare(
                    CloseAction::Exit,
                    &gate,
                    &supervisor,
                    Path::new("root"),
                    async { unreachable!() }
                )
                .await
                .unwrap(),
            CloseAction::Exit
        );
        assert_eq!(gate.try_acquire().await.err(), Some(GateError::Busy));
        drop(close);
        assert!(gate.try_acquire().await.is_ok());
    }

    #[tokio::test]
    async fn stale_close_choice_cannot_bypass_maintenance() {
        for action in [CloseAction::StopThenExit, CloseAction::KeepRunningHeadless] {
            let gate = TokioMaintenanceGate::new();
            let supervisor = Supervisor::new(&gate, Lifecycle::Running);
            let close = CloseCoordinator::default();
            let request = close.open_request();
            // The runtime stopped and maintenance began while the dialog was open.
            *supervisor.lifecycle.lock().unwrap() = Lifecycle::Stopped;
            let lease = gate.acquire().await.unwrap();
            assert!(close.resolve(&request));
            assert!(matches!(
                close
                    .prepare(action, &gate, &supervisor, Path::new("root"), async {
                        panic!("stale response must not stop maintenance")
                    })
                    .await,
                Err(RuntimeError::Busy)
            ));
            assert!(!close.headless());
            assert!(close.exit_lease.lock().unwrap().is_none());
            drop(lease);
        }
        // The headless-stop exit uses the same guard and restores the manager on refusal.
        let gate = TokioMaintenanceGate::new();
        let supervisor = Supervisor::new(&gate, Lifecycle::Stopped);
        let close = CloseCoordinator::default();
        close.set_headless(true);
        let _lease = gate.acquire().await.unwrap();
        assert!(matches!(
            close
                .prepare(
                    CloseAction::Exit,
                    &gate,
                    &supervisor,
                    Path::new("root"),
                    async { unreachable!() }
                )
                .await,
            Err(RuntimeError::Busy)
        ));
        assert!(close.exit_lease.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn a_supervised_running_runtime_keeps_the_headless_behavior() {
        let gate = TokioMaintenanceGate::new();
        let supervisor = Supervisor::new(&gate, Lifecycle::Running);
        *supervisor.inventory.lock().unwrap() = Ok(InventoryState::OwnedRunning);
        supervisor.require_gate.store(false, Ordering::SeqCst);
        assert_eq!(
            CloseCoordinator::default()
                .prepare(
                    CloseAction::KeepRunningHeadless,
                    &gate,
                    &supervisor,
                    Path::new("root"),
                    async { unreachable!() }
                )
                .await
                .unwrap(),
            CloseAction::KeepRunningHeadless
        );
    }

    #[tokio::test]
    async fn recovery_close_holds_the_same_gate_through_exit() {
        let gate = TokioMaintenanceGate::new();
        let close = CloseCoordinator::default();
        let lease = gate.acquire().await.unwrap();
        assert_eq!(close.prepare_recovery(&gate).await, Err(RuntimeError::Busy));
        assert!(close.exit_lease.lock().unwrap().is_none());
        drop(lease);
        assert_eq!(close.prepare_recovery(&gate).await, Ok(CloseAction::Exit));
        assert_eq!(gate.try_acquire().await.err(), Some(GateError::Busy));
        assert_eq!(close.prepare_recovery(&gate).await, Ok(CloseAction::Exit));
    }

    #[test]
    fn the_verbatim_prefix_is_stripped_before_the_file_manager_opens_a_path() {
        for (path, expected) in [
            (
                r"\\?\C:\Users\me\ComfyUI\output",
                r"C:\Users\me\ComfyUI\output",
            ),
            (r"\\?\UNC\server\share\ComfyUI", r"\\server\share\ComfyUI"),
            (r"C:\Users\me\ComfyUI", r"C:\Users\me\ComfyUI"),
            (r"\\server\share\ComfyUI", r"\\server\share\ComfyUI"),
            ("/home/me/ComfyUI/output", "/home/me/ComfyUI/output"),
        ] {
            assert_eq!(
                without_verbatim_prefix(Path::new(path)),
                PathBuf::from(expected),
                "{path}"
            );
        }
    }
}
