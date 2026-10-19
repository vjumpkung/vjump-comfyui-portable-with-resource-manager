ROLE: coder (write-capable). Mechanically integrate an already-verified package into the main working copy. Do not redesign or expand scope. If underspecified or conflicting with the code, stop that item and report the gap.

GOAL: integrate M6 package E1 (hashed accelerators, explicit selections, persisted component outcomes, pin safety) into main.

STATE: main applied `docs/handoff/patches/e1-accelerators-verified.patch` (verified on the older baseline commit 0ffa136) with `git apply --3way`. Files with conflicts contain markers (`<<<<<<< ours` = current main, `>>>>>>> theirs` = E1); main will list them in the message below this prompt. Since 0ffa136, main gained: M5 WP6 (runtime NodeLifecycle), WP1 (pinned containment, `InstallSpec.guarded: Option<GuardedRequirements>`, `CommandSpec.{stdin, retained}`), WP2+WP3, hardening H1/H2/H3 (H2: application `shared::Retained`, `MaintenanceLease::retained`, `StepContext.retained`, `RunError::InputFailed`, Windows DLL policy), WP4 (final digest/recovery), WP5 (`NodeRequirementSource`, `EnvironmentDeps.node_requirements`, fatal node requirement failure in every replacement mode, repair_check rules, `NodeAdapters` split), package B (bandwidth/HF relay) and package P (updater + diagnostics). Do not run git commands.

SPEC (authoritative): docs/handoff/plans/m6-approved-plan.md section "E1 - Hashed accelerators, selections, outcomes and pin safety"; docs/handoff/prompts/session2/e1.md (the original coder prompt; its worktree instructions no longer apply).

TASKS:
1. Resolve every conflict keeping BOTH sides' behaviour. In particular, in `crates/application/src/environment/{ports,deps,execute,setup}.rs` and tests: keep WP1's `InstallSpec.guarded` AND E1's `InstallSpec.no_deps` (default false) + `WheelFetcher` port + `EnvironmentDeps.wheels`; keep WP5's node-requirement provisioning (core requirements, then each node set, pins reasserted after every attempt, any node failure fatal) AND E1's accelerator steps (reassert original protected pins after EVERY attempted dependency mutation incl. failure/cancellation with a fresh token on the same lease, `ProvisionError::PinDrift` fatal even in BestEffort/Optional steps, final verification compares against recorded pins and can never re-baseline); keep H2's retention propagation in every new subprocess path E1 adds (wheel install, import probes). Every `InstallSpec { .. }` literal must set both fields. Fix all compile errors from the newer baseline with the smallest change, then make all E1 tests and all existing tests pass.
2. Migration: E1 adds `007_environment_components.sql` and registers it; make sure the migration list in crates/infrastructure/src/shared/sqlite.rs is ordered and no other migration uses number 007 (WP5 must not have added one; if a conflict exists, stop and report).
3. Diagnostics (package P, already merged): its accelerator-outcomes section shows an availability warning because E1 was missing. Now render the stored accelerator results from `EnvironmentRepository::load_components` / `InstalledComponents` for the active environment (id, selected, status, importable None = "untested", message), keeping the whole report redacted; adjust P's diagnostics tests accordingly.
4. Desktop: wire the concrete WheelFetcher into EnvironmentDeps in state.rs (reuse the shared downloader); add IPC error mapping arms for `accelerator_selection_invalid` and `profile_revision_unavailable` (keep `pin_drift`, `cancelled`). Do NOT add `SetupActionDto.accelerators` or UI (package I owns that) unless an existing DTO must change to compile; if `env_info` DTO gains component results, regenerate bindings via the export test and fix TypeScript compile errors with the smallest change (no new UI features).

TESTS (must all pass, names from E1): accelerator_specs_reject_invalid_artifacts, profile_v1_empty_and_v2_components_round_trip, wheel_cache_is_rehashed_before_reuse, wheel_hash_failure_never_runs_uv, accelerator_selection_is_explicit_and_profile_bound, accelerator_failure_reasserts_original_pins, optional_pin_drift_prevents_activation, final_verification_cannot_rebaseline_drift, accelerator_import_failure_is_reported_separately, components_survive_manifest_and_database_round_trip; plus all WP5 tests (repair_required_always_builds_a_replacement, replacement_node_dependency_failure_never_activates, ...) and diagnostics tests.

SUCCESS CRITERIA (run at the end, report each with counts):
- cargo fmt --all -- --check
- cargo clippy --workspace --all-targets --locked -j 4 -- -D warnings
- cargo test --workspace --locked -j 4 --no-fail-fast
- cargo test -p desktop --test bindings --locked -j 4 ; cargo test -p desktop --test acl --locked -j 4
- pnpm -C ui lint ; pnpm -C ui typecheck ; VITEST_MAX_WORKERS=2 pnpm -C ui test
- node --test scripts/check-arch-deps.test.mjs ; node scripts/check-arch-deps.mjs
- `grep -rn "<<<<<<<\|>>>>>>>" crates ui/src` finds nothing.

OUTPUT (final message): conflict resolutions (file:line), semantic merges of WP5 + E1 provisioning, diagnostics change, tests, exact command results, remaining gaps.


## Build environment and repository rules (mandatory)
- WORKING COPY: `D:\vjump\vjump-comfyui-portable-with-resource-manager` (the main repo, branch `feat/m0-skeleton`). The repo moved from `C:\Users\vjump\Documents\...`; any older path in plans/prompts/evidence means this directory. Worktrees under `C:\Users\vjump\vjc-wt\` no longer exist.
- All earlier verified work (M5 C0+FE, WP6, WP1, B) is committed in HEAD. Uncommitted changes in the working copy belong to the current package or to an earlier package of this session: preserve them; never revert files you did not change for this package.
- Read `docs/handoff/tooling/coder-env.md` and `AGENTS.md` first. Build with the default MSVC toolchain pinned by `rust-toolchain.toml` into the default `target/` dir; do NOT set RUSTUP_TOOLCHAIN or CARGO_TARGET_DIR. Use Git Bash or PowerShell. GNU env (`docs/handoff/tooling/gnu-env.sh`) is a fallback only if MSVC linking fails for toolchain reasons (report it).
- Memory: 32 GB machine, roughly 8-10 GB free. Pass `-j 4` to cargo build/test/clippy, prefer targeted `-p <crate>` runs (and test-name filters) while iterating, run the workspace-wide commands once at the end. Never run two cargo commands at once yourself. Vitest: `VITEST_MAX_WORKERS=2`.
- Rust: rustfmt, clippy `-D warnings`, no serde/tokio/reqwest/rusqlite/tauri in domain or application, ports via async-trait `Arc<dyn ...>`, composition only in crates/desktop/src/state.rs, errors per layer with thiserror. Match surrounding style and comment density.
- TypeScript: 2-space, single quotes, no semicolons, kebab-case files, Tauri access only via ui/src/shared/ipc/gateway.ts. Never hand-edit ui/src/shared/ipc/bindings.ts; regenerate only with `cargo test -p desktop --test bindings --locked -j 4 -- --ignored export_bindings`.
- Generated permission TOMLs under crates/desktop/permissions/autogenerated/ are produced by the build; keep new ones, ignore line-ending-only noise.
- Do not commit, push, stash, reset, checkout, or touch git history. Do not modify files outside the stated scope. If the spec is underspecified or conflicts with the code, stop that item and report the gap precisely instead of improvising.
- Git tests must actually execute (git is on PATH); a skipped `require_git!` is not evidence.

## Session 4 baseline note (main, 2026-10-19)
- HEAD a879fc0 contains WP2+WP3, P, H1, H2. Uncommitted in the working copy and verified by main (keep them; build on them): the first-run scaffold publication fix (crates/infrastructure/src/environment/source{.rs,/publish.rs,/tests.rs}: an app-created directory-only scaffold under <root>/ComfyUI is classified Absent and the ComfyUI clone is merged into it in place with a durable publication record) and every earlier package listed above. Checkpoints live in docs/handoff/patches/s4-*.patch (do not edit them).
- Codex runs with danger-full-access; still do not run any git write command (git inside test temp dirs is fine).
- Normalize every file you edit to LF line endings.
- Baseline before the E1 apply (main-verified): 615 Rust tests passed / 9 ignored; H3, WP4 and WP5 are in the working copy (checkpoint docs/handoff/patches/s4-05-through-wp5-verified.patch). Note WP5's actual shapes: `NodeRequirementSource::requirement_sets(&Path, Retained)`, `needs_repair(&EnvId, &Path, Retained)`, `record_outcome`, `NodeRequirementSet.file: Option<GuardedRequirements>`, mandatory `EnvironmentDeps.node_requirements`.

## Conflicts left by `git apply --3way docs/handoff/patches/e1-accelerators-verified.patch` (main, 2026-10-19)
Markers: `<<<<<<< ours` = current working copy (H3/WP4/WP5), `>>>>>>> theirs` = E1.
- crates/application/src/environment/deps.rs (1 hunk)
- crates/application/src/environment/execute.rs (2 hunks)
- crates/application/src/environment/ports.rs (1 hunk)
- crates/application/src/environment/tests/fakes.rs (1 hunk)
- crates/application/src/environment/tests/setup.rs (3 hunks)
- crates/infrastructure/src/environment/packages/tests.rs (1 hunk)
All other E1 files applied cleanly (new: crates/infrastructure/migrations/007_environment_components.sql, crates/infrastructure/src/environment/wheels.rs + wheels/, data/profiles/archive/). Migration list in crates/infrastructure/src/shared/sqlite.rs is 001..007 with no duplicate. Clean-applied files may still fail to compile against the newer baseline — fix with the smallest change.
