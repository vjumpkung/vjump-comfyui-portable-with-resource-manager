use std::sync::atomic::{AtomicBool, Ordering};

use application::{
    settings::{
        CredentialBackend, CredentialPresence, CredentialStore, CredentialStoreError,
        PendingCredentialRef,
    },
    shared::GateError,
};
use domain::settings::CredentialKind;
use infrastructure::settings::secrets::SecretIngress;
use tauri::{
    WebviewWindowBuilder,
    test::{mock_builder, mock_context, noop_assets},
};

use super::*;

struct NoCredentials;

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
    let before_setup = Arc::new(AtomicBool::new(false));
    let build_before_setup = before_setup.clone();
    let builder = mock_builder().plugin(
        tauri::plugin::Builder::<_, ()>::new("startup-order-test")
            .setup(move |app, _| {
                assert!(app.try_state::<AppState>().is_none());
                build_before_setup.store(true, Ordering::SeqCst);
                Ok(())
            })
            .build(),
    );

    let code = run_with_runtime(
        builder,
        mock_context(noop_assets()),
        close.clone(),
        move |app| {
            assert!(before_setup.load(Ordering::SeqCst));
            assert!(app.try_state::<AppState>().is_none());
            assert!(setup_close.get().is_none());
            WebviewWindowBuilder::new(app.handle(), MANAGER_WINDOW, Default::default()).build()?;
            Ok(AppState::new(
                Err(RecoveryInfo::startup_failed()),
                Arc::new(NoCredentials),
                SecretIngress::default(),
                Arc::new(picker::DialogPicker::new(app.handle().clone())),
                Default::default(),
            ))
        },
        move |app, event| match event {
            RunEvent::Ready => {
                let state = app.state::<AppState>();
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

    assert_eq!(code, 0);
    let (managed, gate) = observed.lock().unwrap().take().unwrap();
    let retained = close.get().expect("setup must publish the coordinator");
    assert!(Arc::ptr_eq(retained, &managed.upgrade().unwrap()));
    assert_eq!(
        tauri::async_runtime::block_on(gate.try_acquire()).err(),
        Some(GateError::Busy)
    );
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
