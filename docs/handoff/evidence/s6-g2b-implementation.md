# Package G2b implementation report (coder)

Plan: `docs/handoff/prompts/session6/g2b-release-branch.md` (advisor stand-in ruling G2b, option (a)) plus polish P1. Native finding N12: patch tags (v0.42.1) exist only on `release/vMAJOR.MINOR`, so the default-branch-only placement from package G still left such installs detached.

No git write command was run in this repository. Default MSVC toolchain and default `target/` were used; no `RUSTUP_TOOLCHAIN` / `CARGO_TARGET_DIR`. One cargo command at a time, `-j 6`.

## Per-file summary

### `crates/domain/src/comfy_runtime/update.rs`
- `CheckoutPlacement::Branch` doc updated: "On the default branch, or the tag's release branch, moved to the commit..." (line ~95).
- `BranchFacts<'a> { name, contains_target, local_only_commits }` (line 106, `Debug, Clone, Copy, PartialEq, Eq`).
- `release_line(tag) -> Option<String>` (line 113): strips `v`, exactly 3 `.`-parts, each non-empty ASCII digits, returns `release/v{major}.{minor}`.
- `is_release_line(branch) -> bool` (line 120): strips `release/v`, exactly 2 `.`-parts, each non-empty ASCII digits. Private helper `all_digits`.
- `checkout_placement(default: Option<BranchFacts>, release: Option<BranchFacts>, current_branch: Option<&str>)` (line 134), rules in the approved order: no default -> `Detached("the remote has no default branch")`; current branch not default and not a release line -> `Detached("HEAD is on another branch")`; default contains target -> unpublished? `Detached("the local default branch has unpublished commits")` : `Branch(default)` (never falls through to release); release contains target -> unpublished? `Detached("the local release branch has unpublished commits")` : `Branch(release)`; else `Detached("the target is on neither the default branch nor its release branch")`.
- Tests: `checkout_placement_attaches_only_when_nothing_local_is_lost` (line 205) rewritten with helper `on(name, contains, local)` and covers all 13 listed cases (the three `current` variants of the release-attach case are looped); new `checkout_placement_derives_release_lines` (line 288) covers every listed `release_line` / `is_release_line` input.

### `crates/infrastructure/src/environment/source/placement.rs`
- `placement(git, commit, tag: Option<&str>, cancel)` (line 22).
- Extracted `branch_facts(git, name, commit, cancel) -> Result<(bool, u64), Cancelled>` (line 82): `merge-base --is-ancestor <commit> refs/remotes/origin/<N>` (exit 0 -> true, anything else false), `rev-parse --verify --quiet refs/heads/<N>`, then `rev-list --count refs/remotes/origin/<N>..refs/heads/<N>` (parse failure -> `u64::MAX`), else 0.
- Assembly: default branch resolved as before, current branch as before, `branch_facts(default)`; the release branch is `None` when the default branch contains the target, otherwise `tag.and_then(release_line).filter(valid_ref_name)` mapped through `branch_facts`.

### `crates/infrastructure/src/environment/source.rs`
- `checkout_commit` gained `tag: Option<&str>` (line 341) and forwards it to `placement::placement`; doc comment updated.
- `clone_into_stage` passes `Some(resolved.reference.as_str())` (line 322). Manager's reference is "HEAD", so `release_line` yields `None` and the default branch is used as before. Apply arms are unchanged (`checkout -B N <commit>`, then best-effort `branch --set-upstream-to=origin/N N`).

### `crates/infrastructure/src/comfy_runtime/git.rs`
- `check_out` gained `tag: Option<&str>` (line 87), forwarded to `placement`; doc comment updated. `apply` passes `frozen.tag.as_deref()` (line 317). Apply arms, `can_update`, dirty/diverged and HEAD==commit checks unchanged.

### Tests (real Git in temp fixtures)
- `crates/infrastructure/src/environment/source/tests.rs`: `a_patch_tag_clone_attaches_to_its_release_branch_and_can_pull` (line 885) and `a_patch_tag_clone_whose_release_branch_lacks_it_is_detached` (line 934), fixtures as specified (remote always returns to `main` before cloning). `a_tag_off_the_default_branch_is_detached` and `a_tag_clone_stays_on_the_default_branch_and_can_pull` unchanged and pass (`v2.0.0-side` is not a release-line tag).
- `crates/infrastructure/src/comfy_runtime/git/placement_tests.rs`: added `remote: PathBuf` to `Install` (line 56) plus two small fixture helpers `publish_patch` (line 94: creates `release/v1.0` + tag `v1.0.1` in the remote, back to `main`, `git fetch --tags` in the install) and `publish_patch_two` (line 106); tests `a_patch_tag_switch_attaches_to_its_release_branch_and_back` (line 240) and `local_commits_on_the_release_branch_keep_the_update_detached` (line 281) per spec.

### Polish P1 (UI)
- `ui/src/features/setup/setup-wizard.tsx`: `const listed = [...offers].sort((a, b) => Number(b.recommended) - Number(a.recommended))` (line 175; stable sort keeps backend order otherwise); the profile radios render `listed.map` (line 619). `offers`, `eligible`, `chosen` and all selection logic are untouched.
- `ui/src/features/setup/setup-wizard.test.tsx`: `recommended_profile_is_listed_first` (line 197): profiles older(false), newer(true), third(false) render as `profile-newer`, `profile-older`, `profile-third`, and the first radio is checked.

### Docs
- `docs/REQUIREMENTS.md`: FR-COMFY-6 G note replaced with the approved wording (line 310); section 3.2 line 163 now `ComfyUI/  git checkout on master or release/vX.Y (models/, ...)`.
- `docs/DOCUMENTATION.md` line 299: ComfyUI updates paragraph replaced with the approved wording (the Fast-forward/Repair/fresh-install sentences kept verbatim).

## Commands and results (run from repo root, in this order at the end)
| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | exit 0 (after `cargo fmt --all`) |
| `cargo clippy --workspace --all-targets --locked -j 6 -- -D warnings` | clean, exit 0 |
| `cargo test -p domain --locked -j 6 checkout_placement` | 2 passed, 0 failed (45 filtered) |
| `cargo test -p infrastructure --locked -j 6 tag_clone` | 3 passed, 0 failed (lib; 430 filtered): both new `a_patch_tag_clone_*` + `a_tag_clone_stays_on_the_default_branch_and_can_pull` |
| `cargo test -p infrastructure --locked -j 6 placement` | 7 passed, 0 failed (lib; 426 filtered): 5 `comfy_runtime::git::placement_tests::*` (3 existing + 2 new) + 2 unrelated name matches |
| `cargo test --workspace --locked -j 6 --no-fail-fast` | 758 passed, 0 failed, 10 ignored. application 206; desktop lib 54, acl 11, bindings 1 (+1 ignored), navigation 2, stack_reserve 2, startup_smoke 0 (+1 ignored); domain 47; infrastructure lib 426 (+7 ignored), real_setup 0 (+1 ignored), workflow 9; doc-tests 0 |
| `pnpm -C ui lint` | exit 0, no findings |
| `pnpm -C ui typecheck` | exit 0 |
| `VITEST_MAX_WORKERS=2 pnpm -C ui test` | 30 files passed, 194 tests passed, 0 failed (`recommended_profile_is_listed_first` also run alone: 1 passed) |
| `node scripts/check-arch-deps.mjs` | `Architecture dependencies: OK` |

Git tests executed for real (system git); none were skipped.

## Deviations / notes
- No plan conflict with the code; no item stopped.
- Added two tiny private fixture helpers (`publish_patch`, `publish_patch_two`) in `placement_tests.rs` to avoid duplicating the remote setup between the two specified tests; behavior is exactly the spec's sequence.
- The editor hook reformats `.rs` files with an older rustfmt style (2021-style import order / `assert!(` layout) after each edit; `cargo fmt --all` was run after the last Rust edit and `--check` is clean. `publish.rs` was momentarily flagged by `--check` (hook residue from an earlier package); after `cargo fmt --all` it has no diff against HEAD. All edited files are LF.
- Files outside the stated scope were not edited by hand. `ui/src/shared/ipc/bindings.ts` is unaffected (no DTO change).
