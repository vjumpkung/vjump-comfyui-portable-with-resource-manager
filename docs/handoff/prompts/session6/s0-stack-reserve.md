ROLE: coder (write-capable). Mechanically implement the advisor-approved plan below. Do not redesign or expand scope. If the plan is underspecified or conflicts with the code, stop that item and report the gap precisely instead of improvising.

## Build environment and repository rules (mandatory)
- WORKING COPY: `D:\vjump\vjump-comfyui-portable-with-resource-manager` (branch `feat/m0-skeleton`, HEAD `e384d43`). Uncommitted working-copy changes = verified package Q (`docs/handoff/patches/s6-01-q-verified.patch`) + session-6 docs: preserve them; never revert files you did not change for this package.
- Read `AGENTS.md` and `docs/handoff/tooling/coder-env.md` first. Default MSVC toolchain from `rust-toolchain.toml`, default `target/`; do NOT set RUSTUP_TOOLCHAIN or CARGO_TARGET_DIR.
- Memory: 32 GB machine, ~10 GB free. Use `-j 6`; targeted `-p desktop` runs while iterating; workspace-wide commands once at the end. Never run two cargo commands at once.
- Rust: rustfmt, clippy `-D warnings`; match surrounding style and comment density. Normalize edited files to LF.
- Do not commit, push, stash, reset, checkout, apply, clean or touch git history. Do not modify files outside the stated scope. Do not touch `D:\vjc-s6\` or `D:\comfyui-test\`; do not kill `vjumpkung-comfyui-desktop` processes.

## Problem (native P0, verified by main 2026-10-20 12:40; plan `docs/handoff/plans/s6-s0-stack-overflow.md`)
The debug desktop binary started on a root that has an installed custom node (ComfyUI-Manager) dies with `thread 'main' has overflowed its stack` (exit 127) during startup composition (`crates/desktop/src/lib.rs:97-114`, `tauri::async_runtime::block_on(AppState::compose(..))` → node reconcile/rescan on the main thread; Windows default main-thread reserve = 1 MiB). The same exe with only `editbin /STACK:8388608` starts normally. The same rescan chain also runs on Tauri's default tokio workers (2 MiB) for `node_rescan` and the rescan after ComfyUI stops.

## Approved plan (advisor stand-in ruling S0: GO)
1. `crates/desktop/build.rs` — add a separate block after the existing MSVC manifest block (keep that block unchanged):
```rust
// Debug startup (compose -> node reconcile/rescan) and shutdown run block_on on the main
// thread; the 1 MiB Windows default overflows. Keep in sync with tests/stack_reserve.rs.
if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
    match std::env::var("CARGO_CFG_TARGET_ENV").as_deref() {
        Ok("msvc") => println!("cargo:rustc-link-arg-bins=/STACK:8388608"),
        Ok("gnu") | Ok("gnullvm") => println!("cargo:rustc-link-arg-bins=-Wl,--stack,8388608"),
        _ => {}
    }
}
```
   (Linux/macOS main threads already get 8 MiB.) `-bins` reaches only the `[[bin]]` target, never tests.
2. `crates/desktop/Cargo.toml` — tokio features become `["sync", "macros", "time", "rt-multi-thread"]` (rt-multi-thread currently arrives only through Tauri). No other new dependency (dev-dependency `windows-sys` already has `Win32_System_Threading` through the workspace — verify; if not, add only that feature to the desktop dev-dependency).
3. `crates/desktop/src/lib.rs`:
```rust
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
        async_runtime_builder().build().expect("the async runtime could not start")
    });
    tauri::async_runtime::set(runtime.handle().clone());
}
```
   Call `install_async_runtime();` as the FIRST statement of `run()` (before `initialize_tool_policy` and the builder; anything touching `tauri::async_runtime` first would create the default runtime and `set` would panic). Do NOT call it from `run_with_runtime` (startup_tests rely on the default runtime). Short comment at the call site: worker threads run node rescans; debug builds need more than tokio's 2 MiB default.
4. New `crates/desktop/tests/stack_reserve.rs` with `#![cfg(windows)]` (not gated on debug_assertions):
   - `desktop_exe_reserves_an_eight_mib_main_stack`: `std::fs::read(env!("CARGO_BIN_EXE_vjumpkung-comfyui-desktop"))` → `stack_reserve(&image) >= 8 * 1024 * 1024`.
   - `test_executables_keep_the_default_stack_reserve`: `std::env::current_exe()` → reserve `< 8 MiB` (proves the parser reads the real field and `-bins` scoping).
   - `fn stack_reserve(image: &[u8]) -> Result<u64, String>` std-only with checked `image.get(a..b)` + `from_le_bytes`: require `MZ` at 0; `e_lfanew = u32 @ 0x3C`; require `PE\0\0` at e_lfanew; `SizeOfOptionalHeader = u16 @ pe+20` ≥ 80; `opt = pe + 24`; `magic = u16 @ opt`: `0x20B` → `u64 @ opt+72`; `0x10B` → `u32 @ opt+72` widened; else error.
5. `crates/desktop/src/startup_tests.rs` — `#[cfg(windows)] #[test] fn async_runtime_workers_and_blocking_threads_reserve_eight_mib()`: build `crate::async_runtime_builder().build()`; `reserve()` calls `windows_sys::Win32::System::Threading::GetCurrentThreadStackLimits(&mut low, &mut high)` and returns `high - low`; assert `rt.block_on(rt.spawn(async { reserve() }))` and `rt.block_on(rt.spawn_blocking(reserve))` are both `>= WORKER_STACK_BYTES`. Never call `install_async_runtime()` in tests.
6. No smoke-test change (the node-tree smoke test was ruled optional and is NOT part of this package; main verifies natively on the owner root).
7. Docs: append one row to `docs/handoff/evidence/s3-residuals.md` (next free number): "(native, S0) Debug builds overflowed the 1 MiB Windows main-thread stack during startup node reconcile once a custom node was installed (exit 127, owner-visible as a crash). Fixed in S0: 8 MiB main-thread reserve (/STACK) and 8 MiB tokio worker stacks; PE and worker tests lock the sizes, not the actual usage."

## Success criteria (run at the end; report exact counts)
```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -j 6 -- -D warnings
cargo test -p desktop --locked -j 6 --test stack_reserve
cargo test -p desktop --locked -j 6 --lib async_runtime_workers
cargo test --workspace --locked -j 6 --no-fail-fast
node scripts/check-arch-deps.mjs
```
Baseline before your change: Rust 719 passed / 0 failed / 10 ignored.
OUTPUT: per-file summary with file:line, key diffs, every command result with counts, deviations with reasons. Also write it to `docs/handoff/evidence/s6-s0-implementation.md`.
