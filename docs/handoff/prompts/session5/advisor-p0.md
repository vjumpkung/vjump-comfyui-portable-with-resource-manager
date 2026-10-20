ROLE: advisor (read-only). Do NOT edit any file, do NOT run git write commands. You may read files and run read-only commands (rg, cat, git log/show/diff, cargo metadata). Do not run cargo build/test (another process may hold the build lock).

REPO: D:\vjump\vjump-comfyui-portable-with-resource-manager (Rust/Tauri 2.12.4 desktop app, Clean Architecture; read AGENTS.md and HANDOFF.md §0a/§3 first). HEAD a4175d1 (owner commit of session-4 work).

## User goal
"debug program to check segmentation fault first", then continue the remaining phases (package I UI integration, package R release audit), then README/docs.

## Evidence gathered by main (verified, not hypotheses)
1. `cargo test --workspace --locked -j 6 --no-fail-fast` at HEAD: 709 passed / 0 failed / 9 ignored (matches the handoff baseline). No test binary crashes.
2. Running the debug app binary (`target/debug/vjumpkung-comfyui-desktop.exe`, VJUMPKUNG_COMFYUI_HOME=<empty scratch dir>) exits immediately with code 101 (also 101 from Git Bash, no "Segmentation fault" text):
   `thread 'main' panicked at tauri-2.12.4/src/lib.rs:761:7: state() called before manage() for desktop::state::AppState` with backtrace frame `desktop::run at crates/desktop/src/lib.rs:131`.
3. crates/desktop/src/lib.rs:130-131 (added in session 4 / H3 close policy, commit a4175d1):
   ```rust
   // Retain the coordinator (and any exit lease) through run_return and log flushing.
   let _close = app.state::<AppState>().close.clone();
   let code = app.run_return(|app, event| { ... });
   ```
   Tauri 2.12.4 runs the `.setup(...)` hook (which does `app.manage(state)`) inside `App::run_return` on `RuntimeRunEvent::Ready` (tauri-2.12.4/src/app.rs:1423-1446, `fn setup` at :2688), i.e. AFTER line 131. So EVERY launch of the committed app panics before any window. The owner's "segmentation fault" is almost certainly this crash (no app log exists after the commit; nothing else crashes).
4. Diagnostic probe (temporary, reverted; tree is clean at HEAD again): with line 131 commented out, the app composes a fresh root (migration 8, startup reconciliation, model recovery, node scan all logged), the WebView UI loads under `pnpm -C ui tauri dev`, and the wizard steps Install root → Hardware (NVML) → Python source → Profile → Options work without any crash. NOTE: this machine now HAS an NVIDIA GeForce RTX 3060 (driver 610.88, CUDA 13.3, 12 GiB) — the handoff's "no NVIDIA" is outdated.
5. `CloseCoordinator` (crates/desktop/src/app_lifecycle.rs:78-150) holds `exit_lease: Mutex<Option<Box<dyn MaintenanceLease>>>`; AppState.close is `pub(crate) Arc<CloseCoordinator>` created in `AppState::new` (state.rs:748). The intent of line 131 is that the coordinator (and an exit lease acquired by the close policy) outlives `run_return` (which consumes and drops the App and its managed AppState) until `std::process::exit(code)` after log flushing, so no maintenance can start in that window. Test `exit_lease_prevents_new_maintenance_before_process_exit` (app_lifecycle.rs:650) covers the coordinator, not the wiring in run().
6. No test exercises `desktop::run()`; that is why 709 tests missed it.

## Proposed fix (main)
F1. In `run()`: create `let close_slot: Arc<OnceLock<Arc<app_lifecycle::CloseCoordinator>>> = Arc::new(OnceLock::new());` (or mirror the existing `logging_guard: Arc<Mutex<Option<..>>>` idiom), move a clone into the setup closure, and in setup do `let _ = close_slot.set(state.close.clone());` immediately before `app.manage(state)`. Remove the `app.state::<AppState>()` call before `run_return`. Keep `close_slot` alive in `run()` until after the logging-guard drop and `std::process::exit(code)` (process::exit runs no destructors, so the Arc lives to process end). Keep the existing comment intent.
   Alternative F1b: create one `Arc<CloseCoordinator>` before the builder and inject it into the composed AppState (`state.close = close.clone()` before manage). Main prefers F1 (no change to AppState construction).
F2. Audit every other `app.state::<AppState>()` / `try_state` call reachable before setup completes (single-instance callback `restore_manager`, `on_window_event`, `run_return` callback for events before Ready, `app_lifecycle::*`) and report any that can run before `manage`.
F3. Regression test so this cannot recur. Options:
   (a) Integration smoke test in crates/desktop/tests/ that spawns `env!("CARGO_BIN_EXE_vjumpkung-comfyui-desktop")` with VJUMPKUNG_COMFYUI_HOME=<tempdir>, waits (bounded, e.g. ≤60 s) for `data/logs/app.log` to contain "custom node recovery and scan finished", asserts the process is still running ~3 s later, then requests a graceful close (Windows: WM_CLOSE to the manager window, or kill) and asserts the exit. Needs WebView2 + an interactive desktop, opens a window during `cargo test`, and conflicts with a running instance via tauri-plugin-single-instance (second instance exits 0 → must be reported clearly). Run by default on Windows, or `#[ignore]` + listed in the HANDOFF check list?
   (b) Refactor run() so the builder/setup wiring is generic over `R: tauri::Runtime` and test it with `tauri::test::mock_builder()` / MockRuntime (dev-dependency `tauri` already has feature "test"), asserting run_return reaches setup without panicking. Larger change to run().
   (c) both.
F4. After the fix main will manually verify: fresh root, a COPY of the owner's half-set-up bootstrap root D:\vjump\test-comfyui (failed first-run, envs/tools present), a moved-root copy (restricted/rebuild_required path), recovery-mode start, and a graceful window close (exit path: RunEvent::Exit shutdown_downloads/shutdown_library + exit lease) — looking for any further crash.

## Questions for you (classify each AI-decidable vs user-only and decide the AI-decidable ones)
Q1. F1 vs F1b (or something else)? Is retaining the coordinator through process exit still the right semantics?
Q2. Regression test choice F3 (a)/(b)/(c), run-by-default vs ignored, Windows-only gating, exact test name(s), and how to handle the single-instance conflict and the window popping up.
Q3. Any other `state()`-before-`manage()` hazards (F2) — inspect app_lifecycle.rs, lib.rs, comfy_window.rs, updates.rs, diagnostics.rs.
Q4. Anything else in the startup/exit path (lib.rs, state.rs compose/compose_recovery, app_lifecycle.rs) that could crash natively (stack overflow of the 1 MiB Windows main thread from block_on(AppState::compose(..)) in debug builds, COM init on the main thread, etc.) and should be checked in F4?

## Required output
go/no-go; corrected plan (exact files, functions, test names); decisions + assumptions with file:line evidence; risks; success checks (exact commands); only indispensable user questions.
