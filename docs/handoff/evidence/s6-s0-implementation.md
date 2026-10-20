# S0 implementation report: 8 MiB main-thread and worker stacks

Package S0 (native P0: debug desktop exe overflowed the 1 MiB Windows main-thread stack at startup once a custom node was installed). Implemented exactly as the advisor-approved plan (`docs/handoff/prompts/session6/s0-stack-reserve.md`, `docs/handoff/plans/s6-s0-stack-overflow.md`). Default MSVC toolchain, default `target/`, `-j 6`, no git write commands.

## Per-file summary

| File | Change |
| --- | --- |
| `crates/desktop/build.rs:113-121` | New block after the unchanged MSVC manifest block. On Windows: `msvc` emits `cargo:rustc-link-arg-bins=/STACK:8388608`; `gnu`/`gnullvm` emit `-Wl,--stack,8388608`. `-bins` reaches only the `[[bin]]` target, never test executables. |
| `crates/desktop/Cargo.toml:33` | tokio features now `["sync", "macros", "time", "rt-multi-thread"]`. No other dependency change: `windows-sys` already has `Win32_System_Threading` through the workspace (`Cargo.toml:42`), so the dev-dependency is untouched. |
| `crates/desktop/src/lib.rs:23` | `pub(crate) const WORKER_STACK_BYTES: usize = 8 * 1024 * 1024;` |
| `crates/desktop/src/lib.rs:25-29` | `pub(crate) fn async_runtime_builder()`: `Builder::new_multi_thread()` + `enable_all()` + `thread_stack_size(WORKER_STACK_BYTES)`. |
| `crates/desktop/src/lib.rs:31-40` | `fn install_async_runtime()`: process-lifetime `static RUNTIME: OnceLock<Runtime>`, then `tauri::async_runtime::set(handle)`. |
| `crates/desktop/src/lib.rs:42-45` | `install_async_runtime();` is the first statement of `run()` (before `initialize_tool_policy` and the builder), with a comment. Not called from `run_with_runtime`, so the startup tests keep the default runtime. |
| `crates/desktop/src/startup_tests.rs:107-126` | `#[cfg(windows)] #[test] async_runtime_workers_and_blocking_threads_reserve_eight_mib`: builds `crate::async_runtime_builder()`, measures `GetCurrentThreadStackLimits` (`high - low`) on a spawned worker task and on a `spawn_blocking` thread, asserts both `>= WORKER_STACK_BYTES`. Never calls `install_async_runtime()`. |
| `crates/desktop/tests/stack_reserve.rs` (new) | `#![cfg(windows)]`. `desktop_exe_reserves_an_eight_mib_main_stack` reads `CARGO_BIN_EXE_vjumpkung-comfyui-desktop` and asserts reserve `>= 8 MiB`. `test_executables_keep_the_default_stack_reserve` reads `current_exe()` and asserts `< 8 MiB`. `fn stack_reserve(&[u8]) -> Result<u64, String>` is std-only: checked `image.get(a..b)` through a small `bytes::<N>` helper plus `from_le_bytes`; checks `MZ`, `e_lfanew @ 0x3C`, `PE\0\0`, `SizeOfOptionalHeader @ pe+20 >= 80`, then magic `0x20B` -> u64 @ opt+72, `0x10B` -> u32 @ opt+72 widened, else error. |
| `docs/handoff/evidence/s3-residuals.md:28` | Row 22 appended (package S0, text as specified; evidence column lists the changed files and the plan; status Fixed). |

## Key diffs

`crates/desktop/build.rs`
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

`crates/desktop/src/lib.rs`
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
// run(): first statement is install_async_runtime();
```
(rustfmt wrapped the `get_or_init` closure body over several lines.)

## Commands run and results

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | exit 0 (run before and after the docs row; `cargo fmt --all` had reformatted only my `startup_tests.rs` addition; `git status` shows no other file touched) |
| `cargo clippy --workspace --all-targets --locked -j 6 -- -D warnings` | exit 0, no warnings (run twice: after the code changes and at the end) |
| `cargo test -p desktop --locked -j 6 --test stack_reserve` | 2 passed; 0 failed; 0 ignored |
| `cargo test -p desktop --locked -j 6 --lib async_runtime_workers` | 1 passed; 0 failed; 48 filtered out |
| `cargo test --workspace --locked -j 6 --no-fail-fast` | exit 0. 722 passed; 0 failed; 10 ignored. Per target: application 198, desktop lib 49, desktop bin 0, acl 11, bindings 1 (+1 ignored), navigation 2, stack_reserve 2, startup_smoke 0 (+1 ignored), domain 45, infrastructure lib 405 (+7 ignored), real_setup 0 (+1 ignored), workflow 9, doc-tests 0. |
| `node scripts/check-arch-deps.mjs` | `Architecture dependencies: OK` (exit 0) |

Baseline was 719 passed / 0 failed / 10 ignored; delta +3 = 2 PE tests + 1 worker-stack test; ignored count unchanged at 10.

## Verification notes

- The PE test proves the debug `vjumpkung-comfyui-desktop.exe` now carries `SizeOfStackReserve >= 8 MiB`, and the second test proves `-bins` scoping: the integration-test executable (which links with the MSVC default) stays below 8 MiB, so the parser is reading the real field rather than returning a constant.
- Library unit tests (`desktop-*.exe`) and the other integration tests are unaffected, since `rustc-link-arg-bins` applies only to the `[[bin]]` target.
- Native verification on the owner root (the original exit-127 repro with ComfyUI-Manager installed) was not run here; per the package spec, main verifies that. These tests lock the reserved sizes, not actual stack usage.

## Deviations

None from the approved plan. Only cosmetic: rustfmt line-wrapping of the `get_or_init` closure and of the two `spawn` calls in the new startup test; the comment at the `install_async_runtime();` call site is two lines (the plan's short comment plus the "must come first" reason). During the workspace test run, cargo printed "Blocking waiting for file lock on build directory" once (another cargo process outside this package held the lock); my commands ran strictly sequentially. The pre-existing working-copy changes (package Q and session-6 docs) were not touched.
