## 1. No-go

**No-go on the draft as written.** Its main findings are valid, but several choices would leave M5 acceptance gaps:

- Non-repair replacement environments could activate after node dependency failure.
- Git commit IDs alone cannot detect modified node files or legitimate hook mutations.
- Ambiguous recovery in `Verified`/`CleanupPending` could remain non-blocking.
- Immediate path revalidation does not satisfy the explicit race requirement in NFR-SEC-5.
- Node dependency previews currently permit protected-package changes through an unconstrained fallback.
- Git update refusals discard the status required by acceptance scenario 11.
- Stop/rescan integration misses internal stop callers.

The corrected plan below resolves these choices. **This was a read-only review; no files were changed and no builds or tests were run.** All HANDOFF §3 decisions remain fixed.

## 2. Corrected plan

Paths below are relative to the repository root. Execute these packages sequentially.

### WP1 — Implement operation-time filesystem containment

**Files**

- `crates/infrastructure/src/shared/model_paths.rs`
- `crates/infrastructure/src/shared/trash.rs`
- New: `crates/infrastructure/src/custom_nodes/paths.rs`
- `crates/infrastructure/src/custom_nodes/{mod,files,tree,stack,tests}.rs`
- `crates/application/src/custom_nodes/ports.rs`

**Reason:** `Paths::ensure_layout()` follows a linked `ComfyUI`; node mutations use path-based renames despite the pinned-parent contract. NFR-SEC-5 explicitly includes races between checking and use.

1. Add directory-capable entry opening beside the existing file-only API:

   ```rust
   impl PinnedDirectory {
       pub fn open_entry(&self, name: &SafeFileName) -> io::Result<File>;
       pub fn remove_owned_tree(&self, name: &SafeFileName) -> io::Result<()>;
   }
   ```

   `open_entry` accepts regular files or directories, rejects links/reparse points and special entries, and grants the access needed for handle-based renaming. Keep `open()` file-only.

2. Generalize the existing shared `rename_no_replace` implementation to validate its source handle through `open_entry`, so it supports node directories without changing its signature.

3. Add a pinned-entry trash entry point:

   ```rust
   pub fn recycle_entry(
       parent: &PinnedDirectory,
       name: &SafeFileName,
       expected: &FileIdentity,
   ) -> io::Result<()>;
   ```

   It must recycle the validated entry while retaining parent containment protection. Do not fall back to permanent deletion. On Linux, source placement into freedesktop trash must use directory-relative operations; merely dropping the handle and passing the original path to `trash::delete` leaves the race unresolved. Unsupported or cross-volume cases may fail safely and retain the backup.

4. Move node layout handling into `custom_nodes/paths.rs`:

   ```rust
   pub(crate) struct NodePaths { /* retained pinned parent handles */ }

   impl NodePaths {
       pub(crate) fn open(root: &Path, create: bool)
           -> Result<Self, NodeAppError>;

       pub(crate) fn location(
           &self,
           node: &InstalledNode,
       ) -> (Arc<PinnedDirectory>, SafeFileName);

       pub(crate) fn staging(
           &self,
           operation: &NodeId,
       ) -> (Arc<PinnedDirectory>, SafeFileName);

       pub(crate) fn backup(
           &self,
           operation: &NodeId,
       ) -> (Arc<PinnedDirectory>, SafeFileName);
   }
   ```

   Retain pins for the authorized install root, `ComfyUI`, `custom_nodes`, `.disabled`, and the empty hooks directory. Traverse components without following links. On Unix, do not begin with an absolute open that can follow substituted intermediate components.

5. Route **all** node mutations through these protected parents: staging creation/download/extraction, hoisting, publication, backup, enable/disable, restore, staging removal, and recycling. Keep protection alive through subprocess and cleanup completion.

6. Replace the node recycler alias with:

   ```rust
   pub type Recycler = Arc<
       dyn Fn(&PinnedDirectory, &SafeFileName, &FileIdentity)
           -> io::Result<()>
           + Send
           + Sync
   >;
   ```

   Update `NodeOptions::default()` and fixture recyclers accordingly.

7. Remove node uses of the path-based `tree::rename_no_replace`. Keep unrelated model-library behavior outside this package; verify shared helper compatibility with existing library tests.

**Errors:** containment/link refusal → `NodeAppError::UnsafeSource` / IPC `node_unsafe_source`; ordinary I/O failure → `Filesystem` / `node_filesystem`. During recovery, preserve copies and unresolved state.

**Tests in infrastructure**

- `linked_comfyui_and_disabled_parents_are_refused`: external sentinel contents remain untouched.
- `node_mutations_resist_parent_substitution`: substitute a parent at a controlled check/use boundary; publication, restore, cleanup and recycling cannot affect the substituted tree.
- `pinned_directory_renames_never_replace_a_destination`: directory and file cases.
- `failed_safe_recycling_retains_the_backup`: no permanent-delete fallback.

Windows junction coverage and Linux substitution coverage must execute on their respective platforms.

### WP2 — Bind Git reviews to file contents and preserve refusal status

**Files**

- `crates/infrastructure/src/custom_nodes/{files,git,tree,tests}.rs`
- `crates/application/src/custom_nodes/model.rs`
- `crates/desktop/src/ipc/node_dto.rs`

1. Make directory `TreeSnapshot.digest` a deterministic **content tree digest for Git and registry nodes**. Keep the exact Git commit in `NodeSource::Git.commit`.

   `read_tree()` must call `validate_tree()` and `tree_digest()`; remove its `git:<commit>` shortcut. Validate `.git` itself before using Git metadata.

2. Replace Git update requirement/hook stubs with a full staged checkout of the frozen target. Reuse `GitNodes::clone_checkout` with:

   ```rust
   Resolved {
       commit: target.clone(),
       branch: None,
       kind: RefKind::Commit,
   }
   ```

   Resolve/fetch the target once. Do not resolve another target during staging or commit. The live update remains in-place ff-only/detached checkout using `apply`; the staged checkout supplies complete review and validation evidence.

   This removes `read_tree_files_only` and the “missing `.git` means update stub” branch. It also removes the need for a special stub-file whitelist.

3. Require clean staged Git checkouts during preparation and commit revalidation. After live `apply`, verify:

   - HEAD equals the frozen target;
   - normalized origin matches the reviewed source;
   - no unfinished operation or dirty worktree exists;
   - the tree passes link/reparse validation;
   - its content digest equals the reviewed staged digest.

   Perform these checks **before dependencies or hooks run**. Failure must preserve edits and remain unresolved; never reset or clean.

4. Do **not** require every live snapshot to be clean. Dirty Git nodes must remain inspectable and eligible for disable/uninstall; consented hooks may also leave a dirty final tree.

5. Preserve status on refusal:

   ```rust
   NodeAppError::GitRefused { status: String }
   ```

   Populate it from the bounded, redacted `Inspection.state.status` in `freeze_update` and `apply`. Include a safe explanation for divergence/upstream refusal when status is empty. Update all existing constructors and matches.

   Keep IPC code `node_git_refused`; include the bounded node-relative status in its sanitized message. Do not expose absolute paths or raw subprocess errors.

**Tests in infrastructure**

- `git_staged_content_changes_invalidate_the_review`: tracked, untracked and ignored-file mutations change the snapshot or cause refusal.
- `git_update_stages_the_complete_frozen_tree`: non-root files participate in the reviewed digest.
- `git_post_apply_validation_precedes_dependency_execution`: invalid post-apply contents are rejected.
- Extend `git_update_fast_forwards_in_place_and_never_discards_edits`: status is retained; dirty/diverged/late-edit refusal still preserves files.
- `dirty_git_nodes_can_still_be_disabled_and_uninstalled`.

Update existing tests that assume `git:<commit>` is a complete tree identity.

### WP3 — Enforce node constraints and retain registry warnings

**Files**

- `crates/application/src/custom_nodes/workflow.rs`
- `crates/application/src/custom_nodes/tests.rs`
- `crates/infrastructure/src/custom_nodes/{files,registry,tests}.rs`
- `crates/desktop/src/ipc/node_dto.rs`

**Protected packages**

The current `preview_packages()` retries without constraints and can produce an authorized torch change. Acceptance scenario 10 requires rejection for a conflicting node requirement.

1. Remove the unconstrained fallback from node previews.
2. If constrained resolution fails, return `NodeAppError::ResolutionFailed`.
3. If a preview nevertheless reports any protected-package change, return `ResolutionFailed`; do not offer it as a node plan.
4. Node commits require an empty protected-change list. In `dependencies()`, retain `env.pins` unchanged.
5. Preserve protected-package confirmation for the separate FR-ENV-3 package-management workflow.

Keep IPC `node_resolution_failed`; clarify its message that dependencies could not resolve under protected-package constraints and no package changed.

**Registry metadata**

1. In `prepare_registry`, propagate `registry.node(id).await?`; remove `.ok()` and the silent metadata fallback.
2. Build review warnings from both node-level and exact-release metadata. Include flagged/deprecated state and compatibility warnings; deduplicate identical strings deterministically.
3. Keep flags as acknowledged warnings when the service permits installation. Propagate service refusals.

Existing mappings remain:

- 403/404 → `RegistryStatus(code)` / `registry_refused`
- 429/5xx/network failure → `Offline` / `registry_offline`
- Invalid/mismatched body → `InvalidRegistry` / `registry_invalid`

**Tests**

Application:

- Replace `reviewed_protected_package_changes_must_be_confirmed_exactly` with `node_previews_refuse_protected_package_changes`.
- `node_preview_never_retries_without_constraints`: one constrained preview, zero package mutation/publication.
- Retain `warnings_need_acknowledgement_and_unknown_plans_are_stale`.

Infrastructure:

- `registry_detail_failures_abort_preparation`: 403, 404, 500 and invalid/mismatched detail bodies fail before artifact publication.
- `registry_node_and_release_warnings_are_bound_to_review`: node-level flagged/deprecated and release compatibility warnings survive, without duplicate warnings.

### WP4 — Persist final contents and make recovery identity-based

**Files**

- `crates/application/src/custom_nodes/{model,workflow,use_cases,tests}.rs`
- `crates/infrastructure/src/custom_nodes/{files,tests}.rs`
- `crates/desktop/src/ipc/node_dto.rs`

**Successful dependency completion**

1. Before changing the operation to `Verified`, snapshot the published node again.
2. When no hook ran, require equality with the reviewed content digest. Unexpected mutation is a failure.
3. When the consented hook ran, accept a fresh, link-free final digest while retaining the reviewed source revision. A hook changing source/provenance does not authorize a different source.
4. Persist the final digest in `op.node` and the node record. Keep the original requirement/hook digests as the record of what was reviewed.
5. Perform final filesystem validation **before** clearing the environment’s repair state and recording dependency success.
6. If final validation fails, retain the backup and a blocking journal phase; preserve/restore `RepairRequired`. Never convert that boundary into ordinary non-blocking cleanup.

Adjust `FakeFiles::publish` to model the staged contents becoming live. Do not weaken equality checks to accommodate its current `"tree:installed"` default.

**Blocking state without another migration**

Add:

```rust
impl NodeOperation {
    pub fn blocks_start(&self) -> bool {
        self.phase.blocks_start()
            || (!matches!(
                self.phase,
                OperationPhase::Completed | OperationPhase::Failed
            ) && self.failure.is_some())
    }
}
```

In `settle()`:

- `RecoveryAction::Blocked` records a fixed, sanitized recovery failure and preserves the phase.
- Successful reconciliation clears the previous blocking failure before applying its resulting state.
- `ensure_reconciled()` checks `NodeOperation::blocks_start()`, not only the phase.

Thus ordinary recycle failure remains `CleanupPending` and non-blocking; changed/ambiguous copies block launch even when discovered after verification.

Correct the `NodeListDto.unresolved` documentation: not every retained cleanup operation blocks launch.

**Recovery rules**

Validate layout before every branch. Remove `ensure_layout().ok()`.

| Operation/state | Required evidence before action |
|---|---|
| Prepared, unpublished | Remove only protected, operation-owned staging; leave live nodes/backups untouched. |
| Enable/disable | Exactly one expected location exists and its content matches the recorded node. Both present, foreign content, or uncertain identity → `Blocked`. |
| Uninstall before verification | Restore only a backup matching `prior`; destination must be absent. |
| Verified uninstall | Live destination must be absent; recycle only a matching prior backup. An already-absent backup may complete. |
| Install/update before verification | Classify live/staging/backup independently. Restore only a matching prior backup; never overwrite a destination. |
| Verified install/update/retry | Published source and final content must match the recorded final node. Any retained backup must match `prior` before recycling. |
| Git update | Check source, commit **and content**, rather than HEAD alone. Preserve all edits and copies on uncertainty. |

For interrupted `DependenciesInProgress`, first establish the published source/location safely, mark the environment and outcome failed, and retain the backup. A partially executed hook is not proof of successful verification. Missing/foreign/unsafe publication stays unresolved.

Legacy `git:<commit>` journal entries do not prove content identity. Preserve ambiguous legacy copies rather than manufacturing proof and recycling them.

**Tests**

Application:

- `consented_hook_final_digest_is_saved_before_verified`
- `unexpected_tree_mutation_without_a_hook_blocks_completion`
- `final_snapshot_failure_preserves_repair_and_backup`
- `blocked_verified_cleanup_blocks_start_but_trash_failure_does_not`
- `interrupted_dependency_mutation_preserves_failure_state`

Infrastructure:

- `recovery_preserves_foreign_destinations`
- `verified_recovery_preserves_changed_published_trees`
- `recovery_never_restores_or_recycles_a_modified_backup`
- `toggle_recovery_requires_matching_content`
- Extend all three existing boundary tests: `install_recovery_at_every_boundary`, `update_recovery_preserves_the_only_recoverable_copy`, and `uninstall_and_toggle_recovery`.

Assert both the returned action and the contents/existence of every surviving copy.

### WP5 — Restore exact node requirements during replacement provisioning

**Files**

- `crates/domain/src/custom_nodes/installed_node.rs`
- `crates/application/src/environment/{ports,deps,execute}.rs`
- New: `crates/application/src/custom_nodes/requirements.rs`
- `crates/application/src/custom_nodes/{mod,ports,use_cases,workflow,tests}.rs`
- `crates/infrastructure/src/custom_nodes/{sqlite,stack,tests}.rs`
- `crates/infrastructure/src/environment/source.rs`
- `crates/infrastructure/src/environment/source/tests.rs`
- `crates/desktop/src/state.rs`
- Constructor/fake updates:
  - `crates/application/src/environment/tests/{fakes,setup}.rs`
  - `crates/application/src/comfy_runtime/tests.rs`
  - `crates/infrastructure/tests/{workflow,real_setup}.rs`
  - `crates/desktop/tests/support/mod.rs`

**Port and provider**

Add to `environment/ports.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeRequirementSet {
    pub node_id: NodeId,
    pub name: SafeFileName,
    pub source_identity: String,
    pub file: Option<PathBuf>,
    pub extra: Vec<String>,
}

#[async_trait]
pub trait NodeRequirementSource: Send + Sync {
    async fn requirement_sets(
        &self,
        root: &Path,
    ) -> Result<Vec<NodeRequirementSet>, ProvisionError>;

    async fn record_outcome(
        &self,
        environment: &EnvId,
        set: &NodeRequirementSet,
        state: DependencyState,
    ) -> Result<(), ProvisionError>;

    async fn needs_repair(
        &self,
        environment: &EnvId,
        root: &Path,
    ) -> Result<bool, ProvisionError>;
}
```

Add `EnvironmentDeps.node_requirements: Arc<dyn NodeRequirementSource>`.

Implement it in application `custom_nodes/requirements.rs`:

```rust
pub struct NodeRequirements {
    store: Arc<dyn NodeRepository>,
    files: Arc<dyn NodeFiles>,
}

impl NodeRequirements {
    pub fn new(
        store: Arc<dyn NodeRepository>,
        files: Arc<dyn NodeFiles>,
    ) -> Self;
}
```

It must not contain `EnvironmentDeps` or `NodeService`; this avoids a construction cycle.

Add the repository reader:

```rust
async fn dependency(
    &self,
    node: &NodeId,
    environment: &EnvId,
    source_identity: &str,
) -> Result<Option<DependencyState>, NodeAppError>;
```

Read `node_env_deps` using all three keys. Unknown/corrupt persisted states are `Storage`, not success.

**Identity and requirement selection**

- Add pure `InstalledNode::dependency_identity(&self) -> String`, length-framing `source.identity()` and `tree_digest`. Use it consistently for outcome writes and reads. This distinguishes manual edits and same-version content changes without a schema change.
- Observe enabled **and disabled** installed nodes through `NodeFiles`; merge stored provenance conservatively and snapshot current contents.
- Reject unresolved blocking publication or provenance conflict; do not silently build from ambiguous state.
- Include each node’s root requirement file.
- Add persisted registry-only dependency strings from the newest consented Install/Update/RetryDependencies journal record matching that node ID and exact `NodeSource::identity()`.
- Define repository operation ordering as oldest creation first, with SQLite `rowid` as the deterministic equal-timestamp tie-breaker. Select the last matching consented record.
- An externally changed source receives file requirements only. Never resolve a registry release during Repair.
- Do not import node Python or run `install.py`.

Remove the unused `NodeService::repair_requirements()` and replace its test with provider coverage.

Make `ComfySource::requirement_files()` responsible only for ComfyUI’s requirement file: remove `RequirementFiles.nodes` and its filesystem scan. Update its implementations and callers.

**Provisioning behavior**

In `Execution::comfy_requirements()`:

1. Install core ComfyUI requirements.
2. Obtain node sets and install each file plus persisted extra requirements under the existing protected constraints.
3. Reassert pins after every attempted dependency operation, including failure.
4. Record `Ok` only after installation and pin verification succeed; otherwise record `Failed`.
5. **Any node requirement failure is fatal in every replacement mode.** Preserve the old active environment; do not activate the candidate. Preserve command failure information and cancellation behavior.

In `repair_check()`:

- `RepairRequired` always proceeds to replacement. Core import/pin verification alone cannot clear it.
- `NothingToRepair` is allowed only for an already `Verified` environment whose core/pins pass and whose current node requirements have matching successful outcomes.
- Missing/non-OK outcomes for a node with requirements require replacement.
- Failed outcomes remain associated with the old environment after successful replacement.

Hooks remain optional and are not automatically replayed. Repair success establishes the rebuilt environment and requirements, not successful execution of a previously failed arbitrary hook.

**List/scan projection**

`NodeService::list()` projects dependency state from the active environment and exact `dependency_identity()`. Missing outcome → `NotRun`, including unknown/manual nodes; do not invent `Ok`.

`scan_held()` refreshes content/source identity and uses that same projection. Keep `nodes.dependencies` for compatibility, but never use it as the authoritative cross-environment state. **No migration 007 is needed.**

**Composition**

Split adapter creation from environment binding in infrastructure `custom_nodes/stack.rs`:

```rust
pub struct NodeAdapters {
    pub store: Arc<SqliteNodes>,
    pub registry: Arc<RegistryHttp>,
    pub files: Arc<LocalNodeFiles>,
}

impl NodeAdapters {
    pub fn open(root: &Path, db: Db, options: NodeOptions) -> Self;

    pub fn bind(
        self,
        environment: EnvironmentDeps,
        epoch: String,
    ) -> NodeStack;
}
```

Keep `NodeStack::open/open_with` as wrappers for existing callers.

Desktop constructs adapters → `NodeRequirements` → `EnvironmentDeps` → bound `NodeStack`. Reuse the same adapters; do not construct a second independent stack or inject a late optional dependency.

Translate provider/storage errors to fixed `ProvisionError::Failed` descriptions; preserve meaningful node errors within the node facade.

**Tests**

Application:

- `repair_required_always_builds_a_replacement`
- `repair_failure_preserves_active_environment_and_failed_outcomes`
- `replacement_node_dependency_failure_never_activates` — exercise Repair, ChangeTorch, ChangePython and custom-venv replacement.
- `successful_repair_records_new_environment_outcomes_and_preserves_pins`
- `repair_replays_registry_dependencies_for_enabled_and_disabled_nodes`
- `repair_source_mismatch_uses_only_current_file_requirements`
- `repair_never_resolves_registry_or_runs_install_py`
- `node_dependency_state_follows_active_environment_and_content`

Change `repair_does_nothing_when_the_selected_environment_verifies` to seed an already `Verified` environment. Replace the warning expectation in `replacement_environments_receive_existing_node_requirements_under_the_pin_guard` with fatal/no-activation assertions.

Infrastructure:

- `dependency_lookup_uses_node_environment_and_exact_identity`
- `equal_timestamp_operations_have_deterministic_creation_order`

### WP6 — Hold launch/restart checks and required rescanning within orchestration

**Files**

- `crates/application/src/comfy_runtime/{ports,model,use_cases,update_workflow,tests}.rs`
- New: `crates/application/src/custom_nodes/runtime.rs`
- `crates/application/src/custom_nodes/mod.rs`
- `crates/desktop/src/{state,app_lifecycle}.rs`
- `crates/desktop/src/ipc/{comfy_commands,error}.rs`
- `crates/desktop/tests/support/mod.rs`

Add to runtime ports:

```rust
#[async_trait]
pub trait NodeLifecycle: Send + Sync {
    async fn before_start(
        &self,
        lease: &mut dyn MaintenanceLease,
    ) -> Result<(), RuntimeError>;

    async fn after_stop(
        &self,
        lease: &mut dyn MaintenanceLease,
    ) -> Result<(), RuntimeError>;
}
```

Add `RuntimeDeps.nodes: Arc<dyn NodeLifecycle>`.

Implement this port for `NodeService` in `custom_nodes/runtime.rs`:

- `before_start`: `reconcile_held`, then `ensure_reconciled`.
- `after_stop`: awaited `reconcile_held`, then `scan_held`.
- Never acquire the gate inside either method.

Add runtime errors:

```rust
RuntimeError::NodeRecoveryRequired
RuntimeError::NodeRescanFailed
```

Map them to fixed IPC codes/messages:

- `node_recovery_required`: interrupted custom-node work must be resolved before launch.
- `node_rescan_failed`: ComfyUI stopped, but custom nodes could not be rescanned; retry Rescan.

Map a running/unknown runtime during reconciliation to existing `NeedsStop`.

**Ordering**

- Start: try-acquire → node guard → existing runtime inspection/selection/spawn. Transfer the same lease to the supervisor.
- Restart: try-acquire once → verified stop → `after_stop` → guarded start under the same lease.
- Stop: call ungated supervisor stop first, so it can cancel a Start holding the gate; then **wait** for the gate, verify stop again under that lease, and await `after_stop`. The second stop closes the race with another Start winning the interval.
- Update-with-stop: invoke `after_stop` under its already-held lease before source/package changes.
- Application close already calls `StopComfy`; retain that route so it gains the same behavior.

Change `StopComfy::new` to accept `RuntimeDeps`.

Remove desktop pre-lease `ensure_startable()` and duplicate best-effort `after_stop()` calls. Do not swallow required reconciliation failures.

For notifications, add a thin desktop `NodeLifecycle` wrapper delegating to the application implementation and publishing `NodeEvent::Changed` after stop reconciliation, including failure. Business ordering remains in application.

At startup, reconcile **and scan** nodes before exposing normal services.

**Tests**

- `start_checks_node_recovery_under_the_maintenance_lease`
- `a_journal_failure_created_before_lease_acquisition_blocks_start`
- `restart_stops_rescans_and_starts_with_one_acquisition`
- `stop_cancels_start_before_waiting_for_the_gate`
- `stop_rechecks_termination_after_acquiring_the_gate`
- `after_stop_failure_is_returned_and_prevents_restart`
- `update_with_stop_rescans_under_its_existing_lease`

Retain the existing start gate-order and restart acquisition-count tests.

### WP7 — Finish opener and generated integration

**Files**

- `crates/desktop/src/ipc/node_commands.rs`
- `ui/src/shared/ipc/gateway.ts`
- Generated: `ui/src/shared/ipc/bindings.ts`
- Existing opener registration/permission/ACL files: inspect and retain; do not duplicate registration.

In `repository_url`, reject a missing **or empty** raw authority and raw userinfo delimiters before normalization.

Add:

- `repository_browser_urls_reject_empty_authority_and_userinfo`

Cover all five malformed spellings in WP-A of the draft, plus existing unsafe URL cases. Require the exact sanitized `validation` error and no shell invocation.

Add the gateway wrapper:

```ts
nodeOpenRepository: (repository: string) =>
  unwrap(() => commands.nodeOpenRepository(repository)),
```

Regenerate bindings through the explicit export test, then run drift and ACL checks. Regeneration is required because the command collection already changed, even if DTO shapes remain unchanged.

**Scope boundary:** the Custom Nodes page, consent-state UI fixes, virtualization and feature integration remain necessary for full M5 acceptance, but belong to the frontend package. Curated setup-target support, accelerators, moved-install rebuilding, updater, bandwidth, packaging and README remain outside this backend package.

## 3. Decisions and assumptions

All six draft questions are AI-decidable; none needs owner input.

| Question | Decision and evidence |
|---|---|
| **Q1 — Requirement port placement** | **Dedicated environment port, implemented by application `NodeRequirements` over repository/filesystem ports.** `NodeDeps` already contains `EnvironmentDeps`; a provider holding `NodeService` would create a cycle. Split adapter construction before binding. This follows ADR-0002 without coupling `GitSource` to node persistence. |
| **Q2 — Non-repair failures** | **Fatal in every replacement mode.** FR-SETUP-10/11 and §3.3.4 require verified activation and preservation of the previous working environment on failed setup. The current warning test is implementation precedent, not authority to waive those rules. |
| **Q3 — Missing outcomes/global column** | **Missing means `NotRun`; retain the column but stop treating it as authoritative.** Migration 006 already supplies the required keyed table. Include content in its opaque identity string; no schema migration is necessary. Legacy rows conservatively become unknown. |
| **Q4 — Final digest without a hook** | **Require equality when no hook ran; refresh after a consented hook.** `commit_held` currently persists the prepared digest, and Git snapshots currently omit worktree content. Unconditional refreshing would silently bless unreviewed mutations. |
| **Q5 — Containment depth** | **Implement pinned directory/entry operations.** Immediate revalidation plus documentation is insufficient: NFR-SEC-5 explicitly requires handling check/use races, and the existing port already promises pinned parents. Existing shared helpers need directory support; they are not directly sufficient as written. |
| **Q6 — Stop acquisition** | **Ungated cancellation, then waiting acquisition, verified stop and awaited reconciliation.** The supervisor’s port explicitly permits Stop to cancel a Start holding the lease. Acquiring first can deadlock cancellation; try-acquire can silently skip FR-NODE-10. |

Additional decisions:

- **Node operations cannot authorize torch changes.** FR-SETUP-7 and acceptance 10 govern this path. Explicit protected-package changes remain available through FR-ENV-3.
- **Ambiguous verified cleanup blocks launch; ordinary trash unavailability does not.** This preserves the recovery contract without making harmless retained backups fatal.
- **Full staged Git updates replace stubs.** This is the smallest uniform way to bind all files to review while retaining live ff-only application.
- **Creation order defines “latest matching consented journal record.”** Use deterministic SQLite creation ordering rather than random-ID ordering when timestamps tie.
- **Repair does not replay hooks.** This preserves HANDOFF §3 and the explicit-consent requirement; it cannot establish arbitrary node-specific runtime correctness.

## 4. Risks

- Filesystem race protection and trash behavior require native Windows/Linux verification. Path-only unit checks are insufficient.
- Full staged Git checkouts cost additional network traffic and disk space. A frozen commit disappearing upstream must fail safely, never select another commit.
- Legacy unresolved Git journals may lack sufficient content evidence for automatic cleanup. Preserve them and surface recovery state.
- Existing test fakes currently omit important filesystem transitions and classify registry-only installs differently; update them without weakening assertions.
- Repair can restore requirements and protected packages, but cannot undo arbitrary script effects or prove every node imports successfully.
- The frontend still has consent mismatches identified in `exp-c.out.md`. Backend completion alone cannot establish full M5 completion.
- The broken MSVC linker and concurrent compilation restriction prevent verification in this review. Do not assume the draft’s GNU override is a validated replacement.

## 5. Success checks

Run these **later**, with a working linker and coordinated build ownership; none was executed here.

```powershell
cargo fmt --all -- --check

cargo test -p domain custom_nodes --locked
cargo test -p application custom_nodes --locked
cargo test -p application environment::tests --locked
cargo test -p application comfy_runtime --locked
cargo test -p infrastructure custom_nodes --locked
cargo test -p infrastructure environment::source --locked
cargo test -p infrastructure models::library --locked

cargo test -p desktop --test bindings --locked -- --ignored export_bindings
cargo test -p desktop --test bindings --locked
cargo test -p desktop --test acl --locked
cargo test -p desktop node_commands --locked

cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
node --test scripts/check-arch-deps.test.mjs
node scripts/check-arch-deps.mjs

pnpm -C ui typecheck
pnpm -C ui lint
```

Run the named new tests from each package above, plus existing archive, consent, cancellation, pin-guard and recovery regressions. Git tests must actually execute; `require_git!` skipping is not acceptance evidence.

Retain ACL checks:

- `every_command_is_covered_by_the_acl_test`
- `manager_window_with_local_origin_can_invoke_all_commands`
- Both ComfyUI-window denial tests
- `manager_window_with_remote_origin_is_denied`

Required acceptance evidence:

| Scenario | Proof needed |
|---|---|
| **10** | Registry id@version and exact consent survive; conflicting torch requirements fail before mutation; protected versions remain unchanged. |
| **11** | Dirty/diverged Git update refuses with status; enable/disable round-trips through `.disabled/` without losing edits. |
| **14** | Kill/reopen before publication removes owned staging and preserves live nodes; later crash boundaries reconcile conservatively. Retain existing download recovery coverage. |
| **16** | Start waits/refuses throughout node mutation and checks the journal only while holding the lease. Existing model operations remain independent. |
| **17** | Dependency failure leaves `RepairRequired`; failed Repair preserves the old environment; successful Repair restores node requirements and unchanged protected versions. |

Complete native crash, recycling and junction/symlink checks on both release platforms before claiming M5 acceptance. Run coverage against the existing domain/application targets after integration.

## 6. User-only questions

None.