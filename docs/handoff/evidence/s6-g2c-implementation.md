# S6 package G2c: attach a detached checkout when switching to the current version

Branch `feat/m0-skeleton`, HEAD `e384d43`, working copy only (no git writes). Prompt: `docs/handoff/prompts/session6/g2c-same-commit-attach.md`.

## Cause
`UpdateComfy::finish` called `deps.repository.apply(..)` only when `current.commit != run.target.commit`. Switching a detached install to the commit HEAD already points at skipped the G2b placement, so the install stayed `HEAD (no branch)` and `git pull` never worked.

## Per-file summary

### `crates/application/src/comfy_runtime/update_workflow.rs:143-157`
Added an `else if` to the existing `if current.commit != run.target.commit { ... }` block. The original branch (including its `RecoveryRequired` guard for `run.phase != Intent`) is unchanged.

```rust
        } else if current.branch.is_none()
            && !run.target.fast_forward
            && run.phase == UpdatePhase::Intent
        {
            // A detached checkout already at the target is re-placed so it can follow its
            // branch (git pull).
            deps.repository.apply(&root, &run.target, ctx).await?;
        }
```
The infrastructure `apply` (`crates/infrastructure/src/comfy_runtime/git.rs:286`) already handles a same-commit target through `check_out` (G2b placement: `checkout -B <branch> <commit>` plus upstream, or `checkout --detach <commit>` with the logged reason). The following `HEAD == target` verification is unchanged. No infrastructure code changed.

### `crates/application/src/comfy_runtime/tests.rs:627` and `:641`
No fake changes were needed. The existing `Machine` fake's `freeze` returns commit `b`*40 / tag `v2` and `apply` records `runtime_apply`, so each test presets `m.revision` to commit `b`*40 and runs `Machine::request()` (Tag `v2`).
- `switching_to_the_current_tag_re_places_a_detached_checkout`: revision `{ commit: b*40, tag: v2, branch: None }`. Asserts phase `Complete`, `count("runtime_apply") == 1`, commit still `b`*40.
- `switching_to_the_current_tag_on_a_branch_does_not_apply`: same, with `branch: Some("release/v0.2")`. Asserts phase `Complete`, `count("runtime_apply") == 0`.

### `crates/infrastructure/src/comfy_runtime/git/placement_tests.rs:281` (optional test, fixture was straightforward)
`switching_to_the_current_tag_attaches_a_detached_checkout` uses real git, in two parts.
- Install detached at `v1.0.1` (the `release/v1.0`-only patch tag), then `install.switch("v1.0.1", &patch)` (the repository `apply` path). HEAD becomes `release/v1.0` at the patch commit, and `release/v1.0@{upstream}` is `origin/release/v1.0`.
- Install detached at `v1.1.0`, then switch to `v1.1.0`. HEAD becomes `main` at the tip and `git pull --ff-only` succeeds.

### `docs/DOCUMENTATION.md:299`
Inserted into the ComfyUI updates paragraph, after the version-switch sentence: "An install that is still detached (created by an older version of the app) is attached by switching to its current version in the Version card."

## Command results (repo root, MSVC default toolchain, `-j 6`, run sequentially)
```
cargo fmt --all -- --check                          -> exit 0 (cargo fmt --all run after the last Rust edit; it reflowed the two new application tests)
cargo clippy --workspace --all-targets --locked -j 6 -- -D warnings
                                                    -> exit 0, no warnings
cargo test -p application --locked -j 6 switching_to_the_current_tag
                                                    -> 2 passed; 0 failed; 206 filtered out
cargo test --workspace --locked -j 6 --no-fail-fast -> exit 0
node scripts/check-arch-deps.mjs                    -> "Architecture dependencies: OK" (exit 0)
```
Workspace test totals: application 208 passed; desktop lib 54 passed; desktop `acl` 11 passed; `bindings` 1 passed / 1 ignored; `navigation` 2 passed; `stack_reserve` 2 passed; `startup_smoke` 0 passed / 1 ignored; domain 47 passed; infrastructure 427 passed / 7 ignored (includes the new `placement_tests` test); infrastructure `real_setup` 0 passed / 1 ignored; infrastructure `workflow` 9 passed; bin and doc-tests 0 tests. Total 761 passed, 0 failed, 10 ignored.

## Deviations
None. The plan's optional infrastructure test was added. LF line endings kept; no files outside the plan were touched.
