ROLE: coder (write-capable). Mechanically implement the approved plan below. Do not redesign or expand scope. If underspecified or conflicting with the code, stop that item and report the gap.


GOAL: M5 backend WP4 - persist final contents and make recovery identity-based.

APPROVED SPEC (authoritative): docs/handoff/plans/m5-backend-approved-plan.md section "WP4 - Persist final contents and make recovery identity-based", implemented exactly:
- Successful dependency completion: snapshot the published node again before Verified; no hook ran -> require equality with the reviewed content digest (unexpected mutation = failure); consented hook ran -> accept a fresh link-free final digest while keeping the reviewed source revision (hooks never authorize a different source); persist the final digest in op.node and the node record, keep original requirement/hook digests; do final filesystem validation BEFORE clearing environment repair state and recording dependency success; on final validation failure retain the backup and a blocking journal phase and preserve/restore RepairRequired. Adjust FakeFiles::publish to model staged contents becoming live; do not weaken equality checks.
- Blocking state without a migration: extend the existing single policy point `impl NodeOperation { pub fn blocks_start(&self) -> bool }` (crates/application/src/custom_nodes/model.rs) to `self.phase.blocks_start() || (!matches!(self.phase, OperationPhase::Completed | OperationPhase::Failed) && self.failure.is_some())`; settle(): RecoveryAction::Blocked records a fixed sanitized recovery failure and preserves the phase; successful reconciliation clears the previous blocking failure before applying its resulting state; ensure_reconciled() uses NodeOperation::blocks_start(). Ordinary recycle failure stays CleanupPending and non-blocking. Correct the NodeListDto.unresolved doc (not every retained cleanup blocks launch).
- Recovery rules table (validate layout before every branch; remove ensure_layout().ok()): prepared/unpublished, enable/disable, uninstall before verification, verified uninstall, install/update before verification, verified install/update/retry, Git update (source + commit + content) - exactly as in the spec table. Interrupted DependenciesInProgress: establish published source/location safely, mark environment and outcome failed, retain backup; missing/foreign/unsafe publication stays unresolved. Legacy `git:<commit>` journal entries do not prove content identity: preserve ambiguous legacy copies.

EVIDENCE: docs/handoff/evidence/exp-b.md sections "Port/journal types", "Finding 2", "Finding 4".

TESTS (exact names). Application: consented_hook_final_digest_is_saved_before_verified, unexpected_tree_mutation_without_a_hook_blocks_completion, final_snapshot_failure_preserves_repair_and_backup, blocked_verified_cleanup_blocks_start_but_trash_failure_does_not, interrupted_dependency_mutation_preserves_failure_state. Infrastructure: recovery_preserves_foreign_destinations, verified_recovery_preserves_changed_published_trees, recovery_never_restores_or_recycles_a_modified_backup, toggle_recovery_requires_matching_content; extend install_recovery_at_every_boundary, update_recovery_preserves_the_only_recoverable_copy, uninstall_and_toggle_recovery. Assert both the returned action and the contents/existence of every surviving copy.

SUCCESS CRITERIA (run at the end):
- cargo fmt --all -- --check
- cargo clippy --workspace --all-targets --locked -j 4 -- -D warnings
- cargo test --workspace --locked -j 4 --no-fail-fast
- If an IPC DTO changed: regenerate bindings via the export test, then `pnpm -C ui typecheck` and `pnpm -C ui test`.
- node scripts/check-arch-deps.mjs

OUTPUT (final message): per-file summary (file:line), new/changed signatures, tests added/changed (names), exact command results, spec gaps.

AMENDMENT (advisor, docs/handoff/plans/m5-h-hardening-approved-plan.md §7): hardening packages H1 (bounded pinned traversal: `read_tree/validate_tree/tree_digest` take `&Arc<PinnedDirectory>`, `create_new_child`, depth/path limits) and H2 (retention contract: `Retained`, `MaintenanceLease::retained`, `StepContext.retained`, Git config preflight, fixed Git error messages) and H3 (Windows recycle sink, close policy) are already in the working copy. Keep the approved WP4 recovery table and all named tests unchanged in intent; implement recovery/final-snapshot logic on top of H1's bounded pinned snapshots and H2's retention contract (recovery/publish Git calls carry retention). Preserve the distinction between ambiguous recovery (blocks start) and ordinary recycle failure (CleanupPending, non-blocking) — note H3 makes a refused recycle an ordinary failure.


## Build environment and repository rules (mandatory)
- WORKING COPY: `D:\vjump\vjump-comfyui-portable-with-resource-manager` (the main repo, branch `feat/m0-skeleton`). The repo moved from `C:\Users\vjump\Documents\...`; any older path in plans/prompts/evidence means this directory. Worktrees under `C:\Users\vjump\vjc-wt\` no longer exist.
- All earlier verified work (M5 C0+FE, WP6, WP1, B) is committed in HEAD. Uncommitted changes in the working copy belong to the current package or to an earlier package of this session: preserve them; never revert files you did not change for this package.
- Read `docs/handoff/tooling/coder-env.md` and `AGENTS.md` first. Build with the default MSVC toolchain pinned by `rust-toolchain.toml` into the default `target/` dir; do NOT set RUSTUP_TOOLCHAIN or CARGO_TARGET_DIR. Use Git Bash or PowerShell. GNU env (`docs/handoff/tooling/gnu-env.sh`) is a fallback only if MSVC linking fails for toolchain reasons (report it).
- Memory: 32 GB machine, roughly 8-10 GB free. Pass `-j 4` to cargo build/test/clippy, prefer targeted `-p <crate>` runs (and test-name filters) while iterating, run the workspace-wide commands once at the end. Never run two cargo commands at once yourself. Vitest: `VITEST_MAX_WORKERS=2`.
- Rust: rustfmt, clippy `-D warnings`, no serde/tokio/reqwest/rusqlite/tauri in domain or application, ports via async-trait `Arc<dyn ...>`, composition only in crates/desktop/src/state.rs, errors per layer with thiserror. Match surrounding style and comment density.
- TypeScript: 2-space, single quotes, no semicolons, kebab-case files, Tauri access only via ui/src/shared/ipc/gateway.ts. Never hand-edit ui/src/shared/ipc/bindings.ts; regenerate only with `cargo test -p desktop --test bindings --locked -j 4 -- --ignored export_bindings`.
- Generated permission TOMLs under crates/desktop/permissions/autogenerated/ are produced by the build; keep new ones, ignore line-ending-only noise.
- Do not commit, push, stash, reset, checkout, or touch git history. Do not modify files outside the stated scope. If the spec is underspecified or conflicts with the code, stop that item and report the gap precisely instead of improvising.
- Git tests must actually execute (git is on PATH); a skipped `require_git!` is not evidence.

## Session 4 baseline note (main, 2026-10-19)
- HEAD a879fc0 contains WP2+WP3, P, H1, H2. Uncommitted in the working copy and verified by main (keep them; build on them): the first-run scaffold publication fix (crates/infrastructure/src/environment/source{.rs,/publish.rs,/tests.rs}, custom_nodes/tests.rs, ui log-view) and every earlier package of this session listed in the AMENDMENT above. Checkpoints live in docs/handoff/patches/s4-*.patch (do not edit them).
- Codex runs with danger-full-access; still do not run any git write command (git inside test temp dirs is fine).
- Normalize every file you edit to LF line endings.
- Baseline before WP4 (main-verified): 582 Rust tests passed / 9 ignored; H3 (Windows recycle sink, close policy) and the verbatim recycle-path fix are in the working copy (checkpoint docs/handoff/patches/s4-02-through-h3-verified.patch).
