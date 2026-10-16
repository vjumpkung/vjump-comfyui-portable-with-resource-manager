# M5 backend completion plan (draft by main, for advisor review)

Goal: close HANDOFF.md "M5 backend audit findings" 1–7 plus the repository-opener gap, with regression tests at the real failure/recovery boundaries, preserving every fixed decision in HANDOFF §3 and REQUIREMENTS (§3.3 layering, §3.3.3 maintenance gate, §6.2 reliability, FR-NODE-4/6/7/8/9/10, FR-SETUP-10, NFR-SEC-3/5, acceptance 10/11/14/16/17).

Evidence: scratchpad `exp-a.out.md` (findings 1, 6) and `exp-b.out.md` (findings 2,3,4,5,7). Re-read code before editing.

Constraints: no serde/tokio/reqwest/rusqlite in domain/application; ports via async-trait `Arc<dyn …>`; composition only in desktop `state.rs`; no recursive gate acquisition (held-lease `*_held` methods); never re-resolve a different registry release, import node Python, or run install.py without consent; Git never reset/force/discard; tokens never logged. Regenerate bindings only via `cargo test -p desktop --test bindings --locked -- --ignored export_bindings` if IPC DTOs change.

## WP-A Repository opener (desktop) — small
- `crates/desktop/src/ipc/node_commands.rs::repository_url`: reject an empty raw authority (`authority.is_empty()`), in addition to `@`. Add regression tests for `https:////@github.com/example/node`, `https:///github.com/x/y`, `https://@github.com/x/y`, `https://github.com@evil.com/x` and keep the two existing tests.
- Regenerate bindings (export test), add `nodeOpenRepository(repository: string)` to `ui/src/shared/ipc/gateway.ts` following the existing wrapper style.

## WP-B Findings 1 + 6 (cross-context: environment, custom_nodes, comfy_runtime, desktop)

### Finding 1 — Repair restores node dependencies; failure state preserved
Proposed design:
1. New environment port (application `environment/ports.rs`), e.g.
   ```rust
   pub struct NodeRequirementSet { pub node_id: String, pub source_identity: String, pub file: Option<PathBuf>, pub extra: Vec<String> /* persisted registry deps */ }
   #[async_trait] pub trait NodeRequirementSource: Send + Sync {
       async fn requirement_sets(&self, root: &Path) -> Result<Vec<NodeRequirementSet>, ProvisionError>;
       async fn record_outcome(&self, env: &EnvId, set: &NodeRequirementSet, ok: bool) -> Result<(), ProvisionError>;
       async fn failed_in(&self, env: &EnvId, root: &Path) -> Result<bool, ProvisionError>; // any current node (enabled or disabled) whose current source identity lacks an Ok outcome in env while it has requirements
   }
   ```
   added to `EnvironmentDeps`. Alternative (HANDOFF suggestion): extend `ComfySource` and inject `NodeRepository` into infrastructure `GitSource`. Advisor to pick; main prefers a dedicated port implemented in the **application** custom_nodes context over `NodeRepository` + `NodeFiles` (no new infra coupling, testable with existing fakes), wired in desktop state.rs. Watch for construction-order cycles (NodeDeps contains EnvironmentDeps).
2. Requirement selection per installed node (enabled AND disabled): root `requirements.txt` (as today) + `dependency_requirements` of the latest consented (consent_identity present) Install/Update/RetryDependencies operation whose `node` source identity equals the node's current source identity. Unmanaged/manual nodes: file only. Never re-resolve registry, never run install.py.
3. `Execution::comfy_requirements`: install per set (file and extra lines) under the existing constraint file + pin re-assertion; `record_outcome(new_env, set, ok)` for each. Mode Repair: any node set failure is fatal for the run (replacement NOT activated; previous env stays active and keeps `RepairRequired`). Other modes (change torch / python / custom venv): keep current non-fatal Warning behaviour (existing test `replacement_environments_receive_existing_node_requirements_under_the_pin_guard`), but outcomes are recorded for the new env so node state is per env.
4. `repair_check`: only return "nothing to repair"/clear `RepairRequired` when core verification + pins pass **and** `failed_in(active_env)` is false. Otherwise proceed to a replacement build.
5. In-place node operations (workflow.rs dependencies step) keep writing `save_dependency` for (node, env, source identity) — confirm they write both ok and failed.
6. Node list dependency state becomes environment-scoped: `NodeService::list`/scan (or the desktop presenter) derives each node's `DependencyState` from `node_env_deps` for the **active** env + the node's **current** source identity (missing row → `NotRun` if the node has requirements, `Ok` if it has none? advisor decide), instead of the global `nodes.dependencies` column. Add `NodeRepository::dependency(node, env, source_identity)` (or bulk `dependencies(env)`) and SQLite impl reading `node_env_deps`. Keep the column for backward compatibility or migrate; advisor decide (migration 007 only if needed).
7. Delete or wire the dead `NodeService::repair_requirements()` (it becomes the requirement-set provider or is removed with its test replaced).
Tests: application environment setup tests — repair with a node-dep failure does not activate and keeps RepairRequired; repair succeeds when node deps install → activates, outcomes Ok; repair_check does not clear RepairRequired when node outcome failed in active env; non-repair replacement records failed outcome but activates; registry extra deps are included for enabled and disabled nodes; disabled node included; source identity mismatch (node updated outside app) uses file only. custom_nodes tests: dependency state follows active env. infrastructure sqlite test for the reader.

### Finding 6 — launch checks under the maintenance lease
1. comfy_runtime application gets a port, e.g. `pub trait LaunchGuard: Send + Sync { async fn before_start(&self, lease: &mut dyn MaintenanceLease) -> Result<(), RuntimeError>; async fn after_stop(&self, lease: &mut dyn MaintenanceLease); }` (names advisor's call), implemented in application custom_nodes by an adapter over `NodeService` (`reconcile_held` then refuse with a sanitized `RecoveryRequired`-style RuntimeError if any `blocks_start()` op remains; `after_stop` = reconcile + scan held, errors logged/recorded, never silently claiming success).
2. Start use case: acquire lease (existing try_acquire) → `before_start(lease)` → existing `start_under_lease`.
3. Restart use case: one lease → supervisor.stop() (verified) → `after_stop(lease)` (rescan) → `before_start(lease)` → start under the same lease. No second acquisition.
4. Desktop `comfy_start`/`comfy_restart`: remove the pre-lease `ensure_startable()` calls. `comfy_stop`: after a verified stop, run the node after-stop work with a **waiting** acquire (not try_acquire) so a Busy gate does not skip the FR-NODE-10 rescan; failures are logged (redacted) and the node subscription publishes `changed`; unresolved operations remain in the journal and therefore block the next start under the lease.
5. Map the new runtime error to an IPC code the UI can show (e.g. `node_recovery_required`).
Tests: application comfy_runtime tests with a fake guard — guard runs after the lease is acquired and before process inspection/spawn; a blocking journal refuses start; restart calls stop → after_stop → before_start → start with exactly one acquisition; an op becoming blocking between "IPC call" and lease acquisition is caught (guard evaluated under lease). Desktop wiring compiles; ACL tests unchanged.

## WP-C Findings 2, 3, 4, 5, 7 (infrastructure custom_nodes files.rs/git.rs/registry.rs/tree.rs + application workflow.rs)

### Finding 4 (do first; recovery depends on it) — refresh final digest after hooks
- In `workflow.rs` after dependencies/hook and successful `verify_final`, and before persisting `Verified`: `files.snapshot(root, &op.node, None)` of the live published node; set `op.node.tree_digest` (and node record) to the refreshed digest; persist with `Verified`. If snapshot fails → treat as an unresolved state (keep backup, do not recycle). Only refresh when the hook actually ran (otherwise assert equality with the reviewed digest and block on mismatch?) — advisor decide.
- Test: FakeHook mutates a file → Verified op/node carry the refreshed digest; later recovery uses it.

### Finding 2 — recovery proves identity before moving/recycling
In `files.rs::recover_blocking` (and enable/disable/uninstall branches), compute `digest_of` for every copy before acting:
- Verified/CleanupPending: recycle the backup only if live digest == `op.node.tree_digest` AND backup digest == `op.prior.tree_digest`; otherwise keep every copy and return `Blocked` (phase stays; non-blocking phases do not block start, but the op stays unresolved and visible).
- Enable/disable: `(from, to)` existence plus digest equality with the expected node identity; mismatch/foreign copy → `Blocked`.
- Uninstall: restore or recycle the backup only if its digest == prior digest; else `Blocked`.
- Install/update restore branches: restore the backup only if it matches the prior digest; never overwrite/recycle a foreign destination; when the backup is the only recoverable old copy and is modified, preserve it.
- `ensure_layout().ok()` in recovery must not swallow layout errors → `Blocked`/error.
Tests (infrastructure `custom_nodes/tests.rs`): foreign destination, changed published tree in Verified/CleanupPending, modified backup (update and uninstall), enable/disable foreign copy, each crash boundary; assert copies preserved and action `Blocked`.

### Finding 3 — Git tree revalidation
- Git snapshot (`read_tree(..., git=true)`): in addition to `git:<commit>`, require a clean worktree (porcelain status ignoring app marker files already excluded) and `validate_tree` (no links/reparse) for staged checkouts and on revalidation; mismatch → stale plan / Blocked.
- Git update staging stub: revalidation rejects any staged file other than root `requirements.txt`/`install.py` (and app marker).
- After `apply` (ff-only/detached checkout) in publish: run `validate_tree` + clean-worktree check on the live dir; failure → operation fails without discarding edits; recorded failure; node conflict/dirty shown.
- Preserve dirty/diverged refusal and immutable consent binding (exact commit).
Tests: git fixtures (require_git!) — staged checkout with an untracked/modified file is rejected at commit; post-apply invalid tree is reported; existing git tests keep passing.

### Finding 5 — registry warnings/refusals
- `files.rs prepare_registry`: replace `self.registry.node(id).await.ok()` with propagated errors (refusal/not-found/offline → prepare fails with the mapped NodeAppError).
- Merge node-level status (flagged/banned/deleted/pending) and deprecated flags into the review warnings (dedupe with release-level warnings).
Tests: detail GET 403/404/500 → prepare fails; node-level flagged/deprecated produce warnings requiring acknowledgement.

### Finding 7 — filesystem parent containment
- `ensure_layout`: `symlink_metadata` for `ComfyUI` (reject link/reparse/non-dir), and canonical containment `canonical(custom_nodes).parent() == canonical(root/ComfyUI)` etc.
- Before each mutation (publish/backup/move_enabled/recycle/restore/discard), re-validate the parent chain (root/ComfyUI/custom_nodes[/.disabled]) is non-link and canonically contained. Advisor: decide whether to reuse `shared/model_paths.rs` PinnedDirectory handle-based `rename_no_replace` for directory renames (true TOCTOU protection) or document the residual race in the `NodeFiles` port doc. Main leans: re-validate immediately before each rename + update the port doc to state exactly what is guaranteed, unless PinnedDirectory already supports directory handles cheaply.
Tests: linked `ComfyUI` parent (unix symlink / windows junction where creatable without admin) → refused; `.disabled` link refused.

## Ordering and verification
WP-A → WP-B → WP-C (WP-B and WP-C both touch workflow.rs; run sequentially). After each WP: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, `node scripts/check-arch-deps.mjs`, bindings drift test. (Local Windows build uses the GNU toolchain override `RUSTUP_TOOLCHAIN=1.99.0-x86_64-pc-windows-gnu`, `CARGO_TARGET_DIR=target-gnu`, ucrt64 gcc on PATH, because MSVC C++ tools are not installed on this machine.)

## Questions for advisor (classify AI-decidable vs user-only)
Q1 Port placement for node requirement sets (dedicated port in application vs ComfySource+GitSource with NodeRepository).
Q2 Non-repair re-setup: keep node-dep failures as warnings (record per-env outcomes) or make them fatal?
Q3 Missing node_env_deps row semantics for the list; keep/migrate the global `nodes.dependencies` column.
Q4 Post-hook digest refresh policy when the hook did not run.
Q5 Finding 7 depth: PinnedDirectory handle-based directory renames vs immediate re-validation + documented residual race.
Q6 comfy_stop after-stop: waiting acquire vs try + surfaced status.
