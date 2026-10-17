ROLE: coder (write-capable). Mechanically implement the approved plan below. Do not redesign or expand scope. If underspecified or conflicting with the code, stop that item and report the gap.

WORKING COPY: C:\Users\vjump\Documents\vjump-comfyui-portable-with-resource-manager (main repo). It contains uncommitted work - preserve it, never revert files you did not change for this package: M5 WP6 (verified), M5 WP1 pinned containment (verified except the two gaps below), and M6 package B (bandwidth limiter + HF relay; acquisition files, Cargo.toml/lock, desktop state.rs/commands.rs/download_dto.rs, settings-form) which was just patch-merged and verified by main with clippy + infrastructure/desktop tests. If a workspace failure appears in B's acquisition code, report it rather than rewriting B. Never touch the worktrees under C:\Users\vjump\vjc-wt\.

GOAL: complete M5 backend WP1 by closing its two scope gaps (registry archive download, and containment through dependency/hook subprocesses).

APPROVED SPEC (authoritative, implement exactly): docs/handoff/plans/m5-wp1-gap-decision.md (advisor decision accepted by main) - its Q1 and Q2 sections, the "Corrected file scope and verification plan" table (all listed files are in scope, including environment/packages.rs + tests and shared/process.rs + environment/exec.rs for resource ownership through CleanupFailed), the ten named tests, and the platform requirements (Windows read pins that block replacement while allowing uv/Python readers; Linux descriptor-anchored inputs via the existing anchored_path() mechanism; consume the opened requirements/script files or protected snapshots rather than re-joining paths; preserve relative -r/-c/local-path requirement semantics and hook cwd/imports/__file__ behaviour). Background: docs/handoff/plans/m5-backend-approved-plan.md section "WP1". Use the proposed NodeFiles doc text from the decision (adjusted to what you actually implement).
Do NOT edit docs/handoff/plans/*, HANDOFF.md or prompts - main records documentation. Do not start WP2/WP3.

TESTS (exact names): registry_download_writes_the_opened_staging_file, registry_archive_replacement_is_not_extracted, registry_create_new_refusal_preserves_existing_entries, registry_download_cancellation_cleans_only_owned_entries, node_execution_guard_spans_preview_install_hook_and_cleanup, node_subprocess_inputs_resist_parent_and_leaf_substitution, node_execution_refuses_linked_requirements_and_hooks, guarded_requirements_preserve_relative_includes_and_local_paths, guarded_hook_preserves_cwd_imports_and_script_filename, cleanup_failure_retains_execution_resources_until_children_exit. Use deterministic substitution boundaries and external sentinels; cfg-gate platform-specific tests; report exactly which ran on Windows and which are Linux-only. Tests that need a real Python/uv may use the pinned uv only if already available on this machine without network; otherwise use a fake runner that records the exact inputs/cwd and state why.

SUCCESS CRITERIA (run at the end):
- cargo fmt --all -- --check
- cargo clippy --workspace --all-targets --locked -j 3 -- -D warnings
- cargo test --workspace --locked -j 3 --no-fail-fast
- node scripts/check-arch-deps.mjs

OUTPUT (final message): per-file summary (file:line), new/changed signatures (NodeLocationGuard, pin_location, hook run, downloader), tests added (names) and which ran on Windows, exact command results, residual risks/limitations precisely stated.
