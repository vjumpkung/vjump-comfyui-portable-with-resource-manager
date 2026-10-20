## Build environment and repository rules (mandatory)
- WORKING COPY: `D:\vjump\vjump-comfyui-portable-with-resource-manager` (branch `feat/m0-skeleton`, HEAD `a4175d1`). The uncommitted working-copy changes are the verified P0 startup fix (crates/desktop/src/lib.rs, app_lifecycle.rs, startup_tests.rs, tests/startup_smoke.rs, Cargo.toml/Cargo.lock): preserve them; never revert files you did not change for this package.
- Read `docs/handoff/tooling/coder-env.md` and `AGENTS.md` first. Default MSVC toolchain from `rust-toolchain.toml`, default `target/`; do NOT set RUSTUP_TOOLCHAIN or CARGO_TARGET_DIR.
- Memory: 32 GB machine, ~10 GB free. Use `-j 6` for cargo, targeted `-p <crate>` runs and test filters while iterating, workspace-wide commands once at the end. Never run two cargo commands at once. Vitest: `VITEST_MAX_WORKERS=2`.
- Never hand-edit ui/src/shared/ipc/bindings.ts; regenerate only with `cargo test -p desktop --test bindings --locked -j 6 -- --ignored export_bindings`. Keep generated permission TOMLs.
- Normalize edited files to LF (`.gitattributes` = `* text=auto`).
- Do not commit, push, stash, reset, checkout, apply, clean or touch git history.


GOAL: M6 package I — finish the desktop IPC contract and integrate the UI for E1 accelerators, E2 curated setup nodes, and E3 missing-root recovery/guided rebuild; preserve the completed B/P settings integration and M5 behavior; remove obsolete milestone copy.

BASE / PRECONDITION:
- Reviewed baseline: a4175d19f4544286e0c111850ec1e544e44f0183, which contains the committed session-4 E1/E2/E3 work.
- A separate P0 package runs before I and fixes startup in crates/desktop/src/lib.rs, with its regression test. Assume P0 is applied; preserve it. Do not edit lib.rs or take ownership of that fix.
- HANDOFF.md and exp-ui-i.md contain historical statements predating committed E2/E3 work. Use current code and regenerated bindings as contract evidence.
- Do not run real setup/download/GPU acceptance as part of I. That belongs to R.

APPROVED SPEC:
docs/handoff/plans/m6-approved-plan.md, section “I — Integrate IPC, recovery composition and UI”, lines 675–717.
Read AGENTS.md, HANDOFF.md §0a/§3/§5.3 row I, docs/handoff/evidence/exp-ui-i.md, docs/handoff/evidence/s4-reviews.md, and docs/DESIGN.md.

RULES:
- English UI copy; existing design tokens/components, accessible controls, wrapping paths.
- TypeScript: two spaces, single quotes, omitted semicolons.
- Tauri/native access and generated-binding imports stay inside shared/ipc/gateway.ts.
- Features may import shared code and themselves, never another feature. App composes feature reuse through public barrels.
- Regenerate bindings through the Rust export test; never edit generated bindings manually.
- Keep current commands, ACL restrictions, maintenance-gate ownership and restricted composition.
- Scope is integration. Do not redesign relocation, node execution, profile schemas, settings, updater policy, or packaging.

Expected file scope:
- crates/desktop/src/ipc/env_dto.rs and its unit tests
- crates/desktop/tests/acl.rs
- ui/src/shared/ipc/{gateway,gateway.test}.ts
- generated ui/src/shared/ipc/bindings.ts
- ui/src/shared/testing/environment.ts and directly affected DTO fixtures
- ui/src/features/setup/{model,setup-wizard,run-progress,use-setup-run,index}.*
  and their tests as needed
- new ui/src/features/setup/rebuild-setup.tsx if useful for the dedicated rebuild path
- ui/src/features/custom-nodes/{index.ts,review-dialog.tsx}
  and directly relevant dialog tests
- ui/src/app/App.tsx, new ui/src/app/App.test.tsx,
  ui/src/app/shell/{app-shell.tsx,app-shell.test.tsx}
- ui/src/features/environment/{environment-page.tsx,environment-page.test.tsx}
- Other frontend call sites only when directly required by the new required DTO fields.
- Existing settings/comfy-runtime integration is verification scope; edit only a demonstrated integration regression.

## A. Desktop IPC contract additions

Primary file: crates/desktop/src/ipc/env_dto.rs.

1. Add ProfileDto.accelerators with this exact generated shape:

   type AcceleratorOptionDto = {
     id: string
     name: string
     defaultSelected: boolean
     provenance: string
   }

   ProfileDto.accelerators: AcceleratorOptionDto[]

   Map directly from SetupProfile.accelerators:
   id, name, default_selected, provenance.
   Preserve catalog order and exact authored labels/provenance.

   Do not add an experimental boolean:
   AcceleratorSpec has no experimental field (domain/environment/profile.rs:90–99).
   Do not infer experimental from !default_selected or ProfileStatus.
   Current py312 option names already include “(experimental)” and their provenance
   explains the lack of app validation. The containing py312 profile is stable.
   Display those names/provenance faithfully. Current py313/314 empty lists stay empty.
   No profile/domain/schema or artifact-data changes.

2. Add required SetupRequestDto.accelerators: Vec<String>,
   generated as accelerators: string[].

   In SetupActionDto::into_application, replace the current
   accelerators: Vec::new() at env_dto.rs:934 with request.accelerators.

   An explicit [] selects none. Do not substitute defaults in the DTO conversion.
   Unknown/duplicate IDs remain backend validation:
   application/environment/setup.rs:203–207,
   code accelerator_selection_invalid.

3. Add EnvironmentInfoDto.components with this exact nested shape:

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

   EnvironmentInfoDto.components: InstalledComponentsDto

   Implement explicit From conversions from application types.
   Use serde camelCase for fields and snake_case for the status enum.
   Copy EnvironmentInfo.components, including Manager metadata and every accelerator
   result, without filtering or recomputing it.
   Preserve EnvironmentInfoDto.accelerators: string[].

   Evidence:
   application/environment/model.rs:572–602;
   application/environment/queries.rs:214–224,247–296.
   Empty accelerator results / manager null represent unknown legacy metadata;
   they do not prove an installation has no optional components.

4. Add/extend env_dto.rs tests:

   - profile_options_preserve_component_labels_defaults_and_provenance
     Include a stable profile containing an experimental-labelled unchecked option;
     assert exact name, provenance and defaultSelected, plus an empty profile list.
   - setup_request_forwards_explicit_accelerators
     Assert nonempty and explicitly empty selections survive into SetupRequest.
   - environment_info_preserves_stored_component_results
     Cover all three status values, importable true/false/null, selected false,
     messages, Manager metadata, and the legacy empty shape.

   Update the existing setup_actions_carry_the_selection_and_the_decisions_without_trust
   request literal for the required accelerators field.

5. Preserve existing composition and command registration.

   Already implemented:
   - NodeAdapters / NodeRequirements / EnvironmentDeps / NodeService /
     separate SetupNodes injection:
     state.rs:885–935,955–977 and Services::new around 675–698.
   - Rootless gate and restricted setup access:
     state.rs:724–790.
   - Restricted composition only for a moved root:
     state.rs:868–875.
   - Normal DownloadStack/LibraryStack construction occurs after the restricted return:
     state.rs:979–1029.
   - All eight M6 commands are already in bindings.rs/build.rs.

   Do not reconstruct or duplicate these services.

6. ACL integration: crates/desktop/tests/acl.rs.

   Normal valid-body cases already exist for all eight commands:
   setup_list_curated_nodes,
   setup_open_install,
   setup_new_install,
   app_update_status,
   app_update_check,
   app_update_install,
   app_diagnostics,
   app_copy_diagnostics.

   Add accelerators: [] to the valid setup_run request body around lines 66–83.

   Extend recovery_handlers_keep_root_selection_diagnostics_and_pickers_available
   (around line 751), preserving origin/window denial checks.
   Cover root_missing, root_inaccessible and the existing restricted-style
   rebuild_required recovery fixture.

   In recovery, assert actual supported behavior rather than requiring every
   command to succeed:
   - setup_open_install / setup_new_install:
     fake opener and controlled restart are both reached.
   - app_diagnostics / app_copy_diagnostics:
     available.
   - app_update_status:
     available; disabled test updater reports configured false.
   - app_update_check:
     reaches the handler; disabled updater returns updater_not_configured.
   - app_update_install:
     valid updateId/onEvent body reaches the handler and returns the recovery code,
     because this command requires normal services.
   - setup_list_curated_nodes:
     reaches the handler and returns the recovery code, because it requires
     normal services.
   - Keep setup_status and picker assertions.

   Do not grant additional capabilities or change backend availability policies.
   Existing production restricted-composition tests remain the evidence for actual
   moved-root composition; the ACL fixture is not a substitute for them.

7. Remove obsolete backend UI copy in recovery_message:
   env_dto.rs:524 currently promises guided rebuild “in a later milestone”.
   ManifestMissing still requires manifest recovery; replace the promise with accurate
   restore-manifest guidance. Do not make missing/corrupt manifests rebuildable.

8. Regenerate bindings after Rust changes:

   cargo test -p desktop --test bindings --locked -j 4 -- --ignored export_bindings

## B. Gateway

Files: ui/src/shared/ipc/gateway.ts and gateway.test.ts.

Add precisely these existing generated methods through unwrap:
- setupListCuratedNodes() -> commands.setupListCuratedNodes(), no arguments.
- setupOpenInstall(path: string) -> commands.setupOpenInstall(path).
- setupNewInstall(path: string) -> commands.setupNewInstall(path).

The generated bindings already contain these commands at bindings.ts:18,21–22.
No additional E3 command is needed. Keep setupSelectRoot as the ordinary first-run
root/draft operation; recovery must use the separate open/new methods.

Required gateway tests:
- setup_curated_catalog_takes_no_arguments
- setup_root_recovery_sends_exact_paths
- setup_run_forwards_accelerators_curated_selection_and_retry_decisions
- node_subscription_keeps_tauri_channel_inside_gateway

Assert exact generated-command arguments, unchanged returned DTOs, and existing
typed-error behavior. For subscription, mock nodeSubscribe, verify gateway-created
Channel event forwarding and the returned subscription ID; callers supply callbacks
and never create or receive a Channel.

Keep the existing updater, diagnostics and setup Channel tests passing.

## C. Setup wizard

Files: setup/model.ts, setup-wizard.tsx, run-progress.tsx,
use-setup-run.ts if required, index.ts, tests and fixtures.

Accelerators:
- Options use the effective selected profile’s accelerators.
- Initialize from defaultSelected.
- Reset when the effective profile changes; do not reset user choices on a routine
  query refetch/rerender.
- Distinguish uninitialized selection from an explicit empty selection.
- Send the exact current IDs, including [].
- Show authored experimental labels and provenance. Do not claim validation.
- Empty-profile lists show an appropriate empty state.
- Review summarizes the selection.
- Repair uses recorded selections in the backend (setup.rs:158–191); do not present
  editable options as though they configure Repair. Explain that recorded component
  choices are retained. Rebuild follows the dedicated contract in section D.

Curated catalog:
- Query setupListCuratedNodes only for first_run, not rebuild/recovery/re-setup.
- Initially select all returned catalog entries once; refetch must not reselect
  entries the user unchecked.
- Show name, repository, informational branch and short pinned commit.
- Provide loading/error/retry behavior; do not silently turn catalog failure into
  an empty successful selection.
- Review summarizes the selected nodes.
- First-run request sends selected curatedNodes; other modes send [].
- Copy must say selection is not execution consent: earlier setup steps run first;
  every actual curated plan is reviewed before any curated node is committed.
  Do not claim that no setup work runs before the review.

Review-dialog composition:
- Export ReviewDialog and its props type from
  ui/src/features/custom-nodes/index.ts.
- In setup, define/export a CuratedReviewProps contract using only React and gateway
  types, structurally containing:
    plans: PlanReviewDto[]
    busy: boolean
    onCommit: (items: CommitInput[]) => void
    onClose: () => void
    onCancel: (() => void) | null
    notice?: ReactNode
- Add renderCuratedReview: (props: CuratedReviewProps) => ReactNode to the setup
  composition seam; thread it from SetupWizard to RunProgress.
- ui/src/app/App.tsx imports ReviewDialog through the custom-nodes barrel and
  supplies the renderer. Setup imports no custom-nodes files, barrel, models or
  fixtures, including in tests.
- Reuse ReviewDialog directly; do not use UpdateAllDialog, which prepares active-env
  operations. Do not call nodeCommit/nodeUpdateAll for setup reviews.

Minimal setup-specific dialog options:
- Add optional title, commitLabel and closeLabel props with current M5 defaults.
  App supplies curated/setup wording and “Back to setup” for close.
- Add allowRememberSource?: boolean, default true for M5.
  App supplies false for setup; hide source/hook trust-memory controls and keep
  rememberSource/rememberInstallPy false in setup decisions.
  Backend explicitly ignores these permissions during setup
  (application/custom_nodes/setup.rs:137–151).
- Keep existing source execution consent, warning rendering/acknowledgement, exact
  binding checks and M5 defaults.
- Setup hasInstallPy is false by backend contract; do not offer script execution.
  Display the supplied warnings and persisted install.py guidance.

Run progress:
- Render all run.curated entries and statuses:
  pending, prepared, failed, installed, skipped.
- Display each message, including persisted install.py notes.
- For a retryable curated pause with live curatedReviews, provide ONE batch review
  of the entire returned array in backend order.
- All consent starts unchecked. Render the detached warning explicitly as supplied.
  Execution consent alone must not satisfy warning acknowledgement.
- Preserve the dialog’s binding-keyed reset; changed IDs/bindings must clear approval,
  and returning to an earlier binding must not restore old consent.
- Retry submits one CommitInput per reviewed plan with its exact planId/binding,
  through setupRun({ kind: 'retry', runId, managerConsent, curatedDecisions }).
- If retaining approved decisions for another retry, retain them only in memory
  for the exact run and current ordered plan-ID/binding batch. Resend them on every
  relevant Retry: the backend consumes decisions once.
- Invalidate approvals when the run or review batch changes, when live reviews
  disappear at a consent pause, or when the UI session is recreated.
- A Retry must not bypass unapproved live reviews with curatedDecisions: [].
- After backend restart, a curated consent failure may have curatedReviews: [].
  Offer Retry with [] to obtain fresh reviews, explain why re-review is needed,
  and require fresh approval when the reviews return.
- Closing the dialog dismisses it only; it must not skip setup, discard backend node
  plans, or authorize execution. Offer a way to reopen it.
- Show action errors inside an open modal through notice.
- Preserve exact-source Manager consent, cancellation cleanup and reconnect behavior.

Skip:
- Require failure.canSkip.
- Existing defensive checks for failed run, matching failed step and optional policy
  may remain; policy alone must never authorize Skip.
- A damaged optional step with canSkip false must not expose Skip.

Copy:
- Replace setup-wizard.tsx:234 “M1 checks” with current behavior.
- Replace setup-wizard.tsx:671’s future-update claim with accurate current-checkout
  wording; ComfyUI updates are available separately.
- Remove the entire Options alert at approximately 706–714:
  “M1 scope”, optional components “deferred”, and Start/Stop “arrive in M2”.
- Retain accurate source-build-disabled information only where useful, without
  milestone wording.

## D. App / recovery

Files: App.tsx, App.test.tsx, app-shell.tsx and app-shell.test.tsx;
dedicated rebuild UI under features/setup as needed.

Existing contracts:
- recovery_required + recovery.code root_missing/root_inaccessible:
  installRoot is the recorded saved root.
  state.rs:113–153; env_dto.rs:574–596.
- rebuild_required + recovery.code moved/interpreter_broken:
  env_dto.rs:538–550.
- RootSelectionDto is { restartRequired: boolean }.
  Both recovery root-selection controllers request restart on success:
  env_commands.rs:229–254.
- Rebuild success also requests restart:
  env_commands.rs:175–210.
- A broken interpreter retains normal backend composition; only a moved root uses
  restricted composition. UI must still show the guided rebuild offer for both.

Missing/inaccessible root:
- Show recorded path and backend guidance.
- Offer “Locate existing install” and “Create new install”.
- Use gateway.pickFolder; cancellation makes no root-selection call.
- Locate calls setupOpenInstall(path); New calls setupNewInstall(path).
- Pending actions cannot submit twice. Successful restartRequired response shows
  “Restart required” / restarting feedback and blocks further mutation.
- Do not call setupSelectRoot, transfer settings/draft, or create an ordinary
  first-run wizard against the missing saved root.
- Show useful English messages for:
  root_missing, root_invalid, root_fixed_by_override, root_switch_failed,
  comfyui_running, comfyui_state_unknown, maintenance_busy,
  relocation_recovery_required.
  Preserve sanitized backend detail where useful.
- Explain permissions for root_inaccessible.
- If the saved root has become available again at its original path, instruct the
  user to restart the app. Recheck does not recompose rootless recovery.

D8 decision for I:
- Do not offer Locate/New on rebuild_required.
- Do not change FsExistingInstallOpener::check_separate/open_existing/select_new.
- Current/recorded-root equality and ancestor/descendant refusals remain.
- Record this residual in the implementation report for main/R:
  a moved restricted installation cannot reopen its recorded old root through Locate,
  even if that original installation becomes available again.
  Supporting that requires a separately reviewed backend guard change.
- No arbitrary-install switcher or new restart command.

Guided rebuild:
- Route rebuild_required before AppShell, even when activeEnv exists.
- Use a dedicated rebuild branch/component, not the ordinary six-stage wizard.
  Restricted services cannot serve setupDetectHardware/setupListProfiles/envInfo.
- Summary shows:
  moved versus broken-interpreter reason;
  current root;
  recorded profile ID/revision and Python version from activeEnv when available;
  preserved models, custom nodes, user/input/output, external model roots and launch
  options;
  a fresh app-owned environment replaces the selected environment after verification;
  previous/adopted environment files remain untouched;
  current ComfyUI checkout is retained and node requirements are reinstalled;
  setup does not run install.py.
- Do not offer editable profile/Python/component choices for rebuild.
- Backend validation remains authoritative; show registration errors normally.

The Rebuild button sends this COMPLETE typed action:

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

Require activeEnv before offering this action; otherwise show a recoverable error.
Do not send installRoot: the controller injects its current root.

All SetupRequestDto fields are required for deserialization. For rebuild, the backend
uses mode/current root and replaces the other client fields from the successful
recorded run before applying rebuild overrides:
setup.rs:98–157. The false/empty values above are wire placeholders, not an instruction
to remove recorded components. In particular, cleanCache comes from the recorded
request, not this placeholder.

Progress/recovery routing:
- Use the existing setup-run controller and RunProgress for registered/running,
  failed/interrupted and cancelled rebuilds.
- Keep the rebuild UI mounted across polling transitions and through successful
  “restarting” feedback. Do not briefly enter AppShell before the controlled restart.
- Failed rebuilds may still report phase rebuild_required because health precedence
  comes before retryable-run status (queries.rs:179–201).
  Preserve a path to inspect/retry the existing rebuild run after reload; do not
  replace every failure with a fresh-run button.
- Do not automatically resume a copied historical non-rebuild run merely because
  run.status is failed/interrupted.
- RunSnapshotDto has no run-root field. Do not invent one or infer root identity
  from profileId. Any attempted historical rebuild retry remains subject to the
  backend old-root guard; show its refusal and retain the fresh rebuild option.
- Preserve ordinary first-run/re-setup behavior outside the recovery/rebuild routes.

Other recovery codes:
- Keep the generic recovery alert, backend code/message/path and Recheck.
- Do not infer that bootstrap/manifest corruption is missing-root recovery.
- Remove App.tsx’s “deferred to M6” copy.

Shell:
- Remove app-shell.tsx’s “Coming in a later milestone” fallback.
  Use a neutral unreachable-route fallback or exhaustive existing navigation handling.
- Update its test to assert real page routing, rather than merely absence of the
  removed placeholder.
- Preserve existing CloseRequestDialog behavior.

Completed B/P integration:
- Verify and preserve bandwidth integer KiB/s validation, empty = unlimited.
- Preserve configured-only updater actions and plain-text release notes.
- Preserve both diagnostics buttons using appCopyDiagnostics and read-only fallback.
- Existing required tests already cover these; do not rebuild those features.

## E. Environment page

Files: environment-page.tsx and environment-page.test.tsx.

- Render info.components.accelerators, including unselected/failed/skipped results.
- Show id, selected yes/no, install status, import result and optional message.
- importable true = passed; false = failed; null = untested.
- Empty stored results mean no recorded accelerator outcomes; do not describe that
  as proof no accelerators are installed.
- Preserve the existing accelerators summary and Torch/CUDA/Triton metadata.
- Do not probe imports from the UI.
- Update the environment-page info fixture for the required nested components field.
- Remove stale “M1” wording at approximately lines 345 and 381:
  describe stopping external ComfyUI and entering the recorded inactive environment
  ID in user-facing terms. Do not add a retained-environment listing feature.

## Required UI tests

Keep existing tests passing and update required DTO fixtures/mocks.

ui/src/features/setup/setup-wizard.test.tsx:
- profile_change_resets_accelerator_selection
  Include a routine refetch/rerender that does not reset a deliberate selection.
- empty_accelerator_selection_remains_empty
  Verify the exact setupRun request contains [] after deselecting all.

ui/src/app/App.test.tsx:
- curated_selection_does_not_authorize_execution
  Exercise the composed setup/review flow with the real public ReviewDialog.
  Default selection does not create decisions or commit nodes; execution and warning
  acknowledgement begin unchecked.
- curated_binding_change_clears_consent
  Exercise the injected real dialog, including returning to an earlier binding.
- missing_root_offers_locate_and_new_install
  Cover missing/inaccessible variants, exact gateway calls, picker cancellation,
  restart feedback and root-selection failure.
- rebuild_shows_preserved_data_and_owned_replacement
  Cover moved/broken-interpreter reasons, preserved-data copy and exact complete
  rebuild request.
- rebuild_uses_restricted_safe_ui_and_recovers_failed_run
  Assert no ordinary hardware/profile/environment queries, no automatic retry of
  a copied non-rebuild run, and inspect/retry access for a failed rebuild after reload.
- rebuild_waits_for_controlled_restart_after_success
  Assert successful progress does not expose the normal shell before restart.

ui/src/features/setup/run-progress.test.tsx:
- damaged_optional_step_cannot_be_skipped
- curated_retry_resends_decisions_and_rereviews_after_restart
  Cover repeated exact decisions for the same live batch, invalidation on changed
  bindings/empty reviews, [] Retry to reprepare after restart, and fresh consent.
  Inject a test renderer here; do not import another feature.

ui/src/features/environment/environment-page.test.tsx:
- environment_page_shows_stored_accelerator_results
  Cover installed/failed/skipped, selected false, import true/false/null, messages
  and empty legacy outcomes.

ui/src/shared/ipc/gateway.test.ts:
- setup_curated_catalog_takes_no_arguments
- setup_root_recovery_sends_exact_paths
- setup_run_forwards_accelerators_curated_selection_and_retry_decisions
- node_subscription_keeps_tauri_channel_inside_gateway

Preserve these existing required tests:
- bandwidth_empty_means_unlimited_and_zero_is_invalid
- update_install_echoes_reviewed_update_id
- diagnostics_copy_failure_exposes_read_only_fallback

Preserve existing M5 review-dialog binding, warning, trust and busy-state tests.
Add focused coverage for setup-only hidden trust controls if not covered by the app
integration tests.

## SUCCESS CRITERIA

Functional:
- All three DTO additions match section A; generated bindings are current.
- Explicit empty accelerator selections survive unchanged.
- Curated selection is separate from exact reviewed consent.
- Live review bindings control consent; every relevant Retry includes decisions.
- Backend canSkip false suppresses Skip.
- Rootless recovery and moved rebuild do not mount unsupported normal-service UI.
- Rebuild sends every required wire field, supports failure/reconnect, and waits
  for controlled restart.
- Existing B/P/M5 behavior and ACL restrictions remain intact.
- No user-facing stale milestone promises remain.

Copy audit:
Run:
  rg -n -i 'deferred|later milestone|M1|M2|M6|Coming in' ui/src crates/desktop/src/ipc/env_dto.rs

Inspect every hit. Test deferred<T>() helpers, model filenames and historical
code comments are not obsolete UI copy and need no unrelated cleanup.
Include the entire stale Options M1/M2 alert, both Environment M1 references and
the backend missing-manifest message in the audit.

Run the merged-tree checklist, reporting actual results/counts:
  cargo fmt --all -- --check
  cargo clippy --workspace --all-targets --locked -j 4 -- -D warnings
  cargo test --workspace --locked -j 4 --no-fail-fast
  cargo test -p desktop --test bindings --locked -j 4
  cargo test -p desktop --test acl --locked -j 4
  pnpm -C ui lint
  pnpm -C ui typecheck

In PowerShell set:
  $env:VITEST_MAX_WORKERS = '2'
Then:
  pnpm -C ui test
  pnpm -C ui build
  node --test scripts/check-arch-deps.test.mjs
  node scripts/check-arch-deps.mjs
  cargo build -p desktop --locked -j 4 --features tauri/custom-protocol

The last desktop build follows the frontend build.
Do not reuse historical 709/160 counts as current verification results.
If runtime permissions prevent a check, report it as unrun with the exact blocker;
do not claim a pass.

OUTPUT:
- Per-file changes with file:line.
- Final DTO shapes and gateway payloads.
- Added/updated test names and actual results.
- Exact verification command outcomes/counts.
- Confirmation that P0 was preserved and no relocation guard changed.
- Copy-audit findings.
- D8 residual for main/R, plus the RunSnapshotDto run-root visibility limitation.
- Any deviations or unverified visual/native behavior.
