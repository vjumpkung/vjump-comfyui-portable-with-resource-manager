# M5 backend audit evidence

Read-only inspection of the requested sources and tests. No files were changed; no build, test, or package commands were run.

## Port/journal types

- **`NodeFiles` port** — `crates/application/src/custom_nodes/ports.rs:31–67`. Its trait-level docs say paths derive from the configured root and validated node IDs, and mutations use “shared pinned parents/no-replace primitives.” Only `recover` has a method-level contract: uncertain copies remain and block start (`:60`). The other methods have no method-level doc comments.
  - `prepare(&self, root: &Path, operation: &NodeId, request: &PrepareRequest, prior: Option<&InstalledNode>, ctx: &StepContext) -> Result<StagedNode, NodeAppError>` (`:35`)
  - `snapshot(&self, root: &Path, node: &InstalledNode, staged: Option<&NodeId>) -> Result<TreeSnapshot, NodeAppError>` (`:43`)
  - `path(&self, root: &Path, node: &InstalledNode, staged: Option<&NodeId>) -> PathBuf` (`:49`)
  - `publish(&self, root: &Path, operation: &NodeOperation) -> Result<(), NodeAppError>` (`:50`)
  - `backup(&self, root: &Path, operation: &NodeOperation) -> Result<(), NodeAppError>` (`:51`)
  - `move_enabled(&self, root: &Path, prior: &InstalledNode, enabled: bool) -> Result<(), NodeAppError>` (`:52`)
  - `recycle(&self, root: &Path, operation: &NodeOperation) -> Result<(), NodeAppError>` (`:58`)
  - `discard_staging(&self, root: &Path, operation: &NodeId) -> Result<(), NodeAppError>` (`:59`)
  - `recover(&self, root: &Path, operation: &NodeOperation) -> Result<RecoveryAction, NodeAppError>` (`:61`)
  - `scan(&self, root: &Path) -> Result<Vec<InstalledNode>, NodeAppError>` (`:66`)

- **Journal and review records** — `crates/application/src/custom_nodes/model.rs`.
  - `OperationPhase` (`:152`) has `Prepared`, `PublishIntent`, `OldBackedUp`, `Published`, `DependenciesInProgress`, `Verified`, `CleanupPending`, `Completed`, `Failed`. There is no `Staged` phase. `blocks_start()` lists only `PublishIntent`, `OldBackedUp`, `Published`, and `DependenciesInProgress` (`:179`).
  - `NodeOperation` (`:190`) is the persisted operation journal. It records `id`, `kind`, `phase`, `node`, optional `prior`, `environment`, `pins`, `binding`, optional `artifact_digest`, `requirements_digest`, `hook_digest`, optional `consent_identity`, failure, and derived relative `staging` and `backup` strings plus dependency requirements (`:191–208`). It has no explicit destination path or backup digest.
  - The node/prior records carry name, source, enabled state, and `tree_digest` via `InstalledNode` (`crates/domain/src/custom_nodes/installed_node.rs:33–45,143–153`). `NodeSource::Git` stores the exact commit; `NodeSource::Registry` stores ID, publisher, version, and repository (`:33–45`).
  - `PlanReview` (`model.rs:210`) exposes the source, tree/artifact/requirements/hook digests, environment/pins, protected changes, warnings, trust flags, and requirements/hook presence. `StagedNode` (`:233`) carries the corresponding prepared source/digests/dependencies/warnings; `TreeSnapshot` (`:245`) carries digest, requirements digest, hook digest, and parsed requirement lines.
  - `RecoveryAction` (`model.rs:253`) is `Discarded`, `RolledBack`, `RolledForward`, `CleanupPending`, or `Blocked`.
  - `consent_identity` is set as a formatted string containing plan ID, binding, hook choice, warning acknowledgement, and confirmed-pin count (`crates/application/src/custom_nodes/workflow.rs:463–470`); it does not itself encode a tree or backup digest.

- **Digest coverage**:
  - `tree::tree_digest(root)` (`crates/infrastructure/src/custom_nodes/tree.rs:80–130`) hashes sorted relative file names and each file’s SHA-256. It excludes the top-level `.git`, app-owned `.vjump-node.json` and `.tracking`, and `__pycache__`; it rejects links encountered in the walk (`:34–40,89–124`).
  - `read_tree(dir, git_node)` (`files.rs:153–177`) uses `git:<commit>` for a Git node whose commit metadata can be read; otherwise it calls `tree_digest`. It separately SHA-256 hashes root `requirements.txt` text and a regular, non-link root `install.py`.
  - Registry artifact SHA-256 is accumulated over downloaded ZIP bytes in `RegistryHttp::download` (`registry.rs:157–199`). Update stubs hash their requirements/hook text in `read_tree_files_only` (`files.rs:748–757`). Recovery’s `digest_of` hashes single files as `file:<sha256>` or reads the directory digest (`files.rs:771–776`).

## Finding 2 — Recovery identity checks

- `recover` delegates to `recover_blocking` (`crates/infrastructure/src/custom_nodes/files.rs:714–727,780–896`). The `Verified`/`CleanupPending` path discards staging and recycles any existing backup based on existence; it does not compare either copy with a recorded digest:

  ```rust
  793 if matches!(op.phase, OperationPhase::Verified | OperationPhase::CleanupPending) {
  794     discard(paths);
  795     if exists(&backup) && (recycler(&backup).is_err() || exists(&backup)) {
  796         return Ok(RecoveryAction::CleanupPending);
  ```

- Enable/disable recovery classifies the move using only the two paths’ existence (`:807–814`):

  ```rust
  811 match (exists(&from), exists(&to)) {
  812     (true, false) => RecoveryAction::RolledBack,
  813     (false, true) => RecoveryAction::RolledForward,
  ```

- Uninstall recovery likewise uses `exists(live)`/`exists(backup)` and may restore the backup without checking its contents against `prior` (`:816–830`). Install/update recovery checks a published final path’s digest in some branches (`:846–854`), but the restore branches call `restore(&backup, &final_path)` based on presence (`:871–877`). The prior-digest closure checks the final path, not the backup (`:850–854`).
- `recover_blocking` also calls `paths.ensure_layout().ok()` (`:804`), so a layout-validation error is ignored there.

## Finding 3 — Staged validation and Git update

- **Registry staging:** `prepare_registry` extracts to staging, rejects an archive-supplied app marker, runs `validate_tree`, then takes a tree snapshot (`crates/infrastructure/src/custom_nodes/files.rs:288–300`). The staged `tree_digest` is returned in `StagedNode` (`:317–330`). At commit, `snapshot` is compared against the reviewed tree, requirements, and hook digests before publication (`crates/application/src/custom_nodes/workflow.rs:431–445`).
- **Git install:** `clone_checkout` validates the resolved commit and exact checked-out head (`git.rs:300–354`); `prepare_git_install` runs `validate_tree` on the checkout, writes marker exclusions, then calls `read_tree(..., true)` (`files.rs:370–390`). `tree::validate_tree` rejects links/reparse points and non-file/non-directory entries, with count/size limits (`tree.rs:20–63`). Git is configured with `core.symlinks=false` (`git.rs:106`).
- **Git update:** `prepare_git_update` checks the live node directory itself with `not_link`, confirms Git metadata, then calls `freeze_update` (`files.rs:423–455`). It reads target `requirements.txt` and `install.py` from the frozen commit into a staging stub (`:460–493`); it does not check out and validate the target’s full tree there. The staged digest is `git:<target>`.
- `freeze_update` inspects status before fetch, runs `fetch --tags origin`, inspects again, resolves an exact target commit, rejects a non-object ID, and for a branch requires the current commit to be an ancestor of the target (`git.rs:484–566`). `inspect` uses porcelain status including untracked files and records unfinished operations and ahead/behind counts (`:395–461`). `can_update` refuses dirty worktrees, unfinished operations, divergence (`ahead > 0 && behind > 0`), detached branch updates, or a missing upstream (`crates/domain/src/comfy_runtime/update.rs:67–89`).
- `apply` checks the frozen target is an object ID, checks current HEAD equals the expected prior head, re-runs `can_update`, then uses `merge --ff-only` for branches or detached checkout otherwise; afterward it checks HEAD equals the exact target (`git.rs:618–661`). There is no `validate_tree` call after apply.
- **Consent binding:** `prepare_git_update` stores the target commit in `NodeSource::Git.commit` and `git:<target>` in the staged tree digest (`files.rs:481–493`). `PlanReview` carries source and digest (`application/src/custom_nodes/model.rs:210–231`). `InstallConsent::check` compares plan ID and binding and checks code execution, warning acknowledgement, and hook permission (`domain/src/custom_nodes/consent.rs:15–20`). `commit_held` applies that check to the in-memory prepared plan, then rechecks staged snapshot digests (`workflow.rs:407–445`). For Git publish, the operation’s prior and target commits are passed to `apply` (`files.rs:607–622`). The consent payload has no separate commit field; the exact commit is bound through the prepared plan’s source/binding and validated during publish.

## Finding 4 — Install/update workflow and final digest

- `prepare_held` calls `files.prepare`, records the returned source and tree/artifact/requirements/hook digests in the operation, and builds `PlanReview` (`crates/application/src/custom_nodes/workflow.rs:149–179,232–255`).
- `commit_held` checks consent and stale-plan identity before recording `PublishIntent` (`workflow.rs:407–471`). `mutate_files` backs up an existing node, records `OldBackedUp`, then publishes; afterward the caller records `Published` (`:471–479,546–571`).
- The node is saved after publication (`:480–484`). `dependencies` records `DependenciesInProgress`, installs the reviewed requirements, then optionally runs root `install.py` with consent (`:655–724`), verifies the environment with `verify_final`, and saves the environment (`:729–745`). The caller then records `Verified`, saves the node, records `CleanupPending`, and recycles the backup (`:517–538`).
- **Digest is not refreshed after the hook:** `op.node.tree_digest` is assigned from `prepared.tree_digest` at `workflow.rs:170–177`. No post-hook `files.snapshot` call appears before `Verified` or `save_node` (`:517–522`). Thus the persisted tree digest remains the prepared digest. For Git, `read_tree` uses `git:<commit>` rather than a worktree hash (`files.rs:154–160`).

## Finding 5 — Registry metadata and review warnings

- `RegistryHttp::release` propagates errors from the release endpoint (`crates/infrastructure/src/custom_nodes/registry.rs:139–154`). `prepare_registry` then makes a separate node-detail request with `self.registry.node(id).await.ok()` (`files.rs:252`), which drops any error. The resulting node detail is used for publisher/repository (`:253–263`); its metadata is not merged into release warnings.
- Release metadata extraction reads supported OS, accelerators, ComfyUI version expression, status, and deprecated (`registry.rs:275–282`). `compat::evaluate` derives OS/accelerator/ComfyUI support plus flagged/deprecated booleans (`crates/infrastructure/src/custom_nodes/compat.rs:100–110`). Status is considered flagged when it contains `banned`, `deleted`, `flagged`, or `pending` (`crates/domain/src/custom_nodes/compatibility.rs:38–42`). `Compatibility::warnings` emits incompatible/unknown support warnings and flagged/deprecated warnings (`:55–67`).
- `prepare_registry` computes warnings from **release** metadata, adds a deprecated warning if needed, and puts them on `StagedNode` (`files.rs:317–330`). `prepare_held` transfers them into `PlanReview.warnings` (`workflow.rs:178,232–255`); `InstallConsent::check` requires acknowledgement when warnings exist (`consent.rs:15–20`).
- Search endpoint behavior differs: `search` returns cached pages marked stale on `Offline`, while other errors propagate (`registry.rs:403–460`). `get_json` maps 429/5xx to `Offline` and other non-success statuses to `RegistryStatus` (`:87–103`).

## Finding 7 — Layout and parent validation

- `Paths::ensure_layout` uses `comfy.is_dir()` for `root/ComfyUI` (`crates/infrastructure/src/custom_nodes/files.rs:88–92`), which follows a symlink. It uses `symlink_metadata` plus `is_link`/`is_dir` checks for `custom_nodes` and `.disabled` (`:93–98`). It creates the hook directory with `create_dir_all` (`:100`).
- Operations call `ensure_layout` and then path-based filesystem operations. The custom-node helper is `tree::rename_no_replace(from: &Path, to: &Path)` (`crates/infrastructure/src/custom_nodes/tree.rs:142–180`); it does not accept pinned parent handles. This differs from the `NodeFiles` trait’s pinned-parent contract. `Paths` derives the live, staging, and backup paths from the root and node/operation names (`files.rs:51–85`).
- `files.rs` checks links on selected node/staging directories with `not_link`, and `validate_tree` checks entries inside validated trees. Those checks do not pin the `ComfyUI`/`custom_nodes` parent chain for mutations.

## Reusable helpers

- The model library uses `model_paths` pinned-directory primitives:
  - `AuthorizedModelRoots::new(roots: impl IntoIterator<Item = (String, PathBuf)>) -> io::Result<Self>` (`crates/infrastructure/src/shared/model_paths.rs:25`); canonicalizes and pins each root (`:31–35`).
  - `AuthorizedModelRoots::parent(&self, root_id: &str, path: &RelativeModelPath, create: bool) -> io::Result<(Arc<PinnedDirectory>, SafeFileName)>` (`:37–56`). `RelativeModelPath::new(value: &str) -> Result<Self, ModelValueError>` validates path components (`crates/domain/src/shared/model_values.rs:53–80`).
  - `pin_absolute(path: &Path) -> io::Result<Arc<PinnedDirectory>>` (`model_paths.rs:58`); `PinnedDirectory::child(self: &Arc<Self>, name: &SafeFileName, create: bool) -> io::Result<Arc<PinnedDirectory>>` (`:112`); `PinnedDirectory::open(&self, name: &SafeFileName, write: bool, create_new: bool) -> io::Result<File>` (`:153`).
  - `reject_reparse(file: &File) -> io::Result<()>` (`:216`); `rename_no_replace(source: &PinnedDirectory, name: &SafeFileName, file: &File, target: &PinnedDirectory, new_name: &SafeFileName) -> io::Result<()>` (`:280`). These use opened directory/file handles; Unix traversal uses `openat`/`O_NOFOLLOW`, and Windows opens with reparse-point checks (`:88–109,125–145,153–197`).
  - `LibraryRoots::pinned_directory(&ModelLocation) -> io::Result<Arc<PinnedDirectory>>` (`crates/infrastructure/src/models/library/roots.rs:274`) supplies pinned directories to `models/library/file_ops.rs`, which uses `model_paths::rename_no_replace` (`:115–120,169–174`).
- `shared/fs.rs` provides `atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()>` and `nearest_existing(path: &Path) -> Option<&Path>` (`:8,26`); these are not parent-containment primitives.
- `custom_nodes/tree.rs` also exposes `is_link(meta: &fs::Metadata) -> bool` (`:20`) and `validate_tree(root: &Path) -> Result<(), NodeAppError>` (`:45`), which detect entry links/reparse points within a tree.

## Existing tests and helpers

- **Finding 2:** `install_recovery_at_every_boundary` (`crates/infrastructure/src/custom_nodes/tests.rs:997`) checks staged discard, matching published tree roll-forward, changed live tree blocking, ambiguous staging/live blocking, and prepared-plan discard. `update_recovery_preserves_the_only_recoverable_copy` (`:1070`) exercises update location combinations, restoring the old backup and cleanup after `CleanupPending`. `uninstall_and_toggle_recovery` (`:1174`) checks uninstall restore/recycle and enable/disable classification. These use ordinary directory existence and expected tree digests; they do not test a modified backup identity. Application tests `a_failed_publish_is_settled_from_the_disk_state` (`crates/application/src/custom_nodes/tests.rs:944`) and `startup_reconciliation_applies_the_file_outcome_to_the_records` (`:1015`) check action-to-record/phase handling.
- **Finding 3:** `a_registry_release_is_staged_published_and_recorded_with_provenance` (`infrastructure/tests.rs:473`) checks staged snapshot digest, artifact digest, provenance, and no-overwrite publication. `malicious_archives_write_nothing_outside_owned_staging` (`:654`) checks rejected archive entries leave no escaped files. `git_install_resolves_one_immutable_commit_and_tracks_a_branch` (`:1546`) checks exact commit/branch metadata; `git_symlink_entries_are_not_materialized` (`:1690`) checks installed entries are not links. `git_update_fast_forwards_in_place_and_never_discards_edits` (`:1752`) checks frozen target, dirty/untracked refusal, late edit refusal, and stale head; `git_diverged_and_detached_nodes_need_an_explicit_target` (`:1865`) checks divergence/detached behavior; `git_update_recovery_compares_the_head_with_the_recorded_commits` (`:1924`) checks old/new/unknown HEAD outcomes.
- **Finding 4:** `an_install_follows_the_phases_and_records_the_exact_source` (`application/tests.rs:383`) checks phase order, publish before package install, and no hook without consent. `install_py_runs_only_with_consent_after_requirements_and_trust_is_namespaced` (`:737`) checks package install before hook and scoped remembered trust. `protected_pins_are_reasserted_after_packages_and_after_a_failing_script` (`:696`) checks pin reassertion after a failing hook. These tests do not assert a post-hook filesystem digest refresh.
- **Finding 5:** `incompatible_registry_metadata_becomes_review_warnings` (`infrastructure/tests.rs:548`) checks release-level OS, ComfyUI, accelerator-unknown, deprecated, and flagged warnings. `offline_results_are_stale_and_refusals_are_failures` (`:348`) checks stale cached search and 403 refusal. Application `warnings_need_acknowledgement_and_unknown_plans_are_stale` (`application/tests.rs:490`) checks acknowledgement gating. These do not cover node-detail GET failure or node-level status/deprecation merging.
- **Finding 7:** `linked_node_folders_are_never_moved_or_backed_up` (`infrastructure/tests.rs:930`) checks a linked node path is refused and external content preserved. `tree.rs` test `unix_symlinks_are_rejected` (`:221`) checks tree-level link rejection. No custom-node test found for a linked/reparse `ComfyUI` parent or substitution of the parent chain.
- **Available test helpers/fakes:** infrastructure `Fixture` builds a temp ComfyUI root, loopback `TestServer`/`Route`, registry/cache, and recycler (`infrastructure/tests.rs:92–147`); `registry_node`, `latest`, and `good_zip` build fixtures (`:171–219`); `operation`, `node`, and `dirs` build recovery state (`:221–260,988`). Git coverage uses `Repo`, `GitFixture`, and `require_git!` (`:62–71,1401–1471`). Application tests use `FakeFiles`, `FakeRegistry`, `FakeStore`, `FakeHook`, and `Harness` (`application/tests.rs:23–40,42–250,252–363`). `FakeHook` records calls and can fail but does not mutate node files (`:229–249`).

## Open uncertainties

- Existing recovery tests establish ordinary crash-boundary behavior, but do not establish identity safety for a foreign destination, modified backup, or changed published tree in `Verified`/`CleanupPending`.
- Registry coverage tests release-level warnings and search refusals; it does not establish how a failed `/nodes/{id}` detail request should affect install review.
- Git update tests cover frozen commits and pre-apply dirty/stale checks, but not post-apply tree validation or link/reparse changes after apply.
- The model-library pinned-path helpers are concrete and public in infrastructure, but their API is model-root/location-specific; no generic custom-node pinned-root adapter is present in the inspected files.