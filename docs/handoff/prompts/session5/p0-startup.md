ROLE: coder (write-capable). Mechanically implement the advisor-approved plan below. Do not redesign or expand scope. If the plan is underspecified or conflicts with the code, stop that item and report the gap precisely instead of improvising.

## Build environment and repository rules (mandatory)
- WORKING COPY: `D:\vjump\vjump-comfyui-portable-with-resource-manager` (branch `feat/m0-skeleton`, HEAD `a4175d1`, working tree clean at start — all earlier session work is committed).
- Read `docs/handoff/tooling/coder-env.md` and `AGENTS.md` first. Build with the default MSVC toolchain pinned by `rust-toolchain.toml` into the default `target/` dir; do NOT set RUSTUP_TOOLCHAIN or CARGO_TARGET_DIR.
- Memory: 32 GB machine, ~10 GB free. Pass `-j 6` to cargo, prefer targeted `-p desktop` runs with test filters while iterating, run workspace-wide commands once at the end. Never run two cargo commands at once.
- Rust: rustfmt, clippy `-D warnings`, composition only in crates/desktop. Match surrounding style and comment density (short "why" comments, no narration).
- Do not commit, push, stash, reset, checkout, apply, clean or touch git history. Do not modify files outside the stated scope.

## Problem (verified by main)
Every launch of the committed app panics before any window: `thread 'main' panicked at tauri-2.12.4/src/lib.rs:761:7: state() called before manage() for desktop::state::AppState`, backtrace `desktop::run at crates/desktop/src/lib.rs:131` (exit code 101). Line 131 (`let _close = app.state::<AppState>().close.clone();`, added by the H3 close policy) runs before `run_return`, but Tauri 2.12.4 invokes the `.setup(..)` hook (which calls `app.manage(state)`) inside `App::run_return` on `RuntimeRunEvent::Ready` (tauri-2.12.4/src/app.rs:1442, :2688). No test exercises `desktop::run()`, so the 709 Rust tests pass. With line 131 disabled the app composes a fresh root and the UI works.

## Approved plan (advisor gpt-6.1-sol, GO; implement exactly)

### 1. Retain the close coordinator through process exit (F1) — crates/desktop/src/lib.rs
- Create an outer `Arc<OnceLock<Arc<app_lifecycle::CloseCoordinator>>>` in `run()`. In setup, publish `state.close.clone()` into it immediately before `app.manage(state)`. Remove the `app.state::<AppState>()` lookup before `run_return`. Keep the outer slot alive in `run()` across `run_return`, the logging-guard drop and `std::process::exit(code)` (keep the existing comment intent: the coordinator and any exit lease are retained through run_return and log flushing). Do not depend on managed AppState being destroyed when run_return returns.
- Add a log marker `tracing::info!("application setup finished")` in setup AFTER `app.manage(state)` and `app_lifecycle::spawn_runtime_watch(..)`.
- Add a log marker `tracing::info!("application shutdown finished")` in the `RunEvent::Exit` handling AFTER both `shutdown_downloads` and `shutdown_library` awaits.

### 2. Testable startup sequence (mock coverage) — crates/desktop/src/lib.rs + new crates/desktop/src/startup_tests.rs
- Extract a private generic `run_with_runtime<R: tauri::Runtime>` covering the shared sequence: setup publication (state factory -> publish close into the slot -> `manage` -> post-manage hooks -> "application setup finished"), `build(context)` and `run_return` with the event callback. Inject the state factory and the event callback so the mock test can supply its own state. Keep native plugin registration (single-instance, log, opener, dialog, clipboard, updater), `generate_context!`, the updater config and the real composition (`resolve_root`, logging init, `AppState::compose`/`compose_recovery`, install_shell/clipboard/updater) in `run()`; mock tests must NOT initialize the real single-instance plugin.
- Extract the run event handling into `on_run_event<R: tauri::Runtime>(app: &AppHandle<R>, event: RunEvent)`: on `RunEvent::Exit` shut down downloads and library ONLY when `try_state::<AppState>()` is present (then log "application shutdown finished"); on an unnumbered `ExitRequested` WITHOUT state, leave the default exit permitted and spawn no policy task; with state keep the current behaviour (prevent_exit + keep_core_alive / perform Exit).
- Test `startup_tests::run_return_publishes_state_and_retains_close` (file crates/desktop/src/startup_tests.rs, wired with `#[cfg(test)] #[path = ...] mod` in lib.rs like state/recovery_tests.rs): use `tauri::test::mock_builder()` / MockRuntime (dev-dependency tauri already has feature "test") through the SAME `run_with_runtime` production sequence; establish that state is absent before setup, that after run_return the retained coordinator and the managed coordinator were the same `Arc` (Arc::ptr_eq), and that an exit lease placed in the coordinator remains held after run_return. MockRuntime notes from the advisor: `request_exit` is unimplemented (tauri-2.12.4/src/test/mock_runtime.rs:150) and window-listener registration discards the callback (:722) — terminate the mock loop by destroying its test window and allowing the resulting exit request. This test covers ordering only.

### 3. Guard pre-setup entry points — crates/desktop/src/app_lifecycle.rs
- `restore_manager` (~:338; called from the single-instance callback, which runs inside an `extern "system"` window procedure during Builder::build / WebView2 message pumping — a panic there can abort): return immediately when `try_state::<AppState>()` is absent, BEFORE showing or creating windows. Capture the coordinator before spawning the recreation task.
- `on_manager_close_requested` (~:260): preserve `prevent_close()`, then return without spawning policy work if state is absent (a close during initialization is deferred until initialization finishes).
- `keep_core_alive` (~:250): return false when state is absent.
- Tests in app_lifecycle.rs tests module: `restore_manager_before_setup_is_safe_with_or_without_a_window` and `keep_core_alive_before_setup_is_false` (mock app without managed AppState).
- Internal `decide`, `perform`, restart and reconciliation accesses keep their post-registration invariant; do not change them.

### 4. Native smoke test (ignored, mandatory in the checklist) — new crates/desktop/tests/startup_smoke.rs
- `fresh_root_starts_and_closes_cleanly`, gated `#[cfg(all(target_os = "windows", debug_assertions))]`, `#[ignore = "native: needs WebView2, an interactive desktop and opens a visible window; run explicitly"]`.
- Spawn `env!("CARGO_BIN_EXE_vjumpkung-comfyui-desktop")` with a unique scratch root (tempfile) in `VJUMPKUNG_COMFYUI_HOME`, `RUST_BACKTRACE=1`, `RUST_LOG=info`. Capture stdout/stderr without leaving unread pipes (reader threads or files).
- Precondition (single instance): before spawning, detect an already-running instance of the app (the tauri-plugin-single-instance identifier-derived mutex/window for identifier `dev.vjump.comfyui-desktop` — read tauri-plugin-single-instance-2.5.3/src/platform_impl/windows.rs for the exact names); if present, fail with an explicit precondition message. An early exit 0 without the setup marker is a FAILURE, never success.
- Wait at most 60 s for `"application setup finished"` in `<root>/data/logs/app.log` (or the captured stdout); find the top-level window belonging to the child PID (EnumWindows + GetWindowThreadProcessId, visible, title "vjumpkung ComfyUI").
- Confirm the child is still alive 3 s later.
- `PostMessageW(hwnd, WM_CLOSE, 0, 0)`; require exit code 0 within 15 s, and `"application shutdown finished"` in the log after the setup marker.
- On any failure: kill-and-wait for cleanup only, and include exit status + captured stderr tail in the assertion message. A forced termination must fail the graceful-close assertion.
- Add a Windows-only dev-dependency in crates/desktop/Cargo.toml: `[target.'cfg(windows)'.dev-dependencies] windows-sys = { workspace = true, features = ["Win32_UI_WindowsAndMessaging"] }` (the workspace already pins windows-sys =0.61.2; add any additional feature you need, e.g. Win32_Foundation, Win32_System_Threading). `tempfile` is already a dev-dependency.

### Scope
Only: crates/desktop/src/lib.rs, crates/desktop/src/app_lifecycle.rs, new crates/desktop/src/startup_tests.rs, new crates/desktop/tests/startup_smoke.rs, crates/desktop/Cargo.toml (+ Cargo.lock if the dev-dependency changes it). No domain/application changes, no IPC/bindings regeneration, no state.rs redesign. Do not edit HANDOFF.md or docs (main does).

## Success criteria (run in this order at the end; report each with exact counts)
```
cargo fmt --all -- --check
cargo test -p desktop --lib startup_tests:: --locked -j 6
cargo test -p desktop --lib app_lifecycle::tests:: --locked -j 6
pnpm -C ui build
cargo test -p desktop --test startup_smoke --locked -j 6 --features tauri/custom-protocol fresh_root_starts_and_closes_cleanly -- --ignored --exact --nocapture --test-threads=1
cargo clippy --workspace --all-targets --locked -j 6 -- -D warnings
cargo test --workspace --locked -j 6 --no-fail-fast      (baseline 709 passed / 0 failed / 9 ignored; expect +3 passed, +1 ignored)
node scripts/check-arch-deps.mjs
```
Also: `cargo build -p desktop --locked -j 6` (plain debug build) and run `target/debug/vjumpkung-comfyui-desktop.exe` with VJUMPKUNG_COMFYUI_HOME=<new scratch dir> for ~10 s to confirm it no longer panics (kill it afterwards).

OUTPUT: per-file summary with file:line, the key diffs, every command above with its result and counts, deviations from the plan with reasons, anything not verifiable.
