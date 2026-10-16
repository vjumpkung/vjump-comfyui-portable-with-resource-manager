pub mod app_lifecycle;
pub mod comfy_window;
pub mod ipc;
pub mod picker;
pub mod state;

use std::sync::{Arc, Mutex};

use infrastructure::shared::{
    logging,
    paths::{AppPaths, resolve_root},
};
use tauri::{Manager, RunEvent, WindowEvent};

pub use state::{AppState, RecoveryInfo, Services};

/// The label of the only window that receives the manager capability.
pub const MANAGER_WINDOW: &str = "manager";

pub fn run() {
    let bindings = ipc::bindings::builder::<tauri::Wry>();
    let logging_guard: Arc<Mutex<Option<logging::LoggingGuard>>> = Arc::new(Mutex::new(None));
    let setup_guard = logging_guard.clone();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // A second launch restores the manager, which is absent in background mode.
            app_lifecycle::restore_manager(app);
        }))
        .plugin(tauri_plugin_log::Builder::new().skip_logger().build())
        .plugin(
            tauri_plugin_opener::Builder::new()
                .open_js_links_on_click(false)
                .build(),
        )
        // Registered for the native pickers. The manager calls the app's own pick commands;
        // no capability grants the plugin's commands to any webview.
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(bindings.invoke_handler())
        .on_window_event(|window, event| {
            if window.label() == MANAGER_WINDOW
                && let WindowEvent::CloseRequested { api, .. } = event
            {
                app_lifecycle::on_manager_close_requested(window.app_handle(), api);
            }
        })
        .setup(move |app| {
            let picker = Arc::new(picker::DialogPicker::new(app.handle().clone()));
            // The install root is resolved before anything is opened: debug override, the
            // bootstrap pointer, then the platform default. An unusable pointer never falls
            // back to another root; the app starts in recovery mode instead.
            let state = match resolve_root() {
                Ok(resolved) => {
                    let paths = AppPaths::from_root(&resolved.root);
                    paths.ensure_dirs()?;
                    let guard = logging::init(&paths.logs_dir)?;
                    *setup_guard
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(guard);
                    tracing::info!(
                        source = resolved.source.as_str(),
                        "starting vjumpkung-comfyui-desktop"
                    );
                    match tauri::async_runtime::block_on(AppState::compose(
                        &resolved,
                        picker.clone(),
                    )) {
                        Ok(state) => state,
                        Err(error) => {
                            tracing::error!(%error, "the app could not open its install root");
                            tauri::async_runtime::block_on(AppState::compose_recovery(
                                RecoveryInfo::startup_failed(),
                                picker,
                            ))?
                        }
                    }
                }
                Err(error) => tauri::async_runtime::block_on(AppState::compose_recovery(
                    RecoveryInfo::from_root_error(&error),
                    picker,
                ))?,
            };
            state.install_shell(Arc::new(app_lifecycle::TauriShell(app.handle().clone())));
            app.manage(state);
            app_lifecycle::spawn_runtime_watch(app.handle().clone());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building the application");

    let code = app.run_return(|app, event| {
        // The last window closing must not end a retained background runtime.
        if let RunEvent::Exit = event {
            // Running transfers are paused and cleaned up before the process ends.
            let state = app.state::<AppState>();
            tauri::async_runtime::block_on(state.shutdown_downloads());
        }
        if let RunEvent::ExitRequested {
            code: None, api, ..
        } = event
            && app_lifecycle::keep_core_alive(app)
        {
            api.prevent_exit();
        }
    });
    // Dropping the guard flushes the non-blocking log writer.
    drop(
        logging_guard
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take(),
    );
    std::process::exit(code);
}
