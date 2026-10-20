ROLE: coder (write-capable). Mechanically implement the approved plan below. Do not redesign or expand scope. If the plan is underspecified or conflicts with the code, stop and report the gap precisely instead of improvising.

## Build environment and repository rules (mandatory)
- WORKING COPY: `D:\vjump\vjump-comfyui-portable-with-resource-manager` (branch `feat/m0-skeleton`, HEAD `e384d43`). Uncommitted working-copy changes = verified session-6 packages Q, S0, F, G, X, G2b (`docs/handoff/patches/s6-*`) + docs: preserve them; never revert files you did not change for this package.
- Read `AGENTS.md` and `docs/handoff/tooling/coder-env.md`. Default MSVC toolchain, default `target/`; do NOT set RUSTUP_TOOLCHAIN or CARGO_TARGET_DIR. `-j 6`; never two cargo commands at once. Run `cargo fmt --all` after your last Rust edit (an editor hook may reformat). LF line endings.
- No git write commands on this repository. Do not touch `D:\vjc-s6\` or `D:\comfyui-test\`; do not kill `vjumpkung-comfyui-desktop` processes.

## Native finding (main, 2026-10-20 14:16, binary s6-06 on `D:\vjc-s6\r1`)
The install is detached at v0.42.1 (cloned before G2b). Version card → "Switch to v0.42.1" → "ComfyUI was updated and its environment verified." but `git status -sb` still shows `## HEAD (no branch)`. Cause: `crates/application/src/comfy_runtime/update_workflow.rs` (~:143) calls `deps.repository.apply(..)` only when `current.commit != run.target.commit`, so the G2b placement never runs for a switch to the commit HEAD is already on. Owners with existing detached installs (e.g. `D:\comfyui-test\v1`, detached at v0.42.1) therefore cannot make `git pull` work through the app.

## Plan (main; trivial, AI-decidable, consistent with the G2b ruling)
1. In `update_workflow.rs` `finish`: also call `deps.repository.apply(&root, &run.target, ctx).await?` when `current.commit == run.target.commit` AND `current.branch.is_none()` (detached) AND `!run.target.fast_forward` AND `run.phase == UpdatePhase::Intent`. Keep the existing branch for `current.commit != run.target.commit` unchanged (including its RecoveryRequired guard). Comment: "A detached checkout already at the target is re-placed so it can follow its branch (git pull)." The infrastructure apply already handles a same-commit target: `checkout -B <branch> <commit>` + upstream, or `checkout --detach <commit>` (no-op) with the logged reason. The subsequent HEAD == target verification is unchanged.
2. Tests (application, `crates/application/src/comfy_runtime/tests.rs`, using the existing fakes; record apply calls if the fake does not yet): `switching_to_the_current_tag_re_places_a_detached_checkout` (fake revision detached at the target commit, Tag target → apply called once, update completes); `switching_to_the_current_tag_on_a_branch_does_not_apply` (revision on a branch at the target commit → apply not called). Optionally extend `crates/infrastructure/src/comfy_runtime/git/placement_tests.rs` with `switching_to_the_current_tag_attaches_a_detached_checkout` (real git: detached at the release-line tag commit → after the app's apply path, HEAD on `release/v1.0` tracking `origin/release/v1.0`) if the fixture makes it straightforward.
3. Docs: `docs/DOCUMENTATION.md` ComfyUI updates paragraph — append one sentence: "An install that is still detached (created by an older version of the app) is attached by switching to its current version in the Version card."

## Success criteria (report exact counts)
```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -j 6 -- -D warnings
cargo test -p application --locked -j 6 switching_to_the_current_tag
cargo test --workspace --locked -j 6 --no-fail-fast
node scripts/check-arch-deps.mjs
```
OUTPUT: per-file summary with file:line, key diffs, command results with counts, deviations. Also write it to `docs/handoff/evidence/s6-g2c-implementation.md`.
