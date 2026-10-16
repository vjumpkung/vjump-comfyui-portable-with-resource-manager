## 1. No-go

**No-go for the draft as an M5 completion plan.** The frontend work can proceed using the corrected packages below, but M5 acceptance remains blocked by the backend findings in `HANDOFF.md`.

The draft correctly identifies the five consent defects. It additionally needs:

- Actual Git status for acceptance scenario 11.
- Raw Registry compatibility metadata, currently discarded by the IPC DTO.
- Recovery of pending reviews and recent failures after UI reload.
- Correct cancellation keys, stale-plan handling, batch limits and repair-mode eligibility.
- Backend verification for scenarios 14, 16 and 17.

This review was strictly read-only. No files were changed and no builds or tests were run.

## 2. Corrected plan

All frontend paths below are relative to `ui/src/features/custom-nodes/` unless stated otherwise. Preserve existing uncommitted work and all fixed decisions in `HANDOFF.md` §3.

### WP0 — Preserve the backend acceptance gates

These are **separate backend work**, not features to approximate in React. Frontend implementation may proceed, but M5 cannot close until these pass.

| Required correction | Exact implementation locations | Required regression |
|---|---|---|
| Repair restores file-based and persisted Registry dependencies, including disabled nodes; dependency outcomes follow environment ID and exact source revision. Core verification alone cannot clear node damage. Failed replacement dependency installation cannot activate a supposedly repaired environment. | `crates/application/src/environment/{ports,execute}.rs`; `crates/application/src/custom_nodes/{ports,use_cases}.rs`; `crates/infrastructure/src/environment/source.rs`; `crates/infrastructure/src/custom_nodes/sqlite.rs`; desktop `state.rs` | `repair_does_not_clear_node_failure_from_core_verification_alone`; `repair_reinstalls_registry_only_and_disabled_node_dependencies_under_pins`; `repair_failure_preserves_previous_active_environment`; `dependency_state_is_scoped_to_environment_and_exact_source` |
| Recovery checks recorded identities before restoring, moving or recycling copies. Ambiguous copies remain preserved and block launch, including ambiguity discovered in a previously nonblocking cleanup phase. | Infrastructure `custom_nodes/files.rs`; application `custom_nodes/{model,use_cases}.rs` as needed for persistent blocking state | `recovery_preserves_foreign_destinations_and_modified_backups`; `cleanup_requires_matching_published_and_backup_identities`; `ambiguous_cleanup_blocks_start` |
| Git staging and publication validate the worktree and reject links/reparse points and late edits. | Infrastructure `custom_nodes/{files,git,tree}.rs` | `git_staging_rejects_late_non_requirement_edits`; `git_update_validates_the_applied_tree` |
| A successful consented hook’s final tree identity is persisted before `Verified`. | Application `custom_nodes/workflow.rs`; infrastructure snapshot/recovery implementation | `successful_hook_persists_final_tree_identity_before_verified` |
| Node-detail refusals propagate; node-level and release-level warnings both reach the exact review. | Infrastructure `custom_nodes/files.rs::prepare_registry` | `node_detail_refusal_aborts_preparation`; `node_and_release_warnings_are_both_reviewed` |
| Start checks the node journal while holding the maintenance lease. Restart awaits stop/reconciliation/scan before launch without recursive locking. Stop does not silently swallow required rescan failures. | Application `comfy_runtime/{ports,use_cases}.rs`; desktop `ipc/comfy_commands.rs`, `state.rs` | `start_checks_node_journal_under_maintenance_lease`; `restart_stops_reconciles_and_starts_under_one_lease`; `stop_surfaces_required_rescan_failure` |
| Filesystem operations protect the complete parent chain at operation time. | Infrastructure `custom_nodes/files.rs`, shared pinned-directory primitives | `linked_or_replaced_comfy_parent_is_never_mutated` |

These requirements follow directly from `REQUIREMENTS.md` §3.3.3, FR-NODE-4/6/9/10, NFR-SEC-5 and scenarios 14/16/17. The explorer findings remain present in the inspected implementation.

### WP1 — Complete the minimum IPC contract

#### 1. Finish the repository opener

In `crates/desktop/src/ipc/node_commands.rs::repository_url`, reject an empty raw authority as well as raw userinfo:

```rust
authority.is_none_or(|authority| authority.is_empty() || authority.contains('@'))
```

Retain the existing normalized HTTPS validation and sanitized `validation` error.

Add:

```rust
fn repository_browser_urls_reject_empty_raw_authority()
```

Assert rejection of at least `https:////@github.com/example/node`, ordinary extra-slash spellings and backslash authority spellings; retain the existing valid normalization and unsafe-URL tests.

#### 2. Expose Registry metadata already held by the application

Extend `RegistryNodeDto` in `crates/desktop/src/ipc/node_dto.rs`:

```rust
pub supported_os: Option<Vec<String>>,
pub supported_accelerators: Option<Vec<String>>,
pub supported_comfyui_version: Option<String>,
pub status: Option<String>,
```

Map directly from `RegistryNode.compatibility`:

- `supported_os`
- `supported_accelerators`
- `comfyui_expression`
- `status`

Keep the existing evaluated `os`, `accelerator`, `comfyui`, `flagged`, `deprecated` and `warnings`.

No Registry port change or new Registry command is needed.

#### 3. Add an actual Git-status query

`dirty: boolean | null` and `node_git_refused` cannot show `git status`, detect missing upstream or explain divergence.

Add to `crates/application/src/custom_nodes/model.rs`:

```rust
pub struct NodeGitStatus {
    pub revision: domain::comfy_runtime::Revision,
    pub state: domain::comfy_runtime::RepoState,
}
```

Add to `NodeFiles` in `custom_nodes/ports.rs`:

```rust
async fn git_status(
    &self,
    root: &Path,
    node: &InstalledNode,
) -> Result<NodeGitStatus, NodeAppError>;
```

Add to `NodeService` in `custom_nodes/use_cases.rs`:

```rust
pub async fn git_status(
    &self,
    id: &NodeId,
) -> Result<NodeGitStatus, NodeAppError>;
```

The service acquires the maintenance gate, resolves the stored node ID and configured root, rejects non-Git/single-file requests with `InvalidRequest`, then delegates. This query performs no fetch, update, Python import or hook execution.

Implement `LocalNodeFiles::git_status` in infrastructure `custom_nodes/files.rs` using validated node location and `GitNodes::inspect`. Preserve relative status text, cap it at 2,000 characters and apply the existing redactor.

Add **new** `NodeAppError::GitUnavailable`, mapped in `node_dto.rs::describe` to:

```text
node_git_unavailable
Git is unavailable. Complete tool setup on Windows or install Git on Linux.
```

Use existing `NotFound`, `Busy`, `UnsafeSource` and other adapter errors for their existing meanings. Update `FakeFiles` in application `custom_nodes/tests.rs`.

Add the camelCase DTO:

```rust
pub struct NodeGitStatusDto {
    pub commit: String,
    pub branch: Option<String>,
    pub dirty: bool,
    pub unfinished_operation: bool,
    pub ahead: f64,
    pub behind: f64,
    pub has_upstream: bool,
    pub status: String,
}
```

Add command:

```rust
pub async fn node_git_status(
    state: State<'_, AppState>,
    input: NodeIdInput,
) -> Result<NodeGitStatusDto, IpcError>;
```

Register it in desktop `ipc/bindings.rs`, `build.rs`, manager capability and valid-body ACL cases. Generate its permission through the normal build mechanism.

#### 4. Make reload recovery discoverable

Current `node_list` hides prepared plans and terminal failures. Consequently, a reloaded UI cannot rediscover a completed preparation or reliably find a failed operation.

Add:

```rust
// application/custom_nodes/use_cases.rs
pub fn pending_reviews(&self) -> Result<Vec<PlanReview>, NodeAppError>;

pub async fn recent_operations(
    &self,
) -> Result<Vec<NodeOperation>, NodeAppError>;
```

Rules:

- `pending_reviews` clones the current process-local install/update/retry reviews from `NodePlans`, sorted by ID. Exclude direct toggle/uninstall plans.
- `recent_operations` returns the newest 16 terminal journal records. The repository currently reads operations in ascending creation order.
- Neither method restores consent.

Extend `NodeListDto`:

```rust
pub pending_reviews: Vec<PlanReviewDto>,
pub recent: Vec<OperationDto>,
```

Extend `OperationDto`:

```rust
pub node_id: String,
pub blocks_start: bool,
```

Populate `blocks_start` from the application’s launch-blocking policy. Correct the existing `unresolved` documentation: **not every unresolved operation blocks launch**; ordinary cleanup does not.

Keep `nodeOperationStatus` for an authoritative selected-operation read. Recent history is bounded; do not advertise an unlimited audit-history feature.

#### 5. Regenerate bindings and add gateway wrappers

After the Rust contract is finished and the compiler is available, explicitly regenerate `ui/src/shared/ipc/bindings.ts`. Never edit it manually.

Add in `ui/src/shared/ipc/gateway.ts`:

```ts
nodeOpenRepository(repository: string): Promise<null>
nodeGitStatus(nodeId: string): Promise<NodeGitStatusDto>
```

Wrap the status command with `{ nodeId }`, matching `NodeIdInput`. Retain all existing node wrapper signatures.

### WP2 — Correct pure consent and eligibility rules

Edit `model.ts`. Keep the existing `Consent` fields and initially unchecked values.

Provide:

```ts
sourceLabel(source: NodeSourceDto): string
reviewKey(plan: PlanReviewDto): string
hasTrustNamespace(source: NodeSourceDto): boolean
normalizeConsent(plan: PlanReviewDto, consent: Consent): Consent
canCommit(plan: PlanReviewDto, consent: Consent): boolean
decision(plan: PlanReviewDto, consent: Consent): CommitInput
isUpdateCandidate(node: InstalledNodeDto): boolean
canRetryDependencies(node: InstalledNodeDto): boolean
```

Required behavior:

- `reviewKey` includes both plan ID and binding, preferably `JSON.stringify([id, binding])`.
- `canCommit` always requires `execute`; remembered trust never substitutes for it.
- Warnings and protected changes require their corresponding explicit confirmations.
- `runInstallPy` requires `hasInstallPy && hook`.
- `rememberSource` requires an available trust namespace.
- `rememberInstallPy` requires **all** of `hasInstallPy`, `hook`, `remember`, available namespace and `rememberHook`.
- Unchecking script execution or source trust clears `rememberHook`.
- `decision` echoes the exact ID/binding and sends exactly the reviewed protected changes when confirmed.
- Update candidates exclude manual, single-file, conflict and known-dirty nodes. Unknown Git health still requires status inspection.
- Retry dependencies is available for eligible directory nodes with `failed` **or `not_run`** dependencies. This includes manual directories; backend preparation remains authoritative.

Trust eligibility follows `NodeSource::trust_namespace`: Git repository, or Registry ID plus publisher and repository. Do not offer trust for missing Registry publisher/repository metadata.

### WP3 — Finish list, operation and Registry hooks

Edit `use-nodes.ts`; add `use-registry.ts`.

Keep React Query. Do not introduce another state-management library or rewrite the existing library/download stores.

Export:

```ts
const NODE_KEY = ['custom-nodes'] as const

interface NodeLogEntry {
  epoch: string
  sequence: number | null
  operationId: string
  line: string
}

useNodes()
useNodeOperation(operationId: string | null)
useRegistry(input: Omit<RegistrySearchInput, 'refresh'>)
```

Rules:

- `useNodes` polls `nodeList` every five seconds.
- Subscription snapshot, changed event, gap, discontinuous sequence and reconnection invalidate authoritative list/status queries.
- Ignore duplicate/older ordinary events within the accepted epoch.
- Snapshot/gap markers must trigger recovery even when their sequence equals the previous sequence.
- Use subscription-generation guards so callbacks and late subscription promises from retired attempts cannot update state.
- Retry subscription failure after two seconds.
- Unsubscribe subscriptions resolving after teardown.
- Bound structured live logs to 300 entries. Do not encode operation identity by prefixing strings.
- Poll the selected nonterminal operation every second; invalidate its status on log activity at a bounded rate, such as once per 250 ms.
- Render an authoritative `logTail` after refetch; do not concatenate overlapping tails and live buffers.
- Subscription failure must not disable list/status polling.
- Cleanup unsubscribes and clears timers. **It never calls `nodeCancel`.**

For Registry queries:

- Key by search/page/limit, excluding `refresh`.
- Debounce search by 300 ms; reset page to 1 when committed search or limit changes.
- Initial page 1, limit 50; offered limits must stay within backend maximum 200.
- Manual refresh makes a one-shot request with `refresh: true`; subsequent normal queries use `false`.
- Use backend `fetchedAt` and `stale`. Distinguish refresh failure from a backend-confirmed stale cache.
- Do not install from a previous query’s placeholder results while the new query is pending.

On every mutating command settlement, refresh nodes and relevant `environmentKeys.status`, `.info` and `.packages`, including failure/cancellation.

### WP4 — Build Registry installation and exact review

Create `registry-browser.tsx`, `install-dialog.tsx`; finish `review-dialog.tsx`.

Define in `model.ts`:

```ts
type NodePreparation =
  | { kind: 'registry'; id: string; version: string | null }
  | { kind: 'git'; repository: string; reference: string | null }
  | { kind: 'update'; nodeId: string; reference: string | null }
  | { kind: 'retry'; nodeId: string }
```

Registry browser must show:

- Name, publisher, description, downloads, GitHub stars and latest version.
- Raw supported OS/accelerator/ComfyUI metadata alongside evaluated text-and-icon status.
- Raw Registry status, flagged/deprecated warnings.
- Fetch timestamp, offline-cache notice, pagination and refresh.
- Unknown values as “Unknown” or `—`; preserve numeric zero.
- Repository action through `gateway.nodeOpenRepository`, with `ExternalLink`.
- Plain text rendering for remote descriptions and warnings.

Installation:

- Registry release with optional exact-version text; empty becomes `null`.
- Registry-to-Git choice only when a repository exists.
- Standalone HTTPS Git URL with optional branch/tag/commit.
- Generate a `crypto.randomUUID()` request key before each preparation.
- Preparation returns a review; it never commits automatically.

Keep this public review signature, extending the existing component:

```ts
ReviewDialog({
  plans,
  busy,
  onCommit,
  onClose,
  onCancel,
}: {
  plans: PlanReviewDto[]
  busy: boolean
  onCommit: (items: CommitInput[]) => void
  onClose: () => void
  onCancel: (() => void) | null
})
```

Requirements:

- Show exact resolved source, previous source, requirements, warnings and protected changes.
- Label `treeDigest` **“Tree identity”**, not invariably “SHA-256”: current Git identities are `git:<commit>`.
- Label requirements as covering Registry dependencies and `requirements.txt`.
- Always show the execution checkbox.
- Script execution remains unchecked, including for remembered script trust.
- Reset the entire consent session whenever the plan ID/binding set changes. A keyed inner review session avoids retaining consent if a binding changes away and later returns.
- Closing an idle review explicitly discards its plans through `nodeCancel(plan.id)`.
- While applying, block dismissal; provide explicit “Request cancellation” when a valid active cancellation key exists.
- Never discard plans merely because the page unmounts.

Use `pendingReviews` to offer **Resume review** after navigation/reload, with fresh unchecked consent.

### WP5 — Build installed-node controls and batch updates

Create `installed-table.tsx`, `confirm-dialog.tsx`, `update-all-dialog.tsx`.

Use `@tanstack/react-virtual` above 200 filtered rows. Follow the library table’s navigation mechanics, without importing that feature.

Table behavior:

- Stable identity: `node.id`.
- Show source, version/commit, branch, enabled state, dependency state, origin, conflict and dirty/unknown status.
- Text, source-kind and enabled filters; include disabled nodes by default.
- One active row in the tab order; Arrow keys, Home/End and PageUp/PageDown scroll before focusing.
- Space toggles batch selection.
- Keep actions in an adjacent toolbar/details area to avoid inaccessible fixed-height rows full of controls.

Actions:

- **Update:** inspect Git status first. Show actual status and refuse dirty, unfinished or diverged repositories. Require an explicit reference for detached or upstream-less Git nodes. Backend preparation rechecks everything.
- **Retry dependencies:** prepare and review; never run a hook merely because an earlier installation allowed it.
- **Enable/Disable:** confirmation states the move and resulting enabled state.
- **Uninstall:** confirmation says the folder goes to Recycle Bin/Trash and Python packages remain shared. No permanent-delete fallback.
- **Open repository:** explicit opener IPC.

Repair mode:

- Disable installs and updates.
- Permit Retry dependencies, Enable/Disable and Uninstall as the current backend allows.
- Conflicts prohibit Update/Retry; safe toggle/uninstall attempts remain available.
- Successful Retry or Uninstall does not clear the environment repair banner by itself.

Update all:

- Include disabled eligible nodes.
- Prepare sequentially; retain per-node preparation failures and skips.
- Detached/upstream-less nodes requiring user targets are explicitly skipped with “Update individually with an explicit target.”
- Review each exact prepared plan using the same consent component.
- Commit through `nodeUpdateAll`, not concurrent individual commits.
- Respect the 128-plan/backend limit. For larger selections, process successive groups of at most 128, reviewing each group before committing it. Do not silently truncate.
- Require existing pending reviews to be completed/discarded before starting a new batch.
- Preserve per-plan results, including `cleanupPending`, skipped/up-to-date entries, errors, cancellation and `stoppedForRepair`.
- Stop further groups when repair becomes necessary or cancellation is requested.
- Never automatically replay a partially executed batch. Reprepare and review failed/unrun targets.

### WP6 — Compose page, errors, operations and runtime actions

Create `custom-nodes-page.tsx`, `operations-panel.tsx`, `index.ts`.

Export:

```ts
interface CustomNodesPageProps {
  maintenanceBusy: boolean
  runtime: ComfyStatusDto | null
  runtimeBusy: boolean
  onRepair: () => void
  onStopComfy: () => Promise<void>
  onRestartComfy: () => Promise<StartResultDto>
  onOpenComfy: () => void
}

CustomNodesPage(props: CustomNodesPageProps)
```

The page owns preparation/review/action state. Prevent duplicate submissions and disable conflicting controls while its operation runs. Backend enforcement remains authoritative.

Use these cancellation keys:

| Work | Key |
|---|---|
| Preparation | Client-generated `requestId` |
| Single commit | `plan.id` |
| Update-all execution | Literal `update-all` |
| Idle review discard | Each `plan.id` |
| Direct toggle/uninstall or rescan | No cancel control; current IPC exposes no usable cancellation key |

A cancellation acknowledgment means cancellation was requested, not that cleanup finished. Wait for the command/status outcome. If an explicitly cancelled preparation races with a successful review response, discard that returned plan. Unmount alone must not trigger this behavior.

Operations panel:

- Show unresolved operations, recent terminal results and selected operation status/logs.
- Use `blocksStart` for launch/recovery warnings.
- “Rescan / finish recovery” calls `nodeRescan`; show its failure.
- After backend restart, show persisted failure/recovery information even when an in-memory log tail is unavailable.
- Do not label `cleanup_pending` as installation failure or offer permanent deletion.

Error actions:

| IPC code | UI behavior |
|---|---|
| `comfyui_running` | Offer explicit Stop ComfyUI and retry. Await stop before repeating the refused single request. |
| `maintenance_busy`, `node_operation_running` | Explain busy state; allow deliberate retry. Do not stop another operation. |
| `node_plan_stale` | Remove old consent; offer fresh preparation/review. Never substitute a new target into the old decision. |
| `node_consent_required` | Keep review and require valid explicit selections. |
| `node_git_refused` | Read/show Git status; offer manual resolution or explicit-target input where applicable. |
| `node_git_unavailable` | Show tool-install guidance. |
| `node_up_to_date` | Informational result/skip. |
| `node_dependencies_failed`, `pin_drift`, `environment_not_usable` | Refresh authoritative health; show logs, eligible Retry and Repair. |
| `node_recovery_required` | Show recovery panel and rescan action. |
| `registry_refused` | Show refusal; do not mislabel it as offline cache. |
| `node_resolution_failed`, `node_unsafe_source`, `node_target_exists`, `node_ambiguous_reference` | Explain the specific refusal; no automatic override. |
| `transport` | Treat outcome as uncertain; refresh state before any repeat mutation. |
| `cancelled` | Refresh health and operation state; cancellation may still require Repair. |

After successful verified work, offer **Restart ComfyUI** only when refreshed health permits it and no operation blocks launch. Call `onRestartComfy`; handle both `started` and `port_conflict`. For a conflict, offer `onOpenComfy` to resolve launch options; never silently accept a suggested port.

Shell integration:

- Export `useRuntime` and `useRuntimeActions` from `ui/src/features/comfy-runtime/index.ts`.
- In `ui/src/app/shell/app-shell.tsx`, add a small `CustomNodesRoute` component using those public exports.
- Pass Repair as `onSetup('repair')`.
- Pass awaited stop/restart mutation callbacks.
- Add the `custom-nodes` branch and navigation callback to ComfyUI.
- Do not add another runtime subscription/store or import Comfy feature internals into Custom Nodes.

## 3. Decisions and assumptions

| Question/decision | Classification and answer | Evidence |
|---|---|---|
| **Q1: Is a versions picker required?** | **AI-decidable: no.** Optional exact-version text is sufficient; blank means latest resolved once. | FR-NODE-2 explicitly permits optional `?version=`. Existing `PrepareRegistryInput` supports it. A versions port exists, but exposing it is unnecessary scope. |
| **Q2: Start or Restart after success?** | **AI-decidable: use `comfyRestart`**, including when already stopped. Remove the separate `onStartComfy` proposal. | Existing `RestartComfy::execute` stops then starts under one lease. The required stop/reconcile correction belongs to WP0. |
| **Q3: Missing requirements?** | **AI-decidable:** raw Registry metadata; actual Git status; reload-discoverable reviews/failures; `not_run` Retry; batch bounds/results; repair-mode toggle/uninstall; correct cancellation; backend acceptance gates. | FR-NODE-1/4/6/9/10 and scenarios 10/11/14/16/17; current DTOs and command implementations. |
| Remembered trust | **AI-decidable:** informative only for this UI; retain explicit per-plan execution/script selections. | `InstallConsent::check` always requires `execute_node_code`; current hook execution depends on the submitted hook flag. |
| Protected-package changes | **AI-decidable:** retain exact explicit confirmation; never preselect it. The ordinary conflicting-torch acceptance path must remain blocked with torch unchanged. | Existing `preview_packages`, exact comparison in `commit_held`, FR-ENV-3 and FR-SETUP-7. |
| Retry after dependency failure | **AI-decidable:** Retry may repair a node’s dependency outcome while the environment remains `repair_required`. | Existing `retrying_dependencies_cannot_clear_an_unrelated_repair_requirement` test and workflow behavior. |
| Crash scenario 14 | **AI-decidable:** test prepublication interruption separately from postpublication recovery. Never delete a published node merely to make “unchanged” appear true. | Staged publication and journal phases in `workflow.rs`; §6.2 recovery requirements. |
| Frontend architecture | **AI-decidable:** retain React Query and gateway access; compose cross-feature actions in `app/`. | ADR-0001/0002 and enforced ESLint boundaries. |

Cut from this package:

- Node-set import/export: FR-NODE-11 is P2.
- A Registry versions-list command.
- Source-build authorization: current node IPC deliberately fixes `allow_source_builds` to false.
- New runtime/global state architecture.
- Permanent node deletion.
- Curated setup nodes, accelerators, moved-install rebuild, updater, packaging and README: subsequent M6/README work.

## 4. Risks

- **Backend correctness remains the principal blocker.** Repair banners and disabled buttons cannot establish lease safety, recovery identity or successful dependency restoration.
- Prepared plans are process-local. UI reload can resume them through the proposed list field; backend restart must reconcile their staging instead.
- Current `observed_identity` preserves unsupported raw remotes, and DTO conversion copies repository strings. Review persistence/output sanitation for embedded credentials before displaying newly exposed diagnostics; only validated URLs may reach the opener.
- A channel can disappear without a disconnect event. Polling preserves authoritative state, but tests must not claim silent-channel event delivery automatically reconnects.
- The broken MSVC linker and possible concurrent compilation prevent verification now. Earlier passing results do not cover this plan.
- Windows/Linux native behavior, trash integration and real dependency operations still require platform evidence.

## 5. Success checks

**Commands below are for the implementation owner later. None were executed during this review.** Wait until concurrent compilation has finished and the linker is repaired.

First regenerate bindings explicitly:

```powershell
cargo test -p desktop --test bindings --locked -- --ignored export_bindings
```

Then run:

```powershell
pnpm -C ui lint
pnpm -C ui typecheck
pnpm -C ui test
pnpm -C ui build

cargo fmt --all -- --check
cargo test -p application custom_nodes --locked
cargo test -p application environment --locked
cargo test -p application comfy_runtime --locked
cargo test -p infrastructure custom_nodes --locked
cargo test -p desktop --test acl --locked
cargo test -p desktop --test bindings --locked

node scripts/check-arch-deps.mjs
```

Required frontend tests:

| File | Exact test names and assertions |
|---|---|
| `model.test.ts` | `trusted_source_still_requires_execution_consent`; `decision_masks_hidden_script_trust`; `registry_trust_requires_publisher_and_repository`; `decision_echoes_exact_binding_and_pin_changes`; `retry_includes_not_run_directory_nodes` |
| `review-dialog.test.tsx` | `binding_change_resets_all_consent`; `returning_to_an_old_binding_does_not_restore_consent`; `remembered_hook_starts_unchecked`; `warnings_and_pin_changes_require_explicit_confirmation`; `busy_review_cannot_be_discarded` |
| `use-nodes.test.tsx` | `older_events_do_not_replace_current_state`; `equal_sequence_gap_refetches_list_and_status`; `retired_subscription_callbacks_are_ignored`; `failed_subscription_retries`; `late_subscription_is_unsubscribed`; `silent_channel_still_polls`; `unmount_never_cancels_backend_work`; `logs_are_structured_and_bounded` |
| `use-registry.test.tsx` | `search_resets_page_after_debounce`; `manual_refresh_is_one_shot`; `previous_query_results_cannot_be_installed`; `null_metadata_remains_unknown_and_zero_remains_zero` |
| `installed-table.test.tsx` | `virtualizes_only_above_200_rows`; `end_scrolls_then_focuses_the_last_row`; `filtering_preserves_or_repairs_active_identity`; `keyboard_selection_uses_node_ids` |
| `custom-nodes-page.test.tsx` | `registry_install_requires_exact_review_before_commit`; `reload_rediscovers_review_without_consent`; `failed_operation_remains_visible_after_remount`; `git_refusal_displays_status`; `stop_is_awaited_before_retry`; `stale_commit_requires_new_review`; `uninstall_confirms_trash_and_shared_packages`; `repair_mode_keeps_supported_recovery_actions`; `successful_retry_does_not_clear_repair_banner`; `cancel_uses_request_plan_and_batch_keys`; `unmount_does_not_cancel_inflight_work`; `restart_port_conflict_is_not_reported_as_started` |
| `update-all-dialog.test.tsx` | `prepares_and_commits_sequentially`; `reports_preparation_and_commit_results_per_node`; `stops_remaining_groups_for_repair`; `splits_more_than_128_targets_into_reviewed_groups`; `never_replays_successful_items_after_partial_failure` |
| `ui/src/shared/ipc/gateway.test.ts` | `node_repository_open_uses_generated_command`; `node_git_status_wraps_node_id`; `node_subscription_keeps_tauri_channel_inside_gateway` |
| `ui/src/app/shell/app-shell.test.tsx` | `custom_nodes_navigation_renders_the_feature`; `node_repair_uses_existing_setup_facade`; `node_restart_uses_runtime_action_result` |

Required Rust tests for WP1:

- `registry_dto_preserves_raw_metadata_and_unknown_values`
- `git_status_is_read_only_and_reports_relative_changes`
- `git_status_rejects_links_and_reports_missing_git`
- `pending_reviews_exclude_direct_operations_and_restore_no_consent`
- `node_list_exposes_recent_failures_and_correct_launch_blocking`
- Existing ACL completeness/local-manager/ComfyUI-origin/remote-origin tests must include the new command.
- Existing `bindings_up_to_date` must pass without rewriting files.

Acceptance evidence must additionally demonstrate:

| Scenario | Required evidence |
|---|---|
| **10** | Consent records exact Registry version; conflicting torch requirement remains blocked without explicit pin approval; torch unchanged in the ordinary install path. |
| **11** | Dirty Git update preserves edits and displays actual status; enable/disable round-trips through `.disabled/`. |
| **14** | Kill before publication cleans staging without changing live nodes; later crash boundaries preserve identities and recover conservatively. |
| **16** | Start during node maintenance is refused/waits; journal validation occurs under the lease. |
| **17** | Failed dependencies leave node/environment failure visible; Repair restores required node dependencies under unchanged protected pins before activation. |

Finish with keyboard, focus-trap/Esc, both-theme, offline, long Unicode-name and 200% zoom checks on the actual manager UI.

## 6. User-only questions

None.