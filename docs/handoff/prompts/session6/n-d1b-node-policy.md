ROLE: coder (write-capable). Mechanically implement the advisor-approved plan below. Do not redesign or expand scope. If the plan is underspecified or conflicts with the code, stop that item and report the gap precisely instead of improvising.

## Build environment and repository rules (mandatory)
- WORKING COPY: `D:\vjump\vjump-comfyui-portable-with-resource-manager` (branch `feat/m0-skeleton`, HEAD `e384d43`). Uncommitted working-copy changes = verified session-6 packages Q, S0, F, G, X, G2b, G2c, D3+D5, D3b, U, W, W2, D4 and D1 (partial: everything except item 3) — checkpoint `docs/handoff/patches/s6-12-through-d1-partial-verified.patch`: preserve them; never revert files you did not change for this package.
- Read `AGENTS.md` and `docs/handoff/tooling/coder-env.md`. Default MSVC toolchain, default `target/`; do NOT set RUSTUP_TOOLCHAIN or CARGO_TARGET_DIR. `-j 6`; never two cargo commands at once. Vitest with `VITEST_MAX_WORKERS=2`. Regenerate bindings only via the export test if any DTO changes (none expected). Run `cargo fmt --all` after your last Rust edit. LF line endings. Git tests must actually execute.
- No git write commands on this repository (git in temp fixtures is fine). Do not touch `D:\vjc-s6\` or `D:\comfyui-test\`; do not kill `vjumpkung-comfyui-desktop` processes.

## Owner decisions (product authority, 2026-10-20) — not to be re-litigated
1. First-run setup with the review-step confirmation installs EVERY selected curated node, even the ones the app refuses for content reasons ("allow all even the security problem one"): shipped `__pycache__` (ComfyUI-PlagueKind-Nodes becomes selectable) and D1's Windows top-level native images. Containment refusals stay everywhere.
2. Install order everywhere (setup and Custom Nodes page installs/updates/dependency retries with an approved install.py): **install.py first, then requirements.txt**. The Custom Nodes page keeps its per-plan review/consent and every content-policy refusal ("order only").

## Authoritative plan
Implement EXACTLY the Codex advisor ruling `docs/handoff/evidence/s6-advisor-n.md` — sections "One corrected implementation plan" 1–5 (policy threading `NodePreparePolicy`, raw-output contract `run_raw_stdout` / `GitNodes::read_raw` and the strict Windows pre-checkout `ls-tree` admission check that completes D1 item 3, the two content exceptions with the exact warning texts, hook-first execution order with pin restoration + profile-constraint restore + post-hook validation of the frozen reviewed InstallSpec, warnings/UI/docs copy) and every row of "Required tests and assertions" (exact test names). Draft plan for context: `docs/handoff/plans/s6-n-setup-node-policy.md`; D1 context: `docs/handoff/prompts/session6/d1-r6-closure.md`, `docs/handoff/evidence/s6-d1-implementation.md` (append the D1 completion evidence there; do not rewrite its historical partial results). Residual row 6 in `docs/handoff/evidence/s3-residuals.md` may be set to "Closed before consent (D1); residual after consent and for installed/user-placed nodes" ONLY if the two Windows admission/cleanup tests pass; do not close row 28.

## Success criteria (run at the end; report exact counts)
```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -j 6 -- -D warnings
cargo test -p infrastructure --locked -j 6 raw_stdout
cargo test -p infrastructure --locked -j 6 root_native
cargo test -p infrastructure --locked -j 6 git_root_tree_admission
cargo test -p infrastructure --locked -j 6 consented_setup_git_prepare
cargo test -p application --locked -j 6 install_py
cargo test -p application --locked -j 6 curated
cargo test --workspace --locked -j 6 --no-fail-fast
pnpm -C ui lint
pnpm -C ui typecheck
VITEST_MAX_WORKERS=2 pnpm -C ui test
node scripts/check-arch-deps.mjs
```
Also: with `D:\vjc-s6\r1\tools\uv` first on PATH (read-only use of that folder's uv.exe is allowed) run `cargo test -p infrastructure --locked -j 6 node_preview_never_imports_python_modules_from_node_directory -- --nocapture` and report the printed uv version and whether it ran.
OUTPUT: per-file summary with file:line, key diffs, every command result with counts, deviations with reasons. Also write it to `docs/handoff/evidence/s6-n-implementation.md`.
