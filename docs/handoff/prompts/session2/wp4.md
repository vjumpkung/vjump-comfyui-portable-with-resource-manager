ROLE: coder (write-capable). Mechanically implement the approved plan below. Do not redesign or expand scope. If underspecified or conflicting with the code, stop that item and report the gap.

WORKING COPY: C:\Users\vjump\Documents\vjump-comfyui-portable-with-resource-manager (main repo). It contains uncommitted, verified work from earlier packages (M5 WP6, WP1, WP2+WP3, possibly others) - preserve it; never revert files you did not change for this package. Never touch the worktrees under C:\Users\vjump\vjc-wt\.

GOAL: M5 backend WP4 - persist final contents and make recovery identity-based.

APPROVED SPEC (authoritative): docs/handoff/plans/m5-backend-approved-plan.md section "WP4 - Persist final contents and make recovery identity-based", implemented exactly:
- Successful dependency completion: snapshot the published node again before Verified; no hook ran -> require equality with the reviewed content digest (unexpected mutation = failure); consented hook ran -> accept a fresh link-free final digest while keeping the reviewed source revision (hooks never authorize a different source); persist the final digest in op.node and the node record, keep original requirement/hook digests; do final filesystem validation BEFORE clearing environment repair state and recording dependency success; on final validation failure retain the backup and a blocking journal phase and preserve/restore RepairRequired. Adjust FakeFiles::publish to model staged contents becoming live; do not weaken equality checks.
- Blocking state without a migration: extend the existing single policy point `impl NodeOperation { pub fn blocks_start(&self) -> bool }` (crates/application/src/custom_nodes/model.rs) to `self.phase.blocks_start() || (!matches!(self.phase, OperationPhase::Completed | OperationPhase::Failed) && self.failure.is_some())`; settle(): RecoveryAction::Blocked records a fixed sanitized recovery failure and preserves the phase; successful reconciliation clears the previous blocking failure before applying its resulting state; ensure_reconciled() uses NodeOperation::blocks_start(). Ordinary recycle failure stays CleanupPending and non-blocking. Correct the NodeListDto.unresolved doc (not every retained cleanup blocks launch).
- Recovery rules table (validate layout before every branch; remove ensure_layout().ok()): prepared/unpublished, enable/disable, uninstall before verification, verified uninstall, install/update before verification, verified install/update/retry, Git update (source + commit + content) - exactly as in the spec table. Interrupted DependenciesInProgress: establish published source/location safely, mark environment and outcome failed, retain backup; missing/foreign/unsafe publication stays unresolved. Legacy `git:<commit>` journal entries do not prove content identity: preserve ambiguous legacy copies.

EVIDENCE: docs/handoff/evidence/exp-b.md sections "Port/journal types", "Finding 2", "Finding 4".

TESTS (exact names). Application: consented_hook_final_digest_is_saved_before_verified, unexpected_tree_mutation_without_a_hook_blocks_completion, final_snapshot_failure_preserves_repair_and_backup, blocked_verified_cleanup_blocks_start_but_trash_failure_does_not, interrupted_dependency_mutation_preserves_failure_state. Infrastructure: recovery_preserves_foreign_destinations, verified_recovery_preserves_changed_published_trees, recovery_never_restores_or_recycles_a_modified_backup, toggle_recovery_requires_matching_content; extend install_recovery_at_every_boundary, update_recovery_preserves_the_only_recoverable_copy, uninstall_and_toggle_recovery. Assert both the returned action and the contents/existence of every surviving copy.

SUCCESS CRITERIA (run at the end):
- cargo fmt --all -- --check
- cargo clippy --workspace --all-targets --locked -j 3 -- -D warnings
- cargo test --workspace --locked -j 3 --no-fail-fast
- If an IPC DTO changed: regenerate bindings via the export test, then `pnpm -C ui typecheck` and `pnpm -C ui test`.
- node scripts/check-arch-deps.mjs

OUTPUT (final message): per-file summary (file:line), new/changed signatures, tests added/changed (names), exact command results, spec gaps.
