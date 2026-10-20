pub mod app_lifecycle;
pub mod comfy_window;
pub mod diagnostics;
pub mod ipc;
pub mod picker;
pub mod state;
pub mod updates;

#[cfg(all(test, target_os = "windows", target_env = "msvc"))]
#[link(name = "resource", kind = "static")]
unsafe extern "C" {}

use std::sync::{Arc, Mutex, OnceLock};

use infrastructure::shared::{clock::RandomIds, logging, paths::resolve_root};
use tauri::{AppHandle, Manager, RunEvent, WindowEvent};

pub use state::{AppState, RecoveryInfo, Services};

/// The label of the only window that receives the manager capability.
pub const MANAGER_WINDOW: &str = "manager";

pub(crate) const WORKER_STACK_BYTES: usize = 8 * 1024 * 1024;

pub(crate) fn async_runtime_builder() -> tokio::runtime::Builder {
    let mut builder = tokio::runtime::Builder::new_multi_thread();
    builder.enable_all().thread_stack_size(WORKER_STACK_BYTES);
    builder
}

fn install_async_runtime() {
    // Never dropped: tauri::async_runtime::set takes only a Handle.
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    let runtime = RUNTIME.get_or_init(|| {
        async_runtime_builder()
            .build()
            .expect("the async runtime could not start")
    });
    tauri::async_runtime::set(runtime.handle().clone());
}

pub fn run() {
    // Worker threads run node rescans; debug builds need more than tokio's 2 MiB default. This
    // must come first: anything touching tauri::async_runtime would create the default runtime.
    install_async_runtime();
    // Before any worker starts. A failure does not stop the app: every tool launch repeats the
    // initialization and is refused for as long as it fails.
    let _ = infrastructure::shared::process::initialize_tool_policy();
    let bindings = ipc::bindings::builder::<tauri::Wry>();
    let logging_guard: Arc<Mutex<Option<logging::LoggingGuard>>> = Arc::new(Mutex::new(None));
    let setup_guard = logging_guard.clone();
    // Retain the coordinator (and any exit lease) through run_return and log flushing.
    let close = Arc::new(OnceLock::new());

    let mut builder = tauri::Builder::default()
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
        // Used from Rust only, for Copy diagnostics; no capability grants its commands either.
        .plugin(tauri_plugin_clipboard_manager::init());
    // The signed updater exists only in builds that carry both the public key and the HTTPS
    // endpoint. Without them nothing is registered and the update commands refuse.
    let updater_config = updates::UpdaterBuildConfig::from_build_env();
    if updater_config.is_some() {
        builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    }
    let mut context = tauri::generate_context!();
    if let Some(config) = &updater_config {
        context
            .config_mut()
            .plugins
            .0
            .insert("updater".to_owned(), config.plugin_config());
    }

    let builder = builder
        .invoke_handler(bindings.invoke_handler())
        .on_window_event(|window, event| {
            if window.label() == MANAGER_WINDOW
                && let WindowEvent::CloseRequested { api, .. } = event
            {
                app_lifecycle::on_manager_close_requested(window.app_handle(), api);
            }
        });
    let code = run_with_runtime(
        builder,
        context,
        close.clone(),
        move |app| {
            let picker = Arc::new(picker::DialogPicker::new(app.handle().clone()));
            // The install root is resolved before anything is opened: debug override, the
            // bootstrap pointer, then the platform default. An unusable pointer never falls
            // back to another root; the app starts in recovery mode instead.
            let state = match resolve_root() {
                Ok(resolved) => {
                    // Before composing, so the reconciliation report and any failure to open the
                    // root are logged. A missing root is not created just to hold logs.
                    if let Some(logs_dir) = state::startup_logs_dir(&resolved)
                        && let Ok(guard) = logging::init(&logs_dir)
                    {
                        *setup_guard
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(guard);
                        tracing::info!(
                            source = resolved.source.as_str(),
                            "starting vjumpkung-comfyui-desktop"
                        );
                    }
                    match tauri::async_runtime::block_on(AppState::compose(
                        &resolved,
                        picker.clone(),
                    )) {
                        Ok(state) => state,
                        Err(error) => {
                            tracing::error!(%error, "the app could not open its install root");
                            tauri::async_runtime::block_on(AppState::compose_recovery(
                                RecoveryInfo::startup_failed_for(&resolved),
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
            state.install_clipboard(Arc::new(diagnostics::TauriClipboard::new(
                app.handle().clone(),
            )));
            state.install_updater(Arc::new(updates::SignedUpdater::new(
                updater_config.clone(),
                |_| Box::new(updates::PluginChannel::new(app.handle().clone())),
                Arc::new(updates::AppQuiesce::new(
                    app.handle().clone(),
                    setup_guard.clone(),
                )),
                Arc::new(RandomIds::default()),
            )));
            Ok(state)
        },
        on_run_event,
    );
    // Dropping the guard flushes the non-blocking log writer.
    drop(
        logging_guard
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take(),
    );
    std::process::exit(code);
}

fn run_with_runtime<R: tauri::Runtime>(
    builder: tauri::Builder<R>,
    context: tauri::Context<R>,
    close: Arc<OnceLock<Arc<app_lifecycle::CloseCoordinator>>>,
    state_factory: impl FnOnce(&mut tauri::App<R>) -> Result<AppState, Box<dyn std::error::Error>>
    + Send
    + 'static,
    on_event: impl FnMut(&AppHandle<R>, RunEvent) + 'static,
) -> i32 {
    let app = builder
        .setup(move |app| {
            let state = state_factory(app)?;
            let _ = close.set(state.close.clone());
            app.manage(state);
            app_lifecycle::spawn_runtime_watch(app.handle().clone());
            tracing::info!("application setup finished");
            Ok(())
        })
        .build(context)
        .expect("error while building the application");
    app.run_return(on_event)
}

fn on_run_event<R: tauri::Runtime>(app: &AppHandle<R>, event: RunEvent) {
    // The last window closing must not end a retained background runtime.
    if let RunEvent::Exit = event
        && let Some(state) = app.try_state::<AppState>()
    {
        // Running transfers are paused and cleaned up before the process ends.
        tauri::async_runtime::block_on(state.shutdown_downloads());
        tauri::async_runtime::block_on(state.shutdown_library());
        tracing::info!("application shutdown finished");
    }
    if let RunEvent::ExitRequested {
        code: None, api, ..
    } = event
        && app.try_state::<AppState>().is_some()
    {
        api.prevent_exit();
        if !app_lifecycle::keep_core_alive(app) {
            tauri::async_runtime::spawn(app_lifecycle::perform(
                app.clone(),
                application::comfy_runtime::CloseAction::Exit,
            ));
        }
    }
}

#[cfg(test)]
#[path = "startup_tests.rs"]
mod startup_tests;
