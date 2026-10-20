**Go for the corrected crash-fix plan below. No-go for advancing to I until the startup and graceful-close checks pass.** The demonstrated failure is a Rust startup panic; the evidence does not establish a segmentation fault.

I confirmed HEAD `a4175d1` and a clean tree. This review made no edits and ran no builds or tests.

1. **Q1 — AI-decidable: choose F1 and retain the coordinator through process exit.**

   Create the outer `Arc<OnceLock<Arc<CloseCoordinator>>>`, publish `state.close.clone()` immediately before `app.manage(state)`, and remove the lookup at [lib.rs:131](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/desktop/src/lib.rs:131). Keep the outer slot in `run()` across `run_return`, log flushing, and `process::exit`.

   Tauri invokes user setup inside `run_return`’s Ready handling, and creates configured windows before invoking that setup. The current lookup therefore precedes state registration. See [Tauri Ready handling](C:/Users/vjump/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tauri-2.12.4/src/app.rs:1442) and [setup ordering](C:/Users/vjump/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tauri-2.12.4/src/app.rs:2688).

   F1 preserves the coordinator actually selected by normal, restricted, or recovery composition. F1b would replace the coordinator allocated by [AppState::new](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/desktop/src/state.rs:748) without providing a benefit.

   Retention remains correct and matches the [approved H3 policy](D:/vjump/vjump-comfyui-portable-with-resource-manager/docs/handoff/plans/m5-h-hardening-approved-plan.md:229). Do not depend on managed `AppState` being destroyed when `run_return` returns: installed adapters and spawned tasks hold app handles.

2. **Q2 — AI-decidable: choose a bounded version of F3(c).**

   Add default mock coverage for setup ordering and retention, plus an **ignored Windows native smoke test that is mandatory in the verification checklist**.

   In `crates/desktop/src/lib.rs`, extract a private `run_with_runtime<R>` covering the shared setup-publication, build, and `run_return` sequence. Inject the state factory and event callback. Keep native plugin registration in `run()`; mock tests must not initialize the real single-instance plugin.

   Add these tests:

   | File | Test |
   |---|---|
   | `crates/desktop/src/startup_tests.rs` | `run_return_publishes_state_and_retains_close` |
   | `crates/desktop/src/app_lifecycle.rs` | `restore_manager_before_setup_is_safe_with_or_without_a_window` |
   | `crates/desktop/src/app_lifecycle.rs` | `keep_core_alive_before_setup_is_false` |
   | `crates/desktop/tests/startup_smoke.rs` | `fresh_root_starts_and_closes_cleanly` |

   The first test must exercise the shared production sequence, establish that state is absent before setup, verify the retained and managed coordinator are the same `Arc`, and verify the exit lease remains held after `run_return`.

   MockRuntime supports deferred setup through `run_return`, but its [`request_exit` is unimplemented](C:/Users/vjump/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tauri-2.12.4/src/test/mock_runtime.rs:150), and its [window-listener registration discards the callback](C:/Users/vjump/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tauri-2.12.4/src/test/mock_runtime.rs:722). Terminate the mock loop by destroying its test window and allowing the resulting exit request. Treat this as ordering coverage; native close coverage comes from the smoke test.

   Gate the native test with `cfg(all(target_os = "windows", debug_assertions))` and an explicit ignore reason explaining WebView2, the interactive desktop, and the visible window. Its contract should be:

   - Spawn `CARGO_BIN_EXE_vjumpkung-comfyui-desktop` with a unique scratch root, `RUST_BACKTRACE=1`, and `RUST_LOG=info`.
   - Wait at most 60 seconds for a new `"application setup finished"` log marker **after state registration**, and find the manager window belonging to the child PID.
   - Confirm the child remains alive for three seconds.
   - Post `WM_CLOSE` to that window; require exit code 0 within 15 seconds.
   - Require an `"application shutdown finished"` marker after both shutdown awaits.
   - Capture stdout/stderr without leaving unread pipes. Use kill-and-wait only for failure cleanup; forced termination must fail the graceful-close assertion.

   The proposed node-scan marker occurs before state publication, so it is insufficient as the setup-complete signal.

   **Single-instance handling:** preflight the plugin’s identifier-derived mutex/window. If another instance exists, fail with an explicit precondition message before forwarding anything. An early exit 0 without the scratch-root setup marker is a failure, never a successful smoke test. Changing the root does not isolate the single-instance identity.

   Add a Windows-only `windows-sys` dev dependency with `Win32_UI_WindowsAndMessaging` in `crates/desktop/Cargo.toml`; the pinned package already exists.

3. **Q3 — AI-decidable: fix two additional startup entry points; guard exit callbacks defensively.**

   | Location | Finding and decision |
   |---|---|
   | [restore_manager](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/desktop/src/app_lifecycle.rs:338) | **Real pre-setup exposure.** Return immediately when `try_state::<AppState>()` is absent, before showing or creating windows. Capture the coordinator before spawning the recreation task. |
   | [on_manager_close_requested](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/desktop/src/app_lifecycle.rs:260) | Preserve `prevent_close()`, then return without spawning policy work if state is absent. Startup-time close is deferred until initialization finishes. |
   | [keep_core_alive](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/desktop/src/app_lifecycle.rs:250) | Return false when state is absent. |
   | [run event callback](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/desktop/src/lib.rs:132) | Extract `on_run_event<R>`. On Exit, shut down only when state exists. On an unnumbered ExitRequested without state, leave the default exit permitted and spawn no policy task. |

   The single-instance hazard deserves particular attention. Its plugin initializes during [Builder::build](C:/Users/vjump/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tauri-2.12.4/src/app.rs:2607); its Windows callback runs directly from an [`extern "system"` window procedure](C:/Users/vjump/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tauri-plugin-single-instance-2.5.3/src/platform_impl/windows.rs:162). WebView2 construction [pumps native messages](C:/Users/vjump/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/webview2-com-0.39.1/src/lib.rs:60) before user setup. A panic here can become an abort at the native callback boundary.

   I found no other pre-setup caller requiring changes:

   - `spawn_runtime_watch` is invoked after `manage`.
   - `comfy_window` state access follows IPC or that watcher; its `ComfyOrigin` `try_state` is safe.
   - Updater constructors only store handles. State access occurs during later update operations.
   - Diagnostics constructors do not read managed state.
   - Internal `decide`, `perform`, restart, and reconciliation accesses can retain their post-registration invariant once the entry points above are guarded.

4. **Q4 — AI-decidable: expand F4; make no speculative stack or COM changes.**

   I inspected the current debug executable’s PE header: **1,048,576-byte stack reserve, 4,096-byte initial commit**. The direct `block_on(AppState::compose(...))` at [lib.rs:91](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/desktop/src/lib.rs:91) therefore deserves native execution coverage. Passing test threads does not establish main-thread stack safety. Your successful fresh-root probe substantially reduces concern, but does not cover every deeper startup branch.

   Keep F4 and add these checks:

   - Fresh, half-setup copy, moved/restricted copy, and recovery startup; verify the displayed phase and selected root.
   - Launch a second instance during initialization and again after Ready; the first instance must survive.
   - Close during initialization, then close normally after Ready.
   - Exercise idle close, maintenance refusal, and—once an installed environment is available—Stop-and-exit and headless reopen.
   - Capture exit status and stderr for every unexpected termination. A stack overflow or access violation needs its own debugger stack before selecting a remedy.

   Two isolation corrections matter:

   - The root override is [debug-only](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/shared/paths.rs:169). Release smoke checks need an isolated bootstrap/config environment.
   - A nonexistent debug override becomes a fresh root; it does not exercise bootstrap-missing recovery. Use an isolated child `APPDATA` with a missing-root or corrupt bootstrap fixture.

   A copied installation with a valid old-root manifest intentionally enters restricted mode. Verify the source installation remains unchanged, and do not count that result as a normal half-setup retry.

   No additional COM initialization is justified. Tao initializes OLE/STA for its window and explicitly rejects incompatible COM mode; see [Tao initialization](C:/Users/vjump/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tao-0.37.1/src/platform_impl/windows/window.rs:1136). Keyring probing and NVML detection already run on blocking workers. Preserve the Tauri/WebView event loop on the main thread.

The implementation scope is `lib.rs`, `app_lifecycle.rs`, the two new test files, and the Windows dev dependency. Update `HANDOFF.md` and `docs/IMPLEMENTATION_STATUS.md` with the crash cause, native evidence, required smoke command, current HEAD, and corrected NVIDIA availability. No domain/application changes, IPC regeneration, or `state.rs` redesign are needed.

Main should run these sequentially after implementation; **none were executed in this review**:

```powershell
cargo fmt --all -- --check
cargo test -p desktop --lib startup_tests:: --locked -j 6
cargo test -p desktop --lib app_lifecycle::tests:: --locked -j 6
pnpm -C ui build
cargo test -p desktop --test startup_smoke --locked -j 6 --features tauri/custom-protocol fresh_root_starts_and_closes_cleanly -- --ignored --exact --nocapture --test-threads=1
cargo clippy --workspace --all-targets --locked -j 6 -- -D warnings
cargo test --workspace --locked -j 6 --no-fail-fast
node scripts/check-arch-deps.mjs
```

Proceed to I after these checks and F4 pass, then R and README/docs. Windows evidence does not close the outstanding Linux acceptance work.

**Indispensable user questions: none. Q1–Q4 are AI-decidable within the existing authorization.**