ROLE: coder (write-capable). Mechanically implement the advisor-approved plan below. Do not redesign or expand scope. If the plan is underspecified or conflicts with the code, stop that item and report the gap precisely instead of improvising.

## Build environment and repository rules (mandatory)
- WORKING COPY: `D:\vjump\vjump-comfyui-portable-with-resource-manager` (branch `feat/m0-skeleton`, HEAD `e384d43`). Uncommitted working-copy changes = verified session-6 packages Q, S0, F, G, X, G2b, G2c, D3+D5, D3b (`docs/handoff/patches/s6-*`) + docs: preserve them; never revert files you did not change for this package.
- Read `AGENTS.md` and `docs/handoff/tooling/coder-env.md`. Default MSVC toolchain, default `target/`; do NOT set RUSTUP_TOOLCHAIN or CARGO_TARGET_DIR. `-j 6`; never two cargo commands at once. Vitest with `VITEST_MAX_WORKERS=2`. Regenerate `ui/src/shared/ipc/bindings.ts` only via `cargo test -p desktop --test bindings --locked -- --ignored export_bindings`. Run `cargo fmt --all` after your last Rust edit (an editor hook may reformat). LF line endings. Git tests must actually execute.
- No git write commands on this repository (git in temp fixtures is fine). Do not touch `D:\vjc-s6\` or `D:\comfyui-test\`; do not kill `vjumpkung-comfyui-desktop` processes.

## Owner request (2026-10-20 14:33)
"When updating ComfyUI have an option that git pull (latest commit) or checkout to tag version."
Draft plan: `docs/handoff/plans/s6-u-update-modes.md`. **The authoritative corrected plan is the Codex advisor ruling `docs/handoff/evidence/s6-advisor-u.md` — implement its sections 1–7 exactly ("Corrected implementation plan") and its "Required tests" table (exact test names and assertions).** Decisions: LatestCommit is a new update target; `target_branch` persisted in the update JSON document (no SQL migration); explicit `git fetch --tags origin refs/heads/*:refs/remotes/origin/*` for LatestCommit only; apply uses the PERSISTED branch (never rediscover); Intent + LatestCommit applies even when HEAD already equals the frozen commit; `comfyui_reference` = tag or actual branch (None when detached); **no wizard change** (owner-only, deferred).

## Docs (only these)
`docs/DOCUMENTATION.md` ComfyUI updates section (~:299–303): describe the two update modes — "Update to latest commit (git pull)" (origin's default branch, normally master, development code) and "Update to latest release" / "Switch to <tag>" (tag checkout) — plus the existing Fast-forward of the current branch; correct "Updates use fast-forward only" wording as the advisor states. `docs/REQUIREMENTS.md` FR-COMFY-6: add the explicit development-head option (owner decision 2026-10-20); keep FR-COMFY-7 and §2.4 unchanged.

## Success criteria (run at the end; report exact counts)
```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -j 6 -- -D warnings
cargo test -p domain --locked -j 6 latest_commit
cargo test -p application --locked -j 6 latest_commit
cargo test -p infrastructure --locked -j 6 latest_commit
cargo test -p infrastructure --locked -j 6 update_documents_preserve_latest_commit_and_read_legacy_v1
cargo test -p desktop --locked -j 6 --lib latest_commit_update_target_roundtrips
cargo test -p desktop --test bindings --locked
cargo test --workspace --locked -j 6 --no-fail-fast
pnpm -C ui lint
pnpm -C ui typecheck
VITEST_MAX_WORKERS=2 pnpm -C ui test
pnpm -C ui build
node scripts/check-arch-deps.mjs
```
OUTPUT: per-file summary with file:line, key diffs, every command result with counts, deviations with reasons. Also write it to `docs/handoff/evidence/s6-u-implementation.md`.
