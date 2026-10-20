ROLE: coder (write-capable). Mechanically implement the advisor-approved plan below. Do not redesign or expand scope. If the plan is underspecified or conflicts with the code, stop that item and report the gap precisely instead of improvising.

## Build environment and repository rules (mandatory)
- WORKING COPY: `D:\vjump\vjump-comfyui-portable-with-resource-manager` (branch `feat/m0-skeleton`, HEAD `e384d43`). Uncommitted working-copy changes = verified session-6 packages Q, S0, F, G, X (`docs/handoff/patches/s6-*`) + session-6 docs: preserve them; never revert files you did not change for this package.
- Read `AGENTS.md` and `docs/handoff/tooling/coder-env.md` first. Default MSVC toolchain from `rust-toolchain.toml`, default `target/`; do NOT set RUSTUP_TOOLCHAIN or CARGO_TARGET_DIR.
- Memory: 32 GB machine, ~10 GB free. Use `-j 6`; targeted runs while iterating; workspace-wide commands once at the end. Never run two cargo commands at once. Vitest with `VITEST_MAX_WORKERS=2`. Git tests must actually execute.
- Rust: rustfmt, clippy `-D warnings`; TypeScript: 2-space, single quotes, no semicolons; match surrounding style and comment density. An editor hook may reformat `.rs` files after each edit — run `cargo fmt --all` after your last Rust edit and keep diffs to intended hunks. Normalize edited files to LF.
- Do not commit, push, stash, reset, checkout, apply, clean or touch git history of THIS repository (git in temp test fixtures is fine). Do not modify files outside the stated scope. Do not touch `D:\vjc-s6\` or `D:\comfyui-test\`; do not kill `vjumpkung-comfyui-desktop` processes.

## Native finding (main, fresh first run 2026-10-20 13:50, `docs/handoff/evidence/s6-native-register.md` N12)
After package G, a first run on the default "Latest release" (v0.42.1) still left ComfyUI detached: "Checking out 63be2f9b… detached, because the target is not on the default branch." ComfyUI publishes `vX.Y.0` on `master` but patch tags only on `release/vMAJOR.MINOR` (v0.42.1 only in `origin/release/v0.42`; v0.41.1–3 only in `release/v0.41`; older commits are in several release branches, so derive the branch name from the tag, not from `--contains`). `git pull` therefore still fails for every patch-release install.

## Approved plan (advisor stand-in ruling G2b: option (a))
### 1. Domain `crates/domain/src/comfy_runtime/update.rs` — replace `checkout_placement`
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BranchFacts<'a> { pub name: &'a str, pub contains_target: bool, pub local_only_commits: u64 }
/// `release/vMAJOR.MINOR` for a `vMAJOR.MINOR.PATCH` tag (where ComfyUI publishes patch releases).
pub fn release_line(tag: &str) -> Option<String>   // strip leading "v"; exactly 3 '.'-separated parts, each non-empty ASCII digits; else None
pub fn is_release_line(branch: &str) -> bool       // strip "release/v"; exactly 2 '.'-separated parts, each non-empty ASCII digits
pub fn checkout_placement(default: Option<BranchFacts<'_>>, release: Option<BranchFacts<'_>>, current_branch: Option<&str>) -> CheckoutPlacement
```
Rules in order: (1) `default` None → `Detached("the remote has no default branch")`; (2) `current_branch` Some(b) with b ≠ default.name and `!is_release_line(b)` → `Detached("HEAD is on another branch")`; (3) `default.contains_target` → `local_only_commits > 0` ? `Detached("the local default branch has unpublished commits")` : `Branch(default.name)` — never fall through to the release branch; (4) `release` Some(r) with `r.contains_target` → `r.local_only_commits > 0` ? `Detached("the local release branch has unpublished commits")` : `Branch(r.name)`; (5) else `Detached("the target is on neither the default branch nor its release branch")`. Update the doc comment on `Branch` ("the default branch, or the tag's release branch").
### 2. Facts `crates/infrastructure/src/environment/source/placement.rs`
`placement(git, commit: &str, tag: Option<&str>, cancel)`. Extract `branch_facts(git, name, commit, cancel) -> Result<(bool, u64), Cancelled>` running, for branch N: `merge-base --is-ancestor <commit> refs/remotes/origin/<N>` (exit 0 ⇒ true; anything else incl. missing ref ⇒ false); `rev-parse --verify --quiet refs/heads/<N>`; if it exists `rev-list --count refs/remotes/origin/<N>..refs/heads/<N>` (parse failure ⇒ `u64::MAX`), else 0. Assembly: D as today; current as today; `default = branch_facts(D)`; `release = None` if `default.contains_target`, else `tag.and_then(release_line).filter(|l| valid_ref_name(l))` mapped through `branch_facts`.
### 3. Callers
`source.rs` `checkout_commit` gains `tag: Option<&str>`; `clone_into_stage` passes `Some(resolved.reference.as_str())` (Manager's reference is "HEAD" → no release line → default branch as today). `comfy_runtime/git.rs` `check_out` gains `tag: Option<&str>`; `apply` passes the frozen target's tag (`frozen.tag.as_deref()` or the actual field name). Apply arms unchanged: `Branch(N)` → `checkout -B N <commit>` then best-effort `branch --set-upstream-to=origin/N N` (essential for release branches: `-B` with a SHA never sets tracking). `can_update`, dirty/diverged checks, HEAD==commit checks and `install.json` unchanged.
### 4. Tests (exact names)
Domain: keep `checkout_placement_attaches_only_when_nothing_local_is_lost` with a helper `fn on(name, contains, local) -> BranchFacts` covering: (D✓0,-,None)→Branch master; (D✓0,-,"master")→Branch master; (None,rel✓0,"master")→no default; (D✗,rel✓,"dev")→another branch; (D✓1,-,None)→default unpublished; (D✓MAX,rel✓0,"master")→default unpublished; (D✗,release/v0.42✓0,None)→Branch release/v0.42; (D✗,release/v0.42✓0,"master")→Branch release/v0.42; (D✗,release/v0.42✓0,"release/v0.41")→Branch release/v0.42; (D✓0,-,"release/v0.42")→Branch master; (D✗,release/v0.42✓2,None)→release unpublished; (D✗,release/v0.42✗,None)→neither; (D✗,None,"master")→neither. New `checkout_placement_derives_release_lines`: `release_line` "v0.42.1"→"release/v0.42", "v0.42.0"→"release/v0.42", "v2.0.0-side"→None, "v0.42"→None, "HEAD"→None, "vx.1.2"→None; `is_release_line` "release/v0.42"→true, "release/v0.18.3"→false, "release/foo"→false, "master"→false.
`crates/infrastructure/src/environment/source/tests.rs` (fixture remote; commit inside the remote, then always `checkout main` before cloning so `origin/HEAD` stays main): `a_patch_tag_clone_attaches_to_its_release_branch_and_can_pull` (remote: `checkout -b release/v1.0 v1.0.0`, README "patch", commit, `tag v1.0.1`, `checkout main`; resolve Tag("v1.0.1"), clone; HEAD = `release/v1.0`, upstream `origin/release/v1.0`, HEAD == patch, clean; then remote `checkout release/v1.0`, commit "patch two", `checkout main`; install `pull --ff-only` → HEAD == patch two, still on `release/v1.0`); `a_patch_tag_clone_whose_release_branch_lacks_it_is_detached` (remote: `branch release/v1.0 v1.0.0`, `checkout -b hotfix v1.0.0`, commit, `tag v1.0.1`, `checkout main`; clone → `--abbrev-ref HEAD` == "HEAD", HEAD == hotfix commit). Keep `a_tag_off_the_default_branch_is_detached` unchanged (adapt only if its fixture now matches a release line — it must still detach).
`crates/infrastructure/src/comfy_runtime/git/placement_tests.rs`: add a `remote: PathBuf` field to the fixture; `a_patch_tag_switch_attaches_to_its_release_branch_and_back` (release line as above, `git fetch --tags` in the install; switch to v1.0.1 → head `release/v1.0`, upstream `origin/release/v1.0`, `main` unchanged, revision branch == Some("release/v1.0"); add "patch two" in the remote, `pull --ff-only` in the install → HEAD == patch two; switch to v1.1.0 (on main) → head `main`, `release/v1.0` == patch two); `local_commits_on_the_release_branch_keep_the_update_detached` (attach to release/v1.0, commit local work; switch v1.1.0 → head main, `release/v1.0` == local commit; switch v1.0.1 → detached, `release/v1.0` == local commit).

## Polish P1 (same package, UI only) — recommended profile first
Native finding N9: the recommended Windows profile (py312) is preselected but listed LAST in the wizard profile step. In `ui/src/features/setup/setup-wizard.tsx`, render profiles with recommended ones first, otherwise keeping the backend order (stable sort); selection logic unchanged. Vitest `recommended_profile_is_listed_first`.

## Docs (only these)
- `docs/REQUIREMENTS.md` FR-COMFY-6 — replace the G note with: "ComfyUI stays on a branch that tracks `origin`, so `git pull` works: `master` when the installed tag is on it, otherwise the tag's release branch `release/vMAJOR.MINOR`, where ComfyUI publishes patch releases. Switching tags moves that local branch pointer only when the tree is clean and the branch has no unpublished commits. A tag on neither branch, or a checkout on a branch of the user's own, is checked out detached as before." §3.2: `ComfyUI/  git checkout on master or release/vX.Y`.
- `docs/DOCUMENTATION.md` ComfyUI updates paragraph (added by G) — replace with: "ComfyUI is a normal git checkout that tracks `origin`, so `git pull` works. A `vX.Y.0` release is on `master`, so `git pull` brings the latest development code. A patch release such as v0.42.1 exists only on ComfyUI's `release/v0.42` branch, so `git pull` brings later v0.42.x patches only; use the app's version switch to move to a newer minor version. Prefer the app's Fast-forward button: it reinstalls requirements with the torch pin guard. After a manual `git pull`, run Repair. Fresh installs: the app creates nothing inside the install folder's `ComfyUI` before setup clones it, and restarts once after first-time setup."

## Success criteria (run at the end; report exact counts)
```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -j 6 -- -D warnings
cargo test -p domain --locked -j 6 checkout_placement
cargo test -p infrastructure --locked -j 6 tag_clone
cargo test -p infrastructure --locked -j 6 placement
cargo test --workspace --locked -j 6 --no-fail-fast
pnpm -C ui lint
pnpm -C ui typecheck
VITEST_MAX_WORKERS=2 pnpm -C ui test
node scripts/check-arch-deps.mjs
```
OUTPUT: per-file summary with file:line, key diffs, every command result with counts, deviations with reasons. Also write it to `docs/handoff/evidence/s6-g2b-implementation.md`.
