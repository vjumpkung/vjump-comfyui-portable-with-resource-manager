//! Window and exit handling around the ComfyUI runtime. The close policy lives in the
//! application layer; this module only performs the window and exit actions.
//!
//! Keep-running-headless closes every visible window while the Rust core, the supervisor, the
//! Job Object/process-group ownership and the log readers stay alive. Reopening the app
//! (a second launch) recreates the manager window.

use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use application::comfy_runtime::{CloseAction, RuntimeEvent, close_action};
use domain::comfy_runtime::Lifecycle;
use tauri::{AppHandle, Manager, Runtime, WebviewWindowBuilder};
use tokio::sync::watch;

use crate::{MANAGER_WINDOW, comfy_window::COMFYUI_WINDOW, ipc::error::IpcError, state::AppState};

/// Window and browser actions behind a runtime-independent interface, so the commands stay
/// non-generic. Installed once the application handle exists.
pub trait ShellActions: Send + Sync {
    fn open_comfy_window(&self, url: tauri::Url) -> Result<(), IpcError>;
    fn open_browser(&self, url: tauri::Url);
    fn perform(&self, action: CloseAction);
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
}

/// Tracks the pending manager close request and whether the app runs without windows.
pub struct CloseCoordinator {
    pending: Mutex<Option<String>>,
    counter: AtomicU64,
    headless: AtomicBool,
    changes: watch::Sender<Option<String>>,
}

impl Default for CloseCoordinator {
    fn default() -> Self {
        Self {
            pending: Mutex::new(None),
            counter: AtomicU64::new(0),
            headless: AtomicBool::new(false),
            changes: watch::channel(None).0,
        }
    }
}

impl CloseCoordinator {
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

fn lifecycle_of<R: Runtime>(app: &AppHandle<R>) -> Option<Lifecycle> {
    let state = app.state::<AppState>();
    state
        .services()
        .ok()
        .map(|services| services.comfy.hub.status().lifecycle)
}

/// True while the core must outlive its last window.
pub fn keep_core_alive<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.state::<AppState>().close.headless()
        && !matches!(
            lifecycle_of(app),
            None | Some(Lifecycle::Stopped | Lifecycle::Crashed)
        )
}

/// Handles `WindowEvent::CloseRequested` of the manager. The close is always deferred; the
/// decision runs outside the synchronous event handler.
pub fn on_manager_close_requested<R: Runtime>(app: &AppHandle<R>, api: &tauri::CloseRequestApi) {
    api.prevent_close();
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
    match action {
        CloseAction::Exit => app.exit(0),
        CloseAction::StopThenExit => {
            let Ok(services) = state.services() else {
                app.exit(0);
                return;
            };
            // Exit only after termination, output draining, log flushing and node rescan.
            match services.comfy.stop.execute().await {
                Ok(_) => app.exit(0),
                Err(error) => tracing::error!(%error, "ComfyUI could not be stopped; staying open"),
            }
        }
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
    if let Some(window) = app.get_webview_window(MANAGER_WINDOW) {
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
                app.state::<AppState>().close.set_headless(false);
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
                app.exit(0);
            }
        }
    }
}
