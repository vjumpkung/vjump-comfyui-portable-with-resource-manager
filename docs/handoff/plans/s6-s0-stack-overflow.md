# S0 — P0: debug app overflows the main-thread stack at startup once custom nodes exist (draft by main)

## Native evidence (main, 2026-10-20 12:40, RTX 3060 machine)
- Binary `D:\vjc-s6\bin\vjumpkung-comfyui-desktop-s6-01.exe` (debug + `tauri/custom-protocol`, tree = HEAD e384d43 + Q,
  SHA-256 prefix 2ab32a47) launched with `VJUMPKUNG_COMFYUI_HOME=D:\comfyui-test\v1` (owner root; activated py312 env;
  `ComfyUI/custom_nodes/ComfyUI-Manager` installed by setup). stderr:
  `thread 'main' (44764) has overflowed its stack`, exit 127; app.log stops after
  "model operation recovery finished" (node reconcile/scan never logs). Reproducible twice.
- The owner's own start of the same root at 11:59 (before setup installed the Manager) finished
  ("custom node recovery and scan finished"). Fresh-root smoke test (`tests/startup_smoke.rs`) has no nodes → never hits it.
- Same exe with only the PE stack reserve raised (`editbin /STACK:8388608` on a copy) starts normally on the same root:
  "custom node recovery and scan finished", "application setup finished". ⇒ stack size, not unbounded recursion
  (debug-build async state machines of `AppState::compose` → `NodeServices::reconcile` → `rescan`).
- Where: `crates/desktop/src/lib.rs:97-114` `tauri::async_runtime::block_on(AppState::compose(..))` inside `.setup`
  on the main thread (Windows main-thread reserve = 1 MiB default; nothing sets `/STACK`). `crates/desktop/build.rs`
  only emits `rustc-link-arg-tests` manifest args (:97-109). Shutdown also uses `block_on` on the main thread
  (lib.rs:172-173). Later rescans (after ComfyUI stops, node commands) run on Tauri's default tokio runtime workers
  (2 MiB default stack) — not observed to overflow, not proven safe in debug.
- Release builds were not observed to overflow (smaller futures) but are not proven either.

## Proposed fix (minimal)
1. `crates/desktop/build.rs`: for the desktop binary on Windows, reserve an 8 MiB main-thread stack:
   MSVC `cargo:rustc-link-arg-bins=/STACK:8388608`; windows-gnu `cargo:rustc-link-arg-bins=-Wl,--stack,8388608`
   (select by `CARGO_CFG_TARGET_ENV`). Linux main threads already get the 8 MiB rlimit default; no change.
2. Tokio workers: install an explicit Tauri async runtime before the builder runs
   (`tauri::async_runtime::set(..)`) = multi-thread tokio runtime, `enable_all()`, `thread_stack_size(8 MiB)`
   (same worker count as the default). Only if the advisor rules it necessary (see question).
3. Tests: (a) `#[cfg(windows)]` integration test in `crates/desktop/tests/` that reads the PE header of
   `env!("CARGO_BIN_EXE_vjumpkung-comfyui-desktop")` and asserts `SizeOfStackReserve >= 8 MiB`
   (`desktop_exe_reserves_an_eight_mib_main_stack`); (b) extend the ignored native smoke test with a root that already
   contains a custom node folder tree (`root_with_installed_nodes_starts_and_closes_cleanly`), run explicitly by main.
4. Native check by main: the frozen debug binary starts on `D:\comfyui-test\v1`.

## Questions
1. Is the linker reserve (1) sufficient, or should compose additionally run on a dedicated large-stack thread?
2. Is (2) (8 MiB tokio worker stacks) warranted, given rescans after ComfyUI stop run on workers?
3. Any risk with `rustc-link-arg-bins` and the existing `rustc-link-arg-tests` manifest args / the bundler?
