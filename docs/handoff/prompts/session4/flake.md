ROLE: coder (write-capable). Diagnose and fix test flakiness. Do not change production behaviour unless the root cause is a real production race (then stop and report it instead of fixing it). Do not weaken assertions.

REPO: D:\vjump\vjump-comfyui-portable-with-resource-manager (Rust/Tauri 2; read AGENTS.md and docs/handoff/tooling/coder-env.md). Everything uncommitted is verified work from this session — keep it.

PROBLEM: tests in crates/infrastructure/src/comfy_runtime/process/tests.rs fail intermittently when the whole suite runs in parallel, and pass in isolation. Observed on 2026-10-19 after package E3 added more process-spawning tests (relocation probe / tool tests), with `cargo test --workspace --locked -j 6` and `cargo test -p infrastructure --lib`:
- `ready_owns_listener_releases_gate_and_stop_is_idempotent` panicked at tests.rs:348 (`f.supervisor.stop().await.unwrap()`), once in a full workspace run.
- `failed_cleanup_quarantines_the_gate_and_m1_maintenance` panicked at tests.rs:389, once in `cargo test -p infrastructure --lib --no-fail-fast`.
- The same suite passed on the next run; each test passes 5/5 alone. Neither the tests nor crates/infrastructure/src/comfy_runtime/** changed in this session (git diff HEAD is empty there). The fixture uses `ComfyRuntimeSupervisor::with_readiness_timeout(.., Duration::from_millis(700))` (tests.rs ~166-171) and several sleeps/deadlines (~122, 237, 262, 286-326).

TASK:
1. Find the root cause (timing assumptions under CPU load, shared global state such as ports, Job Objects, process inventory scanning the machine's processes, the maintenance gate, environment variables, or interference from other tests' child processes — e.g. E3's relocation probe/tool tests in crates/infrastructure/src/environment/relocation/** and environment/tools/tests.rs that spawn interpreters). Reproduce it: run `cargo test -p infrastructure --lib --locked -j 4 --no-fail-fast` repeatedly (and, if useful, with a parallel CPU-load generator) until it fails; record how often.
2. Fix the TESTS so they are deterministic under load without weakening what they assert (e.g. explicit synchronization instead of wall-clock races, a readiness timeout that is not the thing under test, unique ports/roots, serializing only the tests that share a real global resource with a documented reason). If the root cause is a production race in the supervisor (e.g. stop() returning an error under a legitimate slow-cleanup path), STOP and report it precisely with evidence instead of changing production code.
3. Prove stability: after the fix run `cargo test -p infrastructure --lib --locked -j 4 --no-fail-fast` at least 5 times in a row (report each result) and once under artificial CPU load if you used one.

SUCCESS CRITERIA (report each with counts):
- cargo fmt --all -- --check
- cargo clippy --workspace --all-targets --locked -j 4 -- -D warnings
- cargo test --workspace --locked -j 4 --no-fail-fast (before: 697 passed / 9 ignored on a good run)
- the 5x stability runs above

RULES: default MSVC toolchain into target/ (no RUSTUP_TOOLCHAIN/CARGO_TARGET_DIR); -j 4; never two cargo commands at once; LF line endings; no git write commands (git in test temp dirs is fine); only touch test files (and test-only helpers) unless you stop and report a production race.

OUTPUT: root cause with evidence, file:line changes, reproduction frequency before/after, exact command results.
