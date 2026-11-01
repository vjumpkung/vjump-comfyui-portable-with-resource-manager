use std::{
    sync::{Barrier, mpsc},
    time::{Duration, Instant},
};

use application::{
    settings::{
        CredentialBackend, CredentialPresence, CredentialStore, CredentialStoreError,
        PendingCredentialRef,
    },
    shared::GateError,
};
use domain::settings::CredentialKind;
use infrastructure::settings::secrets::SecretIngress;
use tauri::test::{mock_builder, mock_context, noop_assets};

use super::*;

struct NoCredentials;

fn recovery_state<R: tauri::Runtime>(app: &AppHandle<R>) -> AppState {
    AppState::new(
        Err(RecoveryInfo::startup_failed()),
        Arc::new(NoCredentials),
        SecretIngress::default(),
        Arc::new(picker::DialogPicker::new(app.clone())),
        Default::default(),
    )
}

fn manager_context() -> tauri::Context<tauri::test::MockRuntime> {
    let mut context = mock_context(noop_assets());
    // Tauri creates configured windows on the main thread in its Ready/setup hook.
    context
        .config_mut()
        .app
        .windows
        .push(tauri::utils::config::WindowConfig {
            label: MANAGER_WINDOW.into(),
            ..Default::default()
        });
    context
}

#[async_trait::async_trait]
impl CredentialStore for NoCredentials {
    async fn commit(
        &self,
        _: CredentialKind,
        _: PendingCredentialRef,
    ) -> Result<(), CredentialStoreError> {
        unreachable!()
    }

    async fn clear(&self, _: CredentialKind) -> Result<(), CredentialStoreError> {
        unreachable!()
    }

    async fn status(&self, _: CredentialKind) -> Result<CredentialPresence, CredentialStoreError> {
        unreachable!()
    }

    fn backend(&self) -> CredentialBackend {
        unreachable!()
    }
}

#[test]
fn run_return_publishes_state_and_retains_close() {
    let close = Arc::new(OnceLock::new());
    let setup_close = close.clone();
    let observed = Arc::new(Mutex::new(None));
    let event_observed = observed.clone();
    let worker = Arc::new(Mutex::new(None));
    let (release, released) = mpsc::channel();
    let mut release = Some(release);
    let deadline = Instant::now() + Duration::from_secs(15);

    let code = run_with_runtime(
        mock_builder(),
        manager_context(),
        close.clone(),
        worker.clone(),
        move |app| {
            assert_eq!(std::thread::current().name(), Some("startup"));
            assert!(app.try_state::<AppState>().is_none());
            assert!(setup_close.get().is_none());
            released.recv_timeout(Duration::from_secs(15)).unwrap();
            Ok(recovery_state(app))
        },
        |_, code| panic!("unexpected startup exit {code}"),
        move |app, event| match event {
            RunEvent::Ready => {
                assert!(!ipc::commands::app_ready(app.clone()));
                assert!(app.try_state::<AppState>().is_none());
                app_lifecycle::restore_manager(app);
                assert!(app.get_webview_window(MANAGER_WINDOW).is_some());
                release.take().unwrap().send(()).unwrap();
            }
            RunEvent::MainEventsCleared => {
                assert!(Instant::now() < deadline, "startup did not finish");
                if !ipc::commands::app_ready(app.clone())
                    || event_observed.lock().unwrap().is_some()
                {
                    return;
                }
                let state = app.state::<AppState>();
                assert!(state.services().is_err(), "recovery counts as ready");
                tauri::async_runtime::block_on(state.close.prepare_recovery(state.gate.as_ref()))
                    .unwrap();
                *event_observed.lock().unwrap() =
                    Some((Arc::downgrade(&state.close), state.gate.clone()));
                // MockRuntime cannot request_exit; destroying its last window permits exit.
                app.get_webview_window(MANAGER_WINDOW)
                    .unwrap()
                    .destroy()
                    .unwrap();
            }
            RunEvent::ExitRequested { .. } => {}
            event => on_run_event(app, event),
        },
    );

    worker.lock().unwrap().take().unwrap().join().unwrap();
    assert_eq!(code, 0);
    let (managed, gate) = observed.lock().unwrap().take().unwrap();
    let retained = close.get().expect("setup must publish the coordinator");
    assert!(Arc::ptr_eq(retained, &managed.upgrade().unwrap()));
    assert_eq!(
        tauri::async_runtime::block_on(gate.try_acquire()).err(),
        Some(GateError::Busy)
    );
}

#[test]
fn close_before_publication_discards_state_and_retains_the_loop() {
    deferred_startup_exit(true);
}

#[test]
fn ordinary_exit_request_before_publication_retains_the_loop() {
    deferred_startup_exit(false);
}

fn deferred_startup_exit(close_window: bool) {
    let close = Arc::new(OnceLock::new());
    let worker = Arc::new(Mutex::new(None));
    let (release, released) = mpsc::channel();
    let (state_sent, state_received) = mpsc::channel();
    let (exit_sent, exit_received) = mpsc::channel();
    let mut release = Some(release);
    let mut exiting = false;
    let mut exit_requested = false;
    let deadline = Instant::now() + Duration::from_secs(15);
    let code = run_with_runtime(
        mock_builder(),
        manager_context(),
        close.clone(),
        worker.clone(),
        move |app| {
            let state = recovery_state(app);
            state_sent.send(Arc::downgrade(&state.close)).unwrap();
            released.recv_timeout(Duration::from_secs(15)).unwrap();
            assert!(!ipc::commands::app_ready(app.clone()));
            Ok(state)
        },
        move |_, code| exit_sent.send(code).unwrap(),
        move |app, event| {
            match &event {
                RunEvent::Ready => {
                    let window = app.get_webview_window(MANAGER_WINDOW).unwrap();
                    if close_window {
                        window.close().unwrap();
                    } else {
                        // Produces an ordinary ExitRequested { code: None }.
                        window.destroy().unwrap();
                    }
                }
                RunEvent::WindowEvent {
                    label,
                    event: WindowEvent::CloseRequested { api, .. },
                    ..
                } if label == MANAGER_WINDOW => {
                    app_lifecycle::on_manager_close_requested(app, api);
                }
                RunEvent::MainEventsCleared => {
                    assert!(Instant::now() < deadline, "startup exit did not finish");
                    let gate = app.state::<Arc<startup::StartupGate>>();
                    if !gate.is_exiting() || exiting {
                        return;
                    }
                    // Reaching this callback proves close/exit was prevented and
                    // the loop remained alive while the factory was blocked.
                    // Only the exit arms of the handlers set the gate to
                    // Exiting here, and an ordinary exit request must have been
                    // delivered exactly in the variant that destroys the window.
                    if let Some(release) = release.take() {
                        assert_eq!(exit_requested, !close_window);
                        // MockRuntime never emits WindowEvent::Destroyed, so Tauri's
                        // registry keeps a destroyed window; its presence says
                        // nothing in either variant. restore_manager must still be
                        // a no-op (it never publishes or recreates anything).
                        app_lifecycle::restore_manager(app);
                        assert!(app.try_state::<AppState>().is_none());
                        release.send(()).unwrap();
                    }
                    if let Ok(code) = exit_received.try_recv() {
                        assert_eq!(code, 0);
                        assert!(app.try_state::<AppState>().is_none());
                        exiting = true;
                        // The seam captures the real request. Only test cleanup
                        // uses last-window destruction to end MockRuntime's loop.
                        // The runtime already dropped the destroyed manager, so the
                        // destroy variant needs a fresh window to be the last one.
                        let window = if close_window {
                            app.get_webview_window(MANAGER_WINDOW).unwrap()
                        } else {
                            tauri::WebviewWindowBuilder::new(app, "cleanup", Default::default())
                                .build()
                                .unwrap()
                        };
                        window.destroy().unwrap();
                    }
                }
                RunEvent::ExitRequested { .. } if exiting => return,
                RunEvent::ExitRequested { code: None, .. } => exit_requested = true,
                _ => {}
            }
            on_run_event(app, event);
        },
    );
    worker.lock().unwrap().take().unwrap().join().unwrap();
    assert_eq!(code, 0);
    assert!(close.get().is_none());
    assert!(state_received.recv().unwrap().upgrade().is_none());
}

#[test]
fn restore_while_exiting_does_not_recreate_the_manager() {
    let app = tauri::test::mock_app();
    let gate = Arc::new(startup::StartupGate::default());
    let state = recovery_state(app.handle());
    state.close.set_headless(true);
    app.manage(state);
    app.manage(gate.clone());
    assert!(gate.begin_exit());
    app_lifecycle::restore_manager(app.handle());
    assert!(app.webview_windows().is_empty());
    assert!(app.state::<AppState>().close.headless());

    tauri::WebviewWindowBuilder::new(&app, MANAGER_WINDOW, Default::default())
        .build()
        .unwrap();
    app_lifecycle::restore_manager(app.handle());
    assert!(app.state::<AppState>().close.headless());
}

#[test]
fn publication_winning_exit_race_keeps_ready_lifecycle() {
    // MockRuntime cannot run the ready-state close path to completion: it has no
    // dialog plugin (a refused close shows a dialog), its request_exit is
    // unimplemented (an accepted close exits), and it never emits
    // WindowEvent::Destroyed (so a refused close cannot be seen restoring the
    // manager). The ready lifecycle is therefore proved by what the startup
    // handling must leave alone: the gate stays Ready, begin_exit() (the atomic
    // decision on_run_event makes for ExitRequested) declines, and the startup
    // exit seam is never used.
    let close = Arc::new(OnceLock::new());
    let worker = Arc::new(Mutex::new(None));
    let event_worker = worker.clone();
    let requested = Arc::new(Mutex::new(Vec::new()));
    let seam_requested = requested.clone();
    let event_requested = requested.clone();
    let exit_seen = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let event_exit_seen = exit_seen.clone();
    let mut destroyed = false;
    let deadline = Instant::now() + Duration::from_secs(15);
    run_with_runtime(
        mock_builder(),
        manager_context(),
        close.clone(),
        worker.clone(),
        |app| Ok(recovery_state(app)),
        move |_, code| seam_requested.lock().unwrap().push(code),
        move |app, event| match event {
            RunEvent::MainEventsCleared if !destroyed => {
                assert!(Instant::now() < deadline, "publication did not finish");
                let gate = app.state::<Arc<startup::StartupGate>>();
                if !gate.is_ready() || !event_worker.lock().unwrap().as_ref().unwrap().is_finished()
                {
                    return;
                }
                assert!(app.try_state::<AppState>().is_some());
                assert!(!gate.begin_exit());
                assert!(gate.is_ready() && !gate.is_exiting());
                assert!(event_requested.lock().unwrap().is_empty());
                destroyed = true;
                // The last window going away produces an ordinary ExitRequested.
                app.get_webview_window(MANAGER_WINDOW)
                    .unwrap()
                    .destroy()
                    .unwrap();
            }
            RunEvent::ExitRequested { code: None, .. } if destroyed => {
                // The ready lifecycle owns this request, not the startup gate.
                let gate = app.state::<Arc<startup::StartupGate>>();
                assert!(!gate.begin_exit());
                assert!(gate.is_ready() && !gate.is_exiting());
                assert!(event_requested.lock().unwrap().is_empty());
                // Not forwarded: it would end in a dialog or app.exit, which
                // MockRuntime cannot serve. Not prevented, so the loop ends.
                event_exit_seen.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            event => on_run_event(app, event),
        },
    );
    worker.lock().unwrap().take().unwrap().join().unwrap();
    assert!(
        exit_seen.load(std::sync::atomic::Ordering::SeqCst),
        "the window destruction must request an exit"
    );
    assert!(requested.lock().unwrap().is_empty());
    assert!(close.get().is_some(), "the published coordinator is kept");
}

#[test]
fn exit_event_before_state_prevents_late_publication() {
    let app = tauri::test::mock_app();
    assert!(!ipc::commands::app_ready(app.handle().clone()));
    let gate = Arc::new(startup::StartupGate::default());
    app.manage(gate.clone());
    on_run_event(app.handle(), RunEvent::Exit);
    let state = recovery_state(app.handle());
    let weak = Arc::downgrade(&state.close);
    assert!(
        gate.publish(state, |_| panic!("must not publish after exit"))
            .is_err()
    );
    assert!(!gate.is_ready());
    assert!(weak.upgrade().is_none());
    assert!(gate.begin_exit());
}

#[test]
fn publish_and_exit_are_serialized_by_the_gate() {
    let app = tauri::test::mock_app();
    let state = recovery_state(app.handle());
    let gate = Arc::new(startup::StartupGate::default());
    let barrier = Arc::new(Barrier::new(2));
    let publish_gate = gate.clone();
    let publish_barrier = barrier.clone();
    let published = std::thread::spawn(move || {
        publish_barrier.wait();
        publish_gate.publish(state, drop).is_ok()
    });
    let exit_gate = gate.clone();
    let exiting = std::thread::spawn(move || {
        barrier.wait();
        exit_gate.begin_exit()
    });
    let published = published.join().unwrap();
    assert_ne!(published, exiting.join().unwrap());
    assert_eq!(gate.is_ready(), published);
    assert!(
        gate.publish(recovery_state(app.handle()), |_| panic!(
            "second publication"
        ))
        .is_err()
    );
}

#[test]
fn exit_waits_for_in_progress_publication() {
    let app = tauri::test::mock_app();
    let state = recovery_state(app.handle());
    let gate = Arc::new(startup::StartupGate::default());
    let (entered, entering) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let publish_gate = gate.clone();
    let published = std::thread::spawn(move || {
        publish_gate
            .publish(state, |_| {
                entered.send(()).unwrap();
                released.recv().unwrap();
            })
            .is_ok()
    });
    entering.recv_timeout(Duration::from_secs(15)).unwrap();
    let exit_gate = gate.clone();
    let (started, starting) = mpsc::channel();
    let exiting = std::thread::spawn(move || {
        started.send(()).unwrap();
        exit_gate.begin_exit()
    });
    starting.recv().unwrap();
    release.send(()).unwrap();
    assert!(published.join().unwrap());
    assert!(!exiting.join().unwrap());
    assert!(gate.is_ready());
}

#[test]
fn factory_failure_requests_code_one_without_publishing() {
    failed_factory(false);
}

#[test]
fn factory_panic_requests_code_one_without_publishing() {
    failed_factory(true);
}

#[cfg(windows)]
#[test]
#[ignore = "native: needs WebView2 and an interactive desktop; opens a visible window"]
fn native_startup_close_hides_until_the_worker_requests_exit() {
    let close = Arc::new(OnceLock::new());
    let worker = Arc::new(Mutex::new(None));
    let (release, released) = mpsc::channel();
    let (exit_sent, exit_received) = mpsc::channel();
    let (state_sent, state_received) = mpsc::channel();
    let hidden = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let event_hidden = hidden.clone();
    let mut release = Some(release);
    let mut closing = false;
    let mut context = mock_context::<tauri::Wry, _>(noop_assets());
    context.config_mut().identifier = "dev.vjump.batch4-startup-test".into();
    let profile = std::env::current_dir()
        .unwrap()
        .join("target/batch4-webview");
    std::fs::create_dir_all(&profile).unwrap();
    context
        .config_mut()
        .app
        .windows
        .push(tauri::utils::config::WindowConfig {
            label: MANAGER_WINDOW.into(),
            data_directory: Some(profile),
            ..Default::default()
        });
    let code = run_with_runtime(
        tauri::Builder::default()
            .any_thread()
            .plugin(tauri_plugin_single_instance::init(|_, _, _| {})),
        context,
        close.clone(),
        worker.clone(),
        move |app| {
            let state = recovery_state(app);
            state_sent.send(Arc::downgrade(&state.close)).unwrap();
            released.recv_timeout(Duration::from_secs(15)).unwrap();
            Ok(state)
        },
        move |handle, code| {
            exit_sent.send(code).unwrap();
            handle.exit(code);
        },
        move |app, event| {
            match &event {
                RunEvent::Ready => app
                    .get_webview_window(MANAGER_WINDOW)
                    .unwrap()
                    .close()
                    .unwrap(),
                RunEvent::WindowEvent {
                    label,
                    event: WindowEvent::CloseRequested { api, .. },
                    ..
                } if label == MANAGER_WINDOW => {
                    app_lifecycle::on_manager_close_requested(app, api);
                    closing = true;
                }
                RunEvent::MainEventsCleared if closing && release.is_some() => {
                    let window = app.get_webview_window(MANAGER_WINDOW).unwrap();
                    assert!(!window.is_visible().unwrap());
                    assert!(app.state::<Arc<startup::StartupGate>>().is_exiting());
                    assert!(app.try_state::<AppState>().is_none());
                    app_lifecycle::restore_manager(app);
                    assert!(!window.is_visible().unwrap());
                    event_hidden.store(true, std::sync::atomic::Ordering::SeqCst);
                    release.take().unwrap().send(()).unwrap();
                }
                _ => {}
            }
            on_run_event(app, event);
        },
    );
    worker.lock().unwrap().take().unwrap().join().unwrap();
    assert!(hidden.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(exit_received.recv().unwrap(), 0);
    assert_eq!(code, 0);
    assert!(close.get().is_none());
    assert!(state_received.recv().unwrap().upgrade().is_none());
}

fn failed_factory(panics: bool) {
    let worker = Arc::new(Mutex::new(None));
    let close = Arc::new(OnceLock::new());
    let (release, released) = mpsc::channel();
    let (exit_sent, exit_received) = mpsc::channel();
    let mut release = Some(release);
    let mut exiting = false;
    let deadline = Instant::now() + Duration::from_secs(15);
    run_with_runtime(
        mock_builder(),
        manager_context(),
        close.clone(),
        worker.clone(),
        move |_| {
            released.recv_timeout(Duration::from_secs(15)).unwrap();
            assert!(!panics, "test factory panic");
            Err("test factory failure".into())
        },
        move |_, code| exit_sent.send(code).unwrap(),
        move |app, event| match event {
            RunEvent::Ready => {
                release.take().unwrap().send(()).unwrap();
            }
            RunEvent::MainEventsCleared if !exiting => {
                assert!(Instant::now() < deadline, "factory failure did not exit");
                if let Ok(code) = exit_received.try_recv() {
                    assert_eq!(code, 1);
                    assert!(!ipc::commands::app_ready(app.clone()));
                    assert!(app.state::<Arc<startup::StartupGate>>().is_exiting());
                    assert!(app.try_state::<AppState>().is_none());
                    exiting = true;
                    app.get_webview_window(MANAGER_WINDOW)
                        .unwrap()
                        .destroy()
                        .unwrap();
                }
            }
            RunEvent::ExitRequested { .. } if exiting => {}
            event => on_run_event(app, event),
        },
    );
    worker.lock().unwrap().take().unwrap().join().unwrap();
    assert!(close.get().is_none());
}

#[cfg(windows)]
#[test]
fn async_runtime_workers_and_blocking_threads_reserve_eight_mib() {
    fn reserve() -> usize {
        let (mut low, mut high) = (0, 0);
        // SAFETY: both pointers refer to live locals for the duration of the call.
        unsafe {
            windows_sys::Win32::System::Threading::GetCurrentThreadStackLimits(&mut low, &mut high)
        };
        high - low
    }

    let runtime = crate::async_runtime_builder().build().unwrap();
    let worker = runtime
        .block_on(runtime.spawn(async { reserve() }))
        .unwrap();
    let blocking = runtime.block_on(runtime.spawn_blocking(reserve)).unwrap();
    assert!(worker >= WORKER_STACK_BYTES, "worker stack {worker}");
    assert!(blocking >= WORKER_STACK_BYTES, "blocking stack {blocking}");
}
