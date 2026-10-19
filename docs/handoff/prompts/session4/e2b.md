ROLE: coder (write-capable). Mechanically implement the corrections below to M6 package E2 (curated setup nodes), which is implemented and verified in the uncommitted working copy (checkpoint docs/handoff/patches/s4-07-through-e2-verified-review-pending.patch; 652 Rust tests pass). Do not redesign or expand scope; if a correction conflicts with the code, stop that item and report the gap precisely.

Background: a read-only review (thinker, standing in for the advisor) accepted E2's design and its six deviations, and required three corrections. Spec: docs/handoff/plans/m6-approved-plan.md section "E2 - Curated nodes through M5, before activation".

## Correction 1 — Skip must never hide published damage after a Retry
Files: crates/application/src/custom_nodes/setup.rs (~299-338), crates/application/src/environment/execute.rs (~1116-1121, ~1141-1218), crates/application/src/environment/setup.rs (~285-322), crates/application/src/environment/model.rs (CuratedStatus), infrastructure run codec if CuratedStatus is persisted there, desktop DTO mapping if CuratedStatusDto mirrors it (regenerate bindings only via the export test).
Failure path today: Damaged commit leaves the entry `Prepared` and consumes its plan; Retry → Preflight returns the Failed candidate to Provisioning and a fresh executor starts with damaged=false; if the requirements re-run does not make the node's outcome Ok (no requirements file, or contents differ from the reviewed tree), `installed()` is false → the entry is prepared again → the Git adapter returns `TargetExists` (infrastructure custom_nodes/files.rs ~437-441) → mapped to NotApplied → skippable, env not failed → `skip()` succeeds → `verify_curated` checks only Installed entries → the candidate activates with a published node that failed or was never reviewed in that state.
Required fix:
- Persist damage: add `CuratedStatus::Failed` and write it (with a fixed message) BEFORE returning on Damaged or PinDrift.
- In `prepare_curated`, never prepare an Install for an entry whose node is already published for this run (Failed, or any entry whose publication started): if `installed()` is true mark it Installed; otherwise fail NON-skippably (`can_skip = false`).
- `skip()` must refuse with `StepNotSkippable` while any curated entry is Failed, regardless of the environment state.
- `verify_curated` must also cover Failed entries (a Failed entry prevents activation).
Tests (exact names): `curated_retry_cannot_skip_unrepaired_published_damage` (damage a node, retry with the requirements re-run NOT repairing it → failure not skippable, `skip()` returns StepNotSkippable, no activation, old active env untouched) and `curated_retry_recognises_a_repaired_node_and_activates` (retry where the re-run succeeds → entry recognised as Installed, no re-clone, activation proceeds). Also strengthen `skip_cannot_hide_published_dependency_damage` to cover the retry-then-skip path.

## Correction 2 — `publication_started` must fail closed
File: crates/application/src/custom_nodes/setup.rs (~217-226). `Err(_) => false` turns a storage error into "not published" (skippable). Treat as published: any error other than a definite NotFound, and any `Failed` operation whose node record still exists in the store (a roll-forward settled as Failed with dependencies NotRun is still published).
Tests: `publication_started_fails_closed_on_storage_errors` and `failed_operation_with_a_stored_node_counts_as_published`.

## Correction 3 — install.py guidance must describe a working path (main's decision)
The current text (execute.rs ~1132 Warning and `SETUP_HOOK_WARNING` in crates/application/src/custom_nodes/workflow.rs:26) tells the user to use "Retry dependencies", but the UI disables Retry once dependencies are `ok` (ui/src/features/custom-nodes/model.ts `retryUnavailableReason`). Main's decision: do NOT add a new signal/DTO field/migration; change the backend text to an accurate path:
- SETUP_HOOK_WARNING: "This node has an install.py. Setup never runs it. If the node needs it, uninstall the node in Custom Nodes and install it again, approving install.py in the review."
- The curated step Warning names the affected nodes and carries the same guidance.
- Persist the hook note per entry in `CuratedProgress.message` so a Retry (which re-runs the step with nothing newly committed) does not erase it; the step's final status stays Warning while any installed entry has a pending hook note.
Tests: extend `curated_setup_never_runs_install_py` to assert the new guidance text and that the note survives a Retry (`curated_hook_note_survives_retry`).

## Also fix (test strength, no behaviour change)
- `setup_actions_carry_the_selection_and_the_decisions_without_trust`: also assert `remember_install_py`, `remember_source_trust` (or the equivalent remember flags) and `run_install_py` are false for setup decisions after `commit_for_setup_held`.

## SUCCESS CRITERIA (run at the end, report each with counts)
- cargo fmt --all -- --check
- cargo clippy --workspace --all-targets --locked -j 4 -- -D warnings
- cargo test --workspace --locked -j 4 --no-fail-fast (before: 652 passed, 0 failed, 9 ignored)
- cargo test -p desktop --test bindings --locked -j 4 ; cargo test -p desktop --test acl --locked -j 4
- If a DTO changed: regenerate bindings via `cargo test -p desktop --test bindings --locked -j 4 -- --ignored export_bindings`, then pnpm -C ui typecheck / lint / VITEST_MAX_WORKERS=2 pnpm -C ui test
- node scripts/check-arch-deps.mjs

OUTPUT: per-file summary (file:line), tests added/changed (names, results), exact command results with counts, gaps.

## Build environment and repository rules (mandatory)
- WORKING COPY: D:\vjump\vjump-comfyui-portable-with-resource-manager (branch feat/m0-skeleton, HEAD a879fc0; everything uncommitted is verified earlier work — keep it). Build with the default MSVC toolchain from rust-toolchain.toml into target/; do not set RUSTUP_TOOLCHAIN or CARGO_TARGET_DIR. Pass -j 4 to cargo; never run two cargo commands at once; use targeted `-p <crate> <filter>` runs while iterating. Cargo calls can take several minutes; use generous timeouts.
- Rust: rustfmt, clippy -D warnings, no serde/tokio/reqwest/rusqlite/tauri in domain or application, match surrounding style and comment density. Normalize edited files to LF.
- Never hand-edit ui/src/shared/ipc/bindings.ts. Do not commit, push, stash, reset, checkout, apply, clean or touch git history (git inside test temp dirs is fine). Do not edit docs/handoff/patches.
