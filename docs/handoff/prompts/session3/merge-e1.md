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
