ROLE: coder (write-capable). Mechanically implement the decision below. Do not redesign or expand scope. If it conflicts with the code, stop and report the gap.

CONTEXT: M5 hardening H3 was just implemented in the working copy (uncommitted; keep it): crates/infrastructure/src/shared/trash.rs (IFileOperation + rejecting IFileOperationProgressSink, positive completion, NotFound check), crates/infrastructure/src/shared/trash/tests.rs, desktop close policy, etc. The H3 coder reported an evidence gap: the model library adapter (crates/infrastructure/src/models/library/file_ops.rs ~line 191-200, `trash::recycle(&parent.path.join(name.as_str()))`) passes verbatim Windows paths (`\\?\C:\...`, from canonicalized/pinned roots) and `SHCreateItemFromParsingName` rejects them with E_INVALIDARG before the sink runs. This predates H3 (HEAD a879fc0 had the same call), so on Windows moving a model to the Recycle Bin (FR-MODEL-3) always fails with TrashFailed today.

DECISION (main; precedent: `rename_no_replace` in crates/infrastructure/src/shared/model_paths.rs strips `\\?\` before SetFileInformationByHandle with the comment "hand it a plain drive path"):
1. In the Windows recycle path of trash.rs, convert the item path to a shell-parsable form before `SHCreateItemFromParsingName`: `\\?\<drive>:\rest` -> `<drive>:\rest`; `\\?\UNC\server\share\rest` -> `\\server\share\rest`; any other verbatim form (e.g. `\\?\Volume{...}`, `\\.\`) -> return an error (fail closed; callers already map it to TrashFailed / CleanupPending; never permanent deletion). Non-verbatim paths are unchanged. Keep using the ORIGINAL path for the post-operation NotFound confirmation and for the test-only refusal injection lookup, or normalize consistently so injection still matches — report which.
2. Do not change callers, H3 semantics, or the failure behaviour.
3. Tests (Windows, crates/infrastructure/src/shared/trash/tests.rs and/or models/library/tests.rs):
   - `windows_recycle_accepts_verbatim_drive_paths`: a real file and a real directory addressed by `\\?\C:\...`-style paths (e.g. from `std::fs::canonicalize`) are recycled successfully (positive completion, source NotFound).
   - `windows_recycle_refuses_unsupported_verbatim_forms`: a `\\?\Volume{...}`-style or `\\.\` path returns an error and the item (when one exists) survives.
   - `model_trash_moves_a_canonical_root_file_to_the_recycle_bin`: through the real model library adapter with a canonicalized root, `trash` succeeds, the file is gone (NotFound) and the H3 refusal test for models still passes.
   Native tests recycle only files created in temp dirs by the test.

SUCCESS CRITERIA (report each):
- cargo fmt --all -- --check
- cargo clippy --workspace --all-targets --locked -j 4 -- -D warnings
- cargo test --workspace --locked -j 4 --no-fail-fast (previous run: 578 passed, 0 failed, 9 ignored)
- node scripts/check-arch-deps.mjs

OUTPUT: file:line summary, test names and results, exact command results, gaps.

## Build environment and repository rules (mandatory)
- WORKING COPY: D:\vjump\vjump-comfyui-portable-with-resource-manager (branch feat/m0-skeleton, HEAD a879fc0). Build with the default MSVC toolchain from rust-toolchain.toml into the default target/ dir; do not set RUSTUP_TOOLCHAIN or CARGO_TARGET_DIR. Pass -j 4 to cargo; never run two cargo commands at once.
- Rust: rustfmt, clippy -D warnings, match surrounding style and comment density. Normalize edited files to LF.
- Do not commit, push, stash, reset, checkout, apply or touch git history. Only modify crates/infrastructure/src/shared/trash.rs, crates/infrastructure/src/shared/trash/tests.rs and crates/infrastructure/src/models/library/tests.rs.
