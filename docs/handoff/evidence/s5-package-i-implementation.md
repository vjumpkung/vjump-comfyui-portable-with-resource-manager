M6 package I implementation report — 2026-10-20

Completed against `a4175d19f4544286e0c111850ec1e544e44f0183` on `feat/m0-skeleton`, with the pre-existing P0 working-copy changes retained. No commits, pushes, index/history changes, real setup, model/component downloads or GPU acceptance were performed.

**Per-file changes**

Locations below are repository-relative `file:line` references. The generated bindings were exported by the Rust test, never edited manually.

| File/location | Change |
| --- | --- |
| `crates/desktop/src/ipc/env_dto.rs:524` | Replaced the missing-manifest milestone promise with matching-manifest backup guidance; manifest recovery remains blocked. |
| `crates/desktop/src/ipc/env_dto.rs:706` | Added authored accelerator option metadata to profiles, preserving order, labels, defaults and provenance. |
| `crates/desktop/src/ipc/env_dto.rs:911` | Required accelerator selections and forwarded them unchanged, including explicit empty selections. |
| `crates/desktop/src/ipc/env_dto.rs:1104` | Added explicit stored-component/status conversions, retaining Manager metadata and all accelerator outcomes alongside the existing summary. Three DTO regression tests start at lines 1560, 1611 and 1636. |
| `crates/desktop/tests/acl.rs:76` | Added the required accelerator field to the valid setup request. |
| `crates/desktop/tests/acl.rs:752` | Extended recovery coverage to missing/inaccessible roots and the restricted-style rebuild fixture; checked actual supported handler results, fake opener plus restart, pickers, and existing origin/window denials. |
| `ui/src/shared/ipc/bindings.ts:156` | Regenerated accelerator options/results, component status, Manager/components metadata, and required profile/request/environment fields. Other type locations: 245, 419, 498, 561, 809, 1051. |
| `ui/src/shared/ipc/gateway.ts:117` | Added the three direct curated catalog/open/new wrappers through `unwrap`. |
| `ui/src/shared/ipc/gateway.test.ts:39` | Added all four required argument, payload, typed-error and subscription-channel tests; Manager consent remains the exact commit string. |
| `ui/src/shared/testing/environment.ts:43` | Added required empty accelerator options to the shared profile fixture. |
| `ui/src/features/setup/model.ts:73` | Made backend `failure.canSkip` mandatory for Skip. Added the React/gateway-only `CuratedReviewProps` contract at line 87. |
| `ui/src/features/setup/model.test.ts:29` | Updated the existing Skip-policy regression with explicit backend refusal. |
| `ui/src/features/setup/setup-wizard.tsx:99` | Initialized accelerator choices once per effective profile, retained explicit empty choices across ordinary refetches, and initialized curated selections once from a first-run-only catalog query. Added catalog loading/error/retry, authored metadata, review summaries, Repair retained-choice copy, exact requests at line 966, and removed stale milestone text/the whole Options alert. |
| `ui/src/features/setup/setup-wizard.test.tsx:195` | Added profile reset/refetch, explicit empty payload, catalog failure/refetch and Repair integration coverage; updated existing mocks and request assertions. |
| `ui/src/features/setup/run-progress.tsx:44` | Added ordered run/plan/binding-keyed batch approval, repeated exact retry decisions, restart re-review, dismissal/reopening and modal notices. Live reviews guard every retryable snapshot. All curated statuses/messages render from line 116. |
| `ui/src/features/setup/run-progress.test.tsx:108` | Added damaged optional-step refusal, live-review guarding and full retry/restart/session-invalidation coverage using an injected test renderer. |
| `ui/src/features/setup/rebuild-setup.tsx:14` | Added dedicated restricted-safe rebuild summary/action/progress. Preserved recorded metadata, tracked registered run IDs independently of transient mutation data, exposed existing failed/cancelled runs and fresh rebuilds, and retained restarting feedback. |
| `ui/src/features/setup/index.ts:1` | Exported the rebuild component and curated review contract through the public setup barrel. |
| `ui/src/features/custom-nodes/review-dialog.tsx:98` | Exported props and added optional setup labels and trust-memory suppression with existing M5 defaults. Kept binding resets, exact-source consent, warning acknowledgement, protected-change checks and busy behavior. Setup decisions keep trust memory false. |
| `ui/src/features/custom-nodes/review-dialog.test.tsx:153` | Added setup-only hidden trust controls, unchecked consent/warnings, exact unremembered decisions and custom-label coverage; retained all existing tests. |
| `ui/src/features/custom-nodes/index.ts:2` | Exported `ReviewDialog` and its props for app composition. |
| `ui/src/app/App.tsx:22` | Composed the real public review dialog with setup wording. Added missing/inaccessible-root actions at line 48, useful stable-code errors, cancellation/pending/restart handling, generic recovery separation, and rebuild-before-shell routing at line 105. Kept rebuild mounted through ready polling and cached refetch errors. Restricted/unknown screens mount neither settings synchronization nor the settings-backed theme toggle. |
| `ui/src/app/App.test.tsx:195` | Added the six required real-composition tests and four regressions for copied successful rebuild history, cached polling errors, rejected fresh-run retries, and generic manifest/bootstrap recovery. |
| `ui/src/app/shell/app-shell.tsx:101` | Replaced the milestone fallback with neutral unreachable-route copy; preserved the global CloseRequestDialog. |
| `ui/src/app/shell/app-shell.test.tsx:66` | Asserted actual Custom Nodes → ComfyUI → Overview routing. |
| `ui/src/features/environment/environment-page.tsx:258` | Rendered all stored accelerator outcomes, selected state, import result and messages; honest legacy empty metadata state. Preserved summary/Torch/CUDA/Triton metadata and removed both obsolete Environment references at lines 391 and 427. |
| `ui/src/features/environment/environment-page.test.tsx:92` | Added installed/failed/skipped, selected false, true/false/null imports, messages and legacy empty outcomes; updated required component fixture. |
| `docs/handoff/evidence/s5-package-i-implementation.md:1` | This implementation and verification report. |

`use-setup-run.ts`, settings, ComfyUI runtime, composition, command registration, capabilities, generated permission TOMLs, profile/domain/schema/artifact data and relocation guards required no changes.

**Final added DTO contract**

```ts
type AcceleratorOptionDto = {
  id: string
  name: string
  defaultSelected: boolean
  provenance: string
}
// Required additions:
// ProfileDto.accelerators: AcceleratorOptionDto[]
// SetupRequestDto.accelerators: string[]
// EnvironmentInfoDto.components: InstalledComponentsDto

type ComponentStatusDto = 'installed' | 'failed' | 'skipped'
type AcceleratorResultDto = {
  id: string
  selected: boolean
  status: ComponentStatusDto
  importable: boolean | null
  message: string | null
}
type ManagerResultDto = {
  selected: boolean
  status: ComponentStatusDto
  sourceCommit: string | null
  message: string | null
}
type InstalledComponentsDto = {
  manager: ManagerResultDto | null
  accelerators: AcceleratorResultDto[]
}
```

There is no invented experimental flag. An empty selection remains empty through DTO conversion. Empty stored results/Manager null mean unknown legacy metadata. The existing `EnvironmentInfoDto.accelerators: string[]` remains intact.

**Gateway payloads and composition**

| Gateway call | Exact generated-command arguments |
| --- | --- |
| `setupListCuratedNodes()` | `commands.setupListCuratedNodes()` |
| `setupOpenInstall(path)` | `commands.setupOpenInstall(path)` |
| `setupNewInstall(path)` | `commands.setupNewInstall(path)` |
| `setupRun(action, callback)` | `commands.setupRun(action, gatewayCreatedChannel)` |
| `nodeSubscribe(callback)` | `commands.nodeSubscribe(gatewayCreatedChannel)`; returns the unchanged subscription ID |

All wrappers use `unwrap`; typed errors preserve stable backend codes and transport details remain sanitized. Callers provide callbacks, never Channels. Ordinary first-run root/draft selection still uses `setupSelectRoot`; recovery uses only open/new. First-run new requests carry the selected accelerator/curated IDs; other modes send empty curated selections and Repair retains backend-recorded component choices.

Reviewed retries send `{ kind: 'retry', runId, managerConsent, curatedDecisions }`, one exact plan ID/binding decision per ordered review. The Manager field is an exact commit string or null. Selection itself produces no execution decision and calls neither nodeCommit nor nodeUpdateAll. Execution and warning acknowledgement start unchecked; detached warnings display verbatim. Decisions are process-local and only retained for the current run/ordered live batch. Empty reviews after restart allow an empty Retry to obtain new reviews, followed by fresh approval.

Guided rebuild sends this complete action; installRoot is intentionally absent:

```ts
{
  kind: 'new',
  request: {
    mode: 'rebuild',
    pythonSource: { kind: 'managed' },
    profileId: status.activeEnv.profileId,
    comfyuiRef: { kind: 'keepCurrent' },
    installManager: false,
    accelerators: [],
    curatedNodes: [],
    cleanCache: false,
    confirmAdoptedChanges: false,
    allowSourceBuilds: false,
  },
}
```

Those false/empty fields are wire placeholders. The backend derives rebuild choices, including cleanCache, from the successful recorded request before applying rebuild overrides. The UI requires activeEnv and presents no editable profile/Python/component choices.

**Tests added or updated**

- Rust DTO: `profile_options_preserve_component_labels_defaults_and_provenance`, `setup_request_forwards_explicit_accelerators`, `environment_info_preserves_stored_component_results`; updated `setup_actions_carry_the_selection_and_the_decisions_without_trust`.
- ACL: updated `recovery_handlers_keep_root_selection_diagnostics_and_pickers_available` and valid-body coverage. Actual existing error normalization is retained: setup_status exposes root_inaccessible, while blocked normal-service commands map it to recovery_required. Disabled update checks return updater_not_configured.
- Gateway: `setup_curated_catalog_takes_no_arguments`, `setup_root_recovery_sends_exact_paths`, `setup_run_forwards_accelerators_curated_selection_and_retry_decisions`, `node_subscription_keeps_tauri_channel_inside_gateway`.
- Wizard: `profile_change_resets_accelerator_selection`, `empty_accelerator_selection_remains_empty`, `catalog_failure_blocks_start_and_refetch_preserves_unchecked_nodes`, `repair_retains_recorded_component_choices_without_querying_curated_catalog`; updated existing typed request/root/validation tests.
- Progress: `damaged_optional_step_cannot_be_skipped`, `live_curated_reviews_guard_every_retryable_snapshot`, `curated_retry_resends_decisions_and_rereviews_after_restart`; preserved exact Manager consent and cancellation/reconnect tests.
- App: `curated_selection_does_not_authorize_execution`, `curated_binding_change_clears_consent`, `missing_root_offers_locate_and_new_install`, `rebuild_shows_preserved_data_and_owned_replacement`, `rebuild_uses_restricted_safe_ui_and_recovers_failed_run`, `rebuild_waits_for_controlled_restart_after_success`, `copied_successful_rebuild_requires_a_fresh_rebuild`, `rebuild_progress_survives_a_status_reconnect_error`, `fresh_rebuild_keeps_its_run_after_a_rejected_retry`, `manifest_recovery_keeps_the_generic_guidance_route`.
- Review dialog: `setup_review_hides_trust_memory_and_keeps_decisions_unremembered`; all existing M5 binding, warning, trust and busy tests pass.
- Environment: `environment_page_shows_stored_accelerator_results`; required info fixture updated. Shell navigation and existing model Skip tests updated.
- Preserved required B/P tests pass in the full suite: `bandwidth_empty_means_unlimited_and_zero_is_invalid`, `update_install_echoes_reviewed_update_id`, `diagnostics_copy_failure_exposes_read_only_fallback`. Configured-only update actions, plain-text release notes, both diagnostics-copy paths and read-only fallback remain covered by unchanged settings/runtime tests.

**Actual merged-tree verification**

Default pinned toolchain: `1.99.0-x86_64-pc-windows-msvc`; default `target/`. No RUSTUP_TOOLCHAIN/CARGO_TARGET_DIR overrides, no concurrent Cargo commands. Iterative Cargo checks used six jobs; requested final commands used four. Vitest used `$env:VITEST_MAX_WORKERS = '2'`.

| Exact command | Outcome |
| --- | --- |
| `cargo fmt --all -- --check` | PASS, exit 0 |
| `cargo clippy --workspace --all-targets --locked -j 4 -- -D warnings` | PASS, exit 0 |
| `cargo test --workspace --locked -j 4 --no-fail-fast` | PASS: **715 passed, 0 failed, 10 ignored** across 15 test/doc-test result groups |
| `cargo test -p desktop --test bindings --locked -j 4` | PASS: **1 passed, 1 ignored** export test |
| `cargo test -p desktop --test acl --locked -j 4` | PASS: **11 passed, 0 failed** |
| `pnpm -C ui lint` | PASS, exit 0; final touched App/rebuild files also passed focused ESLint after review corrections |
| `pnpm -C ui typecheck` | PASS, exit 0; final frontend build also ran `tsc -b` |
| `pnpm -C ui test` | PASS: **29 files, 183 tests**, 0 failures, 36.67 seconds |
| `pnpm -C ui build` | PASS, exit 0; 2067 modules transformed. Vite warned about the main 713.24 kB minified chunk exceeding 500 kB |
| `node --test scripts/check-arch-deps.test.mjs` | PASS: **10 tests**, 0 failures/skips |
| `node scripts/check-arch-deps.mjs` | PASS: `Architecture dependencies: OK` |
| `cargo build -p desktop --locked -j 4 --features tauri/custom-protocol` | PASS, exit 0; ran after the successful frontend build |
| `cargo test -p desktop --test bindings --locked -j 6 -- --ignored export_bindings` | PASS: **1 passed**, 1 filtered; generated contract export per mandatory environment rule |
| `cargo test -p desktop --lib --locked -j 6 ipc::env_dto::tests` | PASS: **6 passed**, 42 filtered |
| `git diff --check` | PASS; edited integration source files contain LF with no CR characters |

Focused UI checks also passed: setup 3 files/18 tests, App 1 file/10 tests, gateway + Environment 2 files/19 tests, review dialog + shell 2 files/14 tests. These are subsets of the final 183 tests and are not added to that total. The workspace suite includes existing restricted-composition/relocation tests and the P0 unit regressions. Ignored native smoke/real-setup tests were not explicitly run.

**Copy audit**

Ran the exact requested command:

```text
rg -n -i 'deferred|later milestone|M1|M2|M6|Coming in' ui/src crates/desktop/src/ipc/env_dto.rs
```

Inspected all **17 matching lines in four test files**: six model fixture filename lines in `library/model-list.test.ts`, one ANSI escape test string in `comfy-runtime/runtime-store.test.ts`, six deferred helper/use lines in `custom-nodes/custom-nodes-page.test.tsx`, and four deferred helper/use lines in `custom-nodes/update-all-dialog.test.tsx`. None is obsolete user-facing copy. The whole Options M1/M2 alert, both Environment M1 references, wizard running-process/update promises, App recovery promise, shell fallback, and backend missing-manifest promise have been removed or replaced accurately.

**Preservation, residuals and limits**

SHA-256 comparison before/after confirms byte-for-byte preservation of all six named P0 files: Cargo.lock, crates/desktop/Cargo.toml, lib.rs, app_lifecycle.rs, startup_tests.rs and tests/startup_smoke.rs. Pre-existing untracked handoff materials remain untouched. No relocation guard, backend availability policy, maintenance gate, production composition, capability or generated permission TOML changed.

D8 residual for main/R: a moved restricted installation cannot reopen its recorded old root through Locate, even if that original installation becomes available again. Supporting that requires a separately reviewed backend guard change. Locate/New are offered only for missing/inaccessible-root recovery; no arbitrary-install switcher or restart command was added. When a saved root becomes available at its original path, the UI instructs the user to restart; Recheck does not recompose rootless services.

RunSnapshotDto has no run-root field. The UI does not invent one or infer root identity from profileId. It exposes existing failed/interrupted rebuild inspection/retry, preserves a fresh rebuild option after backend refusal, and leaves historical-root validation to the backend. Copied historical non-rebuild runs are not automatically resumed.

No functional scope deviations remain. Settings-backed theme controls are omitted while status is unknown or recovery/rebuild may be restricted; ordinary setup/shell behavior is preserved. The ACL fixture tests existing normalized recovery codes and does not replace production moved-root composition tests. The Vite chunk-size warning remains. Native folder dialogs, actual controlled restarts, real WebView appearance/accessibility/zoom/long-path geometry, real downloads/setup and GPU acceptance were not exercised; these remain for R/native acceptance.
