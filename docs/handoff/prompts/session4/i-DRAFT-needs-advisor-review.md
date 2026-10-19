ROLE: coder (write-capable). Mechanically implement the approved plan below. Do not redesign or expand scope. If underspecified or conflicting with the code, stop that item and report the gap precisely.

GOAL: M6 package I — integrate the remaining IPC contract and build the UI for E1 (accelerators), E2 (curated setup nodes), E3 (missing-root recovery and guided rebuild); finish the M5/P leftovers; remove obsolete copy. Backend composition for E1/E2/E3/P/B is already done by those packages; I owns the desktop IPC DTOs listed below and all frontend files.

APPROVED SPEC (authoritative): docs/handoff/plans/m6-approved-plan.md section "I — Integrate IPC, recovery composition and UI" (required integration + required UI tests). UI evidence: docs/handoff/evidence/exp-ui-i.md (file:line map of the wizard, run progress, environment page, custom-nodes review reuse, App/AppShell, stale copy, settings, gateway). Design system: docs/DESIGN.md (English copy only, NFR-UX-2). Frontend rules: AGENTS.md (2-space, single quotes, no semicolons, kebab-case files, Tauri access only via ui/src/shared/ipc/gateway.ts, ESLint feature boundaries — compose cross-feature UI in ui/src/app via public barrel exports, never feature-internal cross-imports).

## A. Desktop IPC contract additions (crates/desktop/src/ipc/{env_dto,env_commands,...}.rs; regenerate bindings ONLY via `cargo test -p desktop --test bindings --locked -j 4 -- --ignored export_bindings`)
1. `ProfileDto.accelerators: AcceleratorOptionDto[]` built from the profile's component specs (E1 `domain::environment` accelerator spec): `{ id, name, defaultSelected, experimental (true when the spec is non-default/unchecked, e.g. py312 wheels per m6 plan Q3), provenance }`. No URLs/hashes needed by the UI unless already exposed.
2. `SetupRequestDto.accelerators: string[]` mapped to `SetupRequest.accelerators` (E1; explicit selection, empty = none; unknown/duplicate ids are refused by the backend with `accelerator_selection_invalid`).
3. `EnvironmentInfoDto.components: AcceleratorResultDto[]` from `EnvironmentInfo.components` (the stored results E1 records, same source the diagnostics use): `{ id, selected, status ('installed'|'failed'|'skipped'|...), importable: boolean | null, message: string | null }`. Keep the existing `accelerators: string[]`.
4. Keep every existing command; verify ACL valid-body cases exist for all eight M6 commands (`setup_list_curated_nodes`, `setup_open_install`, `setup_new_install`, `app_update_status`, `app_update_check`, `app_update_install`, `app_diagnostics`, `app_copy_diagnostics`), including recovery-mode handlers; add any that are missing.

## B. Gateway (ui/src/shared/ipc/gateway.ts + gateway.test.ts)
- Add `setupListCuratedNodes()`, `setupOpenInstall(path)`, `setupNewInstall(path)` (and any other E3 command present in bindings) with exact payload tests.
- Add the missing M5 test `node_subscription_keeps_tauri_channel_inside_gateway` (the Channel is created inside the gateway; callers receive callbacks only).

## C. Setup wizard (ui/src/features/setup/*)
- Options stage: accelerator checklist from the selected profile's `accelerators` (defaults = `defaultSelected`; experimental ones labelled and unchecked by default); changing the profile resets the selection to that profile's defaults; an empty selection stays empty and is sent as `[]`.
- Options stage: curated-node list from `setupListCuratedNodes()` (FirstRun only), all selected by default, showing name, repository, branch and short commit; copy states that selection is not consent — every node is reviewed before anything runs.
- Review stage summarises accelerators and curated selection.
- Run progress: show `curated: CuratedProgressDto[]` (status incl. `failed`, message incl. the persisted install.py note). When the run is paused for curated consent (`failure.step === 'curated_nodes'` and `curatedReviews.length > 0`), present ONE batch review of all `curatedReviews` by reusing the M5 review dialog (export `ReviewDialog` — and any types it needs — from the custom-nodes public barrel `ui/src/features/custom-nodes/index.ts`, and inject it into the setup feature from `ui/src/app` composition, e.g. a render prop; no feature-internal cross-import). Every review carries warnings (incl. the "detached" warning) that must be acknowledged explicitly; consent starts unchecked; a changed binding clears consent. Retry sends `curatedDecisions: CommitInput[]` (one per plan, exact plan id + binding); decisions are sent again on every Retry (they are process-local in the backend). If the run shows the curated consent failure but `curatedReviews` is empty (backend restart), offer Retry to re-review.
- Skip uses `failure.canSkip` from the backend (remove the policy-based inference); never offer Skip when `canSkip` is false.
- Remove stale copy: "Updating ComfyUI is a later milestone" (setup-wizard.tsx ~671), optional accelerators/custom node packs "deferred" (~708-711).

## D. App / recovery (ui/src/app/App.tsx, ui/src/app/shell/app-shell.tsx + tests)
- E3 RECOVERY SURFACE (already in bindings.ts, implemented by E3): missing/inaccessible root = `SetupStatusDto.phase === 'recovery_required'` with `recovery.code` `'root_missing'` or `'root_inaccessible'` and `installRoot` = the recorded path (crates/desktop/src/state.rs ~120-160 `RecoveryInfo`); moved install / broken interpreter = `phase === 'rebuild_required'` with `recovery.code` `'moved'` or `'interpreter_broken'` (crates/desktop/src/ipc/env_dto.rs ~538); `setupOpenInstall(path)` / `setupNewInstall(path)` -> `RootSelectionDto { restartRequired }` (the backend requests the controlled restart); a rebuild is started with `setupRun({ kind: 'new', request: { mode: 'rebuild', ... } })` — the backend derives the immutable rebuild input (crates/application/src/environment/setup.rs ~70); send only what the backend requires for Rebuild and document which request fields it ignores. Other recovery codes (bootstrap_newer, bootstrap_inaccessible, no_data_directory, startup_failed, ...) keep the existing generic alert with Recheck.
- root_missing (and root_inaccessible: explain permissions; offer the same two actions): show the recorded path and two actions — Locate existing install (folder picker via the existing gateway dialog helper → `setupOpenInstall(path)`) and Create new install (folder picker → `setupNewInstall(path)`); on success show "Restart required" (the backend requests the controlled restart); map errors root_invalid, root_fixed_by_override, root_switch_failed, comfyui_running, comfyui_state_unknown, maintenance_busy, relocation_recovery_required to plain English messages.
- rebuild_required: guided-rebuild summary — reason (moved / interpreter broken), what is preserved (models, custom nodes, user/input/output, external model roots, launch options) and that a NEW app-owned environment replaces the old one without modifying it; a button starts the Rebuild setup run.
- Remove "deferred to M6" copy (App.tsx ~66) and "Coming in a later milestone" (app-shell.tsx ~107); update the app-shell test's absence assertion accordingly.

## E. Environment page (ui/src/features/environment/environment-page.tsx + test)
- Show stored accelerator results from `components`: id, selected, install status, import result (`importable null` = "untested") and message; keep Torch/CUDA/Triton rows.

## Required UI tests (exact names; keep the existing ones passing)
profile_change_resets_accelerator_selection, empty_accelerator_selection_remains_empty, curated_selection_does_not_authorize_execution, curated_binding_change_clears_consent, damaged_optional_step_cannot_be_skipped, missing_root_offers_locate_and_new_install, rebuild_shows_preserved_data_and_owned_replacement, bandwidth_empty_means_unlimited_and_zero_is_invalid (exists), update_install_echoes_reviewed_update_id (exists), diagnostics_copy_failure_exposes_read_only_fallback (exists), node_subscription_keeps_tauri_channel_inside_gateway, plus gateway payload tests for the new methods and `environment_page_shows_stored_accelerator_results`, `curated_retry_resends_decisions_and_rereviews_after_restart`.

## SUCCESS CRITERIA (run at the end, report each with counts)
- cargo fmt --all -- --check ; cargo clippy --workspace --all-targets --locked -j 4 -- -D warnings
- cargo test --workspace --locked -j 4 --no-fail-fast
- cargo test -p desktop --test bindings --locked -j 4 (after regeneration) ; cargo test -p desktop --test acl --locked -j 4
- pnpm -C ui lint ; pnpm -C ui typecheck ; VITEST_MAX_WORKERS=2 pnpm -C ui test ; pnpm -C ui build
- node --test scripts/check-arch-deps.test.mjs ; node scripts/check-arch-deps.mjs
- cargo build -p desktop --locked -j 4 --features tauri/custom-protocol (after pnpm -C ui build)

OUTPUT: per-file summary (file:line), DTO/gateway changes, UI tests added (names, results), exact command results with counts, deviations, anything not verifiable without a running app (e.g. visual checks).
