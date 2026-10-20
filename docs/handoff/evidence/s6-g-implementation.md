# Session 6 — package G (git-friendly ComfyUI checkout) implementation report

Coder: Sonnet 5.5 (fallback coder). Spec: `docs/handoff/prompts/session6/g-git-friendly-comfyui.md`. Working copy `D:\vjump\vjump-comfyui-portable-with-resource-manager` (branch `feat/m0-skeleton`, HEAD `e384d43`; Q, S0 and F changes preserved). Default MSVC toolchain and `target/`; `-j 6`; Vitest with `VITEST_MAX_WORKERS=2`. No git write command was run on this repository; git inside temp test fixtures only. The ignored native smoke test was not run (main runs it).

## Per-file summary

### G2 — stay on the default branch
- `crates/domain/src/comfy_runtime/update.rs:94-122` — `pub enum CheckoutPlacement { Branch(String), Detached(&'static str) }` and `pub fn checkout_placement(default_branch, current_branch, target_on_default, local_only_commits)` with the five rules in the specified order and the exact reason strings. Test `checkout_placement_attaches_only_when_nothing_local_is_lost` (`:164`) covers a detached HEAD and HEAD already on the default (both attach), every rule on its own, rule order (earlier rules win when later facts are also bad) and `u64::MAX`.
- `crates/infrastructure/src/environment/source/placement.rs` (new, shared helper, `pub(crate)`) — `placement(git, commit, cancel) -> Result<CheckoutPlacement, Cancelled>` (`:21`). `git` is a closure `Fn(&[&str]) -> CommandSpec` that each call site binds to its own Git command style (`-C <dir>`, controlled environment). It gathers the facts exactly as specified:
  - `symbolic-ref --quiet refs/remotes/origin/HEAD`, strip `refs/remotes/origin/`, must pass `valid_ref_name` (reused from `source.rs`), else no default;
  - `symbolic-ref --quiet --short HEAD` (any failure = detached = `None`);
  - `merge-base --is-ancestor <commit> refs/remotes/origin/<D>` (exit 0 = true, any other outcome = false);
  - `rev-parse --verify --quiet refs/heads/<D>`, then `rev-list --count refs/remotes/origin/<D>..refs/heads/<D>` (0 when the local branch does not exist).
  Probes run through `process::run` with `DiscardSink` and a 60 s timeout (`probe`, `:77`); only cancellation propagates, any other failure answers "not known" (which keeps the checkout detached). When the default branch is unknown the remaining probes are skipped (rule 1 wins regardless).
- `crates/infrastructure/src/environment/source.rs`
  - `:29` `pub(crate) mod placement;` and `:15` import of `CheckoutPlacement`.
  - `clone_into_stage` (`:304-321`): the `Some(branch)` arm is textually the same command as before (`checkout -B <b> --track origin/<b>`); the `None` arm now calls `checkout_commit` (`:333-380`). The existing `head != resolved.commit` verification is unchanged.
  - `checkout_commit`: builds the placement with the step's retained handles and cancel token; `Branch(D)` runs `checkout -B D <commit>` then `branch --set-upstream-to=origin/D D` (failure only logged with `ctx.log`, cancellation still propagates); `Detached(reason)` logs "Checking out {commit} detached, because {reason}." through `ctx.log` and runs the old `checkout --detach <commit>`. All commands go through `run_step`, so progress streaming and error mapping are as before. This also attaches the ComfyUI-Manager clone (`install_manager` -> `clone_checkout`) to its default branch.
- `crates/infrastructure/src/comfy_runtime/git.rs`
  - `:5` import; `check_out` (`:87-122`): same helper with the update's `command()`; `Branch(D)` -> `checkout -B D <commit> --` then best-effort `branch --set-upstream-to=origin/D D` (`tracing::info!` on failure, cancellation propagates); `Detached(reason)` -> `tracing::info!(reason, ...)` and the old `checkout --detach <commit> --`.
  - `apply` (`:308-317`): the fast-forward arm is unchanged (`merge --ff-only`); the non-ff arm calls `check_out`. `can_update` / dirty / diverged policy still runs first; the post-apply `revision().commit == frozen.commit` check is unchanged.
  - `:360` `#[cfg(test)] mod placement_tests;`
- Tests
  - `crates/infrastructure/src/environment/source/tests.rs`: `a_tag_clone_stays_on_the_default_branch_and_can_pull` (`:829`: HEAD `main`, `main@{upstream}` = `origin/main`, HEAD = tag commit, clean tree; `git pull --ff-only` reaches the remote's second commit; after a new remote commit another `pull --ff-only` reaches it and HEAD stays `main`), `a_tag_off_the_default_branch_is_detached` (`:860`: tag on a side branch that is not on `main` -> `rev-parse --abbrev-ref HEAD` = `HEAD`, HEAD = side commit). One extra assertion in the existing `the_manager_is_installed_at_the_approved_commit_and_existing_folders_are_respected` (`~:1015`): the Manager clone is on `main`.
  - `crates/infrastructure/src/comfy_runtime/git/placement_tests.rs` (new, real system git, `GitRepository::with_git`, plain `git clone` of a local remote with tags v1.0.0/v1.1.0): `switching_tags_on_an_attached_checkout_keeps_the_branch` (`:125`), `a_detached_checkout_is_attached_on_update` (`:155`, including the case where local `main` was deleted and is recreated tracking `origin/main`), `local_commits_keep_the_update_detached` (`:187`: HEAD detached at the tag, `main` still equals the local commit, tree clean; a second switch stays detached).

### G1 — clone first
- `crates/infrastructure/src/models/acquisition/stack.rs:62-79` — `open_with_bandwidth(..., create_default_root: bool)`; false = no `create_dir_all`, `AuthorizedModelRoots::new([])`; `open()` (`:52`) passes `true`.
- `crates/infrastructure/src/models/library/roots.rs:75` — `LibraryRoots::deferred(install_root, extra, locks)` (empty roots, nothing created); `new` now creates the folder, calls `deferred`, then installs the default root (same behaviour as before). `refresh()` is untouched.
- `crates/infrastructure/src/models/library/stack.rs:44-51` — `LibraryStack::open(..., create_default_root: bool)` chooses `new` or `deferred`.
- `crates/desktop/src/state.rs:993-1040` — `defer_models = manifest is ManifestState::Absent && symlink_metadata(root/"ComfyUI") is NotFound`, uses the manifest read at the start of `compose` and the `ComfyUI` check is made just before the stacks open (after the startup reconcile, so a rolled-forward activation that left a `ComfyUI` folder is never deferred); both stacks get `!defer_models`; when deferred, `services.nodes.reconcile()` is skipped with `tracing::info!("custom node scan deferred until ComfyUI is installed")`. Restricted mode is not reused.
- `crates/desktop/src/ipc/env_commands.rs:166` — `fn restarts_after_success(mode) -> bool` (`Rebuild | FirstRun`), used at `:188` in `setup_run`; unit test `first_run_and_rebuild_restart_after_success` (`:350`).
- `ui/src/features/setup/run-progress.tsx:255-259` — when `run.status === 'succeeded' && run.mode === 'first_run'`: `<p role="status">Setup finished. Restarting the app to open the new installation…</p>`.
- Call sites updated for the new parameter (pass `true`): `crates/desktop/tests/support/mod.rs` (`LibraryStack::open`), `crates/infrastructure/src/models/library/tests.rs` (`Fx::with_extras`).
- The scaffold-merge path (`publish.rs`) is untouched.
- Tests
  - `crates/desktop/src/state/recovery_tests.rs`: `fresh_root_composes_without_creating_comfyui` (`:570`; empty root, `compose_at`, no `ComfyUI` entry, `setup_services()` available with `SetupPhase::NeedsSetup`, `services()` ok), `activated_root_without_comfyui_still_creates_models` (`:595`; valid manifest, no `ComfyUI`, compose creates `ComfyUI/models`).
  - `crates/infrastructure/src/models/library/tests.rs:883` `deferred_library_roots_register_comfyui_models_once_it_exists` (no roots, `refresh` reports one `RootMissing` for `models` and creates nothing; after `ComfyUI/models` exists `refresh` has no warnings and registers `models` as `Ready`).
  - `crates/infrastructure/src/models/acquisition/tests.rs:912` `a_deferred_download_stack_creates_nothing_under_the_root` (no `ComfyUI`; the root holds only `data` from the staging area; the default root is not authorized).
  - `ui/src/features/setup/run-progress.test.tsx:208` `first_run_success_announces_the_restart` (shown for first_run + succeeded with `role="status"`, absent for repair + succeeded and for first_run + running).

### Docs
- `docs/REQUIREMENTS.md` — §3.2 layout line (`ComfyUI/ git checkout on the default branch ...`, `:163`) and FR-COMFY-6 (`:310`) with the appended sentences (verbatim).
- `docs/DOCUMENTATION.md:299` — the specified paragraph in "Updating ComfyUI and the version picker" (placed after the "Switching to an older tag ..." paragraph).

## Commands (repository root, final state)
| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | exit 0 |
| `cargo clippy --workspace --all-targets --locked -j 6 -- -D warnings` | exit 0, no warnings |
| `cargo test -p domain --locked -j 6 checkout_placement` | 1 passed, 0 failed, 45 filtered |
| `cargo test -p infrastructure --locked -j 6 tag_clone` | 1 passed (`a_tag_clone_stays_on_the_default_branch_and_can_pull`), 0 failed, 419 filtered; real_setup/workflow targets 0 tests |
| `cargo test -p infrastructure --locked -j 6 comfy_runtime` | 20 passed, 0 failed, 2 ignored (subprocess fixtures), 398 filtered; includes the 3 new `placement_tests` |
| `cargo test -p infrastructure --locked -j 6 --lib environment::source` (extra) | 19 passed, 0 failed, 396 filtered |
| `cargo test -p infrastructure --locked -j 6 --lib deferred` (extra) | 2 passed |
| `cargo test -p desktop --locked -j 6 --lib fresh_root_composes_without_creating_comfyui` | 1 passed, 52 filtered |
| `cargo test -p desktop --locked -j 6 --lib -- fresh_root... activated_root... first_run_and_rebuild...` (extra) | 3 passed |
| `cargo test --workspace --locked -j 6 --no-fail-fast` | exit 0; 742 passed, 0 failed, 9 ignored: application 205; desktop lib 53; bin 0; acl 11; bindings 1 (+1 ign); navigation 2; stack_reserve 2; startup_smoke 0 (+1 ign); domain 46; infrastructure lib 413 (+7 ign); real_setup 0 (+1 ign); workflow 9; doc-tests 0 |
| `pnpm -C ui lint` | exit 0, no findings |
| `pnpm -C ui typecheck` | exit 0 |
| `VITEST_MAX_WORKERS=2 pnpm -C ui test` | 29 files, 189 tests passed (F baseline 188 + 1 new) |
| `node scripts/check-arch-deps.mjs` | "Architecture dependencies: OK" |

Mutation check: with `defer_models` forced to `false` in `state.rs`, `fresh_root_composes_without_creating_comfyui` fails with "composition created ComfyUI" (restored afterwards; the `state.rs` diff is the intended one).

No DTO changed, so `bindings.ts` was not regenerated; the bindings drift test and the ACL test pass.

## Deviations and findings
1. **Fixtures are not bare.** The spec said "bare-repo fixtures"; the existing `Remote` fixture of `source/tests.rs` is a normal local repo (`main`, tags, `dev`), so the new tests extend it. Pulling from it works the same. The `GitRepository` tests use their own small plain-clone fixture because `Remote` is private to that module.
2. **"Creates nothing under the root" test.** The download stack still creates its staging folder `data/downloads` (the app's own `data` folder, created by `ensure_dirs` anyway), so the test asserts that the root contains only `data`, that there is no `ComfyUI` entry, and that the default model root is not authorized. The guarantee that matters (nothing under `ComfyUI/`) is what is tested.
3. **Helper shape.** The shared helper shares the fact gathering and the decision; the two apply steps (`checkout -B` + best-effort upstream, or `checkout --detach`) stay at each call site because the two sites use different runners and error types (`run_step` streaming to the setup log vs `GitRepository::run` with `RuntimeError`). Probes use `process::run` directly (not `run_quiet`/`read`) so that a non-zero exit is an answer instead of an error.
4. **Probe failures.** Spawn failures, timeouts and cleanup failures of a probe are "not known" (detached); only cancellation propagates. If the commits of the local default branch cannot be counted (the branch exists but `rev-list` fails) they count as `u64::MAX`, so the checkout is detached with the "unpublished commits" reason.
5. **`--` in the update-site branch command.** `checkout -B D <commit> --` keeps the trailing `--` of the previous detach form; the setup site uses the previous form without it (the commit there comes from `ls-remote`/the resolver, as before).
6. **Extra assertion** in the existing Manager test (Manager clone on `main`), as the spec says the Manager clone is deliberately attached.
7. **Files outside the listed scope that had to change to compile:** `crates/desktop/tests/support/mod.rs` and `crates/infrastructure/src/models/library/tests.rs` (new `create_default_root` argument, `true`).
8. **Editor hook caveat.** The rustfmt PostToolUse hook ran after the Write/Edit of `update.rs` and `placement.rs`; `cargo fmt --all` was run after every later Rust edit and `cargo fmt --all -- --check` passes with only intended hunks in the diffs (all touched files are LF).
9. **Observation.** One `cargo test` printed "Blocking waiting for file lock on build directory" once, i.e. another cargo process (not started by this package) was running at that time; no effect on results.
10. Not done (out of scope): no native smoke run, no change to `install.json` semantics, no migration of already detached installs beyond the existing behaviour (they attach on their next app update/tag switch, as planned).
