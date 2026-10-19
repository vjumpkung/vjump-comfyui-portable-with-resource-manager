ROLE: coder (write-capable). Mechanically implement the decision below. Do not redesign or expand scope. If it conflicts with the code, stop and report the gap.

CONTEXT: A previous coder run implemented the first-run scaffold publication fix in crates/infrastructure/src/environment/source.rs and the new crates/infrastructure/src/environment/source/publish.rs (+ source/tests.rs, custom_nodes/tests.rs, ui log-view). Those changes are uncommitted in the working copy; keep them. It reported one gap: when the step is cancelled mid-merge, `GitSource::verify_rollback` (publish.rs ~line 490) runs `git status` and `head_commit` with the already-cancelled `ctx.cancel`, so verification fails and the result is the INTERRUPTED message with stage + record retained (safe, recovered on the next retry), instead of a verified in-process rollback followed by `ProvisionError::Cancelled`.

DECISION (main; precedent: `inspect_checkout` in source.rs already runs Git with a fresh `application::shared::CancelToken::new()`, and the approved M6 E1 plan reasserts pins after cancellation with a fresh token on the same lease):
1. In `verify_rollback`, run BOTH verification Git commands (`status --porcelain=v1 --ignored --untracked-files=all` and the HEAD read) with a fresh `CancelToken::new()` and a bounded `RunLimits { timeout: Some(Duration::from_secs(120)), ..RunLimits::default() }`, keeping `.retain(ctx.retained.clone())` and the hardened `git_command(layout)` builder. Use a local helper or a small variant of `head_commit` that takes the token/limits; do not change `head_commit`'s behaviour for other callers. The rollback renames themselves are unchanged.
2. `publish_scaffold` then returns the ORIGINAL error after a verified rollback, so a cancellation mid-merge returns `ProvisionError::Cancelled` with the scaffold restored and stage + record removed. Any verification failure still retains stage + record and returns the INTERRUPTED message (unchanged).
3. Test: add case (d) to `interrupted_scaffold_publication_is_recovered_on_retry` (or a separate test named `cancelled_scaffold_publication_rolls_back_and_reports_cancelled`): a cfg(test) fault (e.g. `Fault::CancelAfter(n)`) cancels `ctx.cancel` after >= 1 move; assert the result is `ProvisionError::Cancelled`, the scaffold directories keep their identities and `.vjump-no-hooks` stays empty, the destination has no `.git` and no checkout files, stage + record are gone, and a subsequent clone with a fresh context publishes successfully. Git must really run.

SUCCESS CRITERIA (report each):
- cargo fmt --all -- --check
- cargo clippy --workspace --all-targets --locked -j 4 -- -D warnings
- cargo test --workspace --locked -j 4 --no-fail-fast (previous run: 566 passed, 0 failed, 9 ignored)
- node scripts/check-arch-deps.mjs

OUTPUT: file:line summary, test names and results, exact command results, gaps.

## Build environment and repository rules (mandatory)
- WORKING COPY: D:\vjump\vjump-comfyui-portable-with-resource-manager (branch feat/m0-skeleton, HEAD a879fc0). Build with the default MSVC toolchain from rust-toolchain.toml into the default target/ dir; do not set RUSTUP_TOOLCHAIN or CARGO_TARGET_DIR. Pass -j 4 to cargo; never run two cargo commands at once.
- Rust: rustfmt, clippy -D warnings, match surrounding style and comment density. Normalize edited files to LF.
- Do not commit, push, stash, reset, checkout, apply or touch git history (git inside test temp dirs is fine). Do not modify files outside crates/infrastructure/src/environment/source{.rs,/publish.rs,/tests.rs}.
