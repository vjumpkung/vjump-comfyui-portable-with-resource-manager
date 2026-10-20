ROLE: coder (write-capable). Mechanically implement the advisor-approved plan below. Do not redesign or expand scope. If the plan is underspecified or conflicts with the code, stop that item and report the gap precisely instead of improvising.

## Build environment and repository rules (mandatory)
- WORKING COPY: `D:\vjump\vjump-comfyui-portable-with-resource-manager` (branch `feat/m0-skeleton`, HEAD `a4175d1`). Uncommitted working-copy changes are verified packages P0 (startup fix) and I (UI integration), checkpoint `docs/handoff/patches/s5-02-through-i-verified.patch`: preserve them; never revert files you did not change for this package.
- Read `AGENTS.md` and `docs/handoff/tooling/coder-env.md` first. Default MSVC toolchain from `rust-toolchain.toml`, default `target/`; do NOT set RUSTUP_TOOLCHAIN or CARGO_TARGET_DIR.
- Memory: 32 GB machine, ~10 GB free. Use `-j 6` for cargo; targeted `-p <crate>` runs with test filters while iterating; workspace-wide commands once at the end. Never run two cargo commands at once. Vitest: set `VITEST_MAX_WORKERS=2`.
- Never hand-edit `ui/src/shared/ipc/bindings.ts`; regenerate only with `cargo test -p desktop --test bindings --locked -j 6 -- --ignored export_bindings`.
- Rust: rustfmt, clippy `-D warnings`, no serde in domain/application, match surrounding style and comment density. TypeScript: 2-space, single quotes, no semicolons; Tauri access only via `ui/src/shared/ipc/gateway.ts`; features never import other features. Normalize edited files to LF.
- Do not commit, push, stash, reset, checkout, apply, clean or touch git history. Do not modify files outside the stated scope.
- A native acceptance app may be running from `D:\vjc-s5\bin\` — do not kill any `vjumpkung-comfyui-desktop` process and do not touch `D:\vjc-s5\`.

## Problem (native finding, verified by main 2026-10-20)
Default first-run setup (all 22 curated nodes selected) always fails in `curated_nodes` with "unsafe node path, archive or source" at catalog entry `ComfyUI-PlagueKind-Nodes` @ `d58d006a4ea32c25c06499f2ff104f0852a045a6`: the repository tracks `bundle_nodes/__pycache__/bundle_nodes.cpython-314.pyc`, which `validate_fresh_git` correctly refuses (the content digest excludes `__pycache__`, so shipped bytecode would be outside the reviewed identity). d58d006 is upstream HEAD; no clean commit exists. All other 21 pinned commits pass. Retry re-fails deterministically; Skip skips every not-installed curated node. With that node deselected, setup + batch review of 21 + install + final verify + ComfyUI start all succeed natively.

## Approved plan (advisor stand-in ruling D2: GO with O2-narrowed; O1 rejected, O3 deferred)
Keep the entry visible but mark it UNAVAILABLE and unselectable (reversible data change; catalog order and pin kept).
1. `crates/infrastructure/src/environment/data/curated_nodes.json` — PlagueKind entry: add `"unavailable": "Not available at this pinned commit: the repository includes compiled Python cache files (__pycache__) that are not part of the reviewed source, so the app cannot install it safely."`
2. `crates/infrastructure/src/environment/curated.rs` — `EntryDoc`: `#[serde(default)] unavailable: Option<String>`; validate the reason: trimmed, non-empty, ≤ 300 bytes, no control characters; otherwise a new `CuratedError::Unavailable` ("a curated node has an invalid unavailability reason"). Map it into the spec.
3. application `crates/application/src/environment/model.rs` (~473): `CuratedNodeSpec.unavailable: Option<String>`; update every constructor/fake (tests/fakes.rs etc.).
4. application `crates/application/src/environment/setup.rs` (~302-310, `curated_progress` / selection validation): selecting an unavailable id is refused with the existing `CuratedSelectionInvalid` (`curated_selection_invalid`).
5. `crates/desktop/src/ipc/env_dto.rs` (~341-360): `CuratedNodeDto.unavailable: Option<String>` (generated as `unavailable: string | null`); regenerate bindings.
6. `ui/src/features/setup/setup-wizard.tsx` — (~164-165) the initial default selection excludes unavailable entries; (~798-815) an unavailable entry's checkbox is disabled, unticked, and shows "Unavailable: {reason}"; unavailable ids are never sent. Update `ui/src/shared/testing/environment.ts` fixtures and any test mocks requiring the new field.
7. Tests (exact names):
   - Rust: `bundled_catalog_marks_only_plaguekind_unavailable` (infrastructure curated tests), `catalog_rejects_blank_or_control_unavailable_reason`, `first_run_rejects_unavailable_curated_selection` (application setup tests); keep all existing tests passing (update fixtures for the new field).
   - Vitest (setup-wizard.test.tsx): `unavailable_curated_nodes_start_unticked_disabled_show_reason_and_are_not_sent`.
8. Docs (only these lines): `docs/DOCUMENTATION.md` ~151 (curated catalog text) → "21 of the 22 are ticked by default; ComfyUI-PlagueKind-Nodes is listed as unavailable at its pinned commit (it ships `__pycache__`)." and `docs/handoff/evidence/s3-residuals.md` row 16 → "Skip optional step skips every curated node not yet installed; to keep the others, abort and start setup without the failing node." Do not edit other docs.
Not in scope: auto-skipping entries of runs saved before this change; O1 sparse checkout; O3 per-entry skip.

## Success criteria (run at the end, report each with exact counts)
```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -j 6 -- -D warnings
cargo test --workspace --locked -j 6 --no-fail-fast        (baseline 715 passed / 0 failed / 10 ignored; expect +3)
cargo test -p desktop --test bindings --locked -j 6         (after regeneration)
cargo test -p desktop --test acl --locked -j 6
pnpm -C ui lint ; pnpm -C ui typecheck ; pnpm -C ui test    (baseline 29 files / 183 tests; expect +1)
pnpm -C ui build
node scripts/check-arch-deps.mjs
```
OUTPUT: per-file summary with file:line, key diffs, every command result with counts, deviations with reasons. Write the report also to `docs/handoff/evidence/s5-d2-implementation.md`.
