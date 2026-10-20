# U — ComfyUI update modes: "latest commit (git pull)" or "tag version" (draft by main, 2026-10-20 14:35)

## Owner request (14:33)
"When updating ComfyUI have an option that git pull (latest commit) or checkout to tag version."

## Current state (HEAD e384d43 + session-6 working copy through G2c; checkpoint `patches/s6-07-through-g2c-verified.patch`)
- Domain `crates/domain/src/comfy_runtime/update.rs:32-36` `UpdateTarget { LatestRelease, Tag(Tag), BranchFfOnly }`;
  `can_update` (:67-90): dirty / unfinished operation / diverged refused; BranchFfOnly needs a branch + upstream.
  G2b added `BranchFacts`, `release_line`, `is_release_line`, `checkout_placement` (default branch when it contains the
  target, else the tag's `release/vX.Y`, else detached; never moves a branch with unpublished commits).
- Application `crates/application/src/comfy_runtime/model.rs:127-131` `FrozenUpdate { commit, tag, fast_forward }`;
  `update_workflow.rs` freeze → persist Intent → `finish` (apply when HEAD ≠ target, or G2c: detached at target) →
  requirements reinstall with the pin guard → verify → publish manifest (`comfyui_reference` = tag or actual branch).
- Infrastructure `crates/infrastructure/src/comfy_runtime/git.rs`: `fetch --tags` (default refspec also updates
  `origin/*`), `freeze` (:236-283: LatestRelease via GitHub resolve + tag peel; Tag via tag peel; BranchFfOnly via
  `@{upstream}`), `apply` (ff arm `merge --ff-only`; otherwise `check_out(tag)` = G2b placement + `checkout -B` +
  upstream, or detached). Placement facts helper `crates/infrastructure/src/environment/source/placement.rs`.
- Desktop `crates/desktop/src/ipc/comfy_dto.rs:377-398` `UpdateTargetDto` (kinds `latest_release`, `tag`,
  `branch_ff_only`); command `comfy_update`.
- UI `ui/src/features/comfy-runtime/versions-panel.tsx`: "Update to latest release", "Fast-forward {branch}" (only when
  on a branch with upstream; on `release/v0.42` it brings only v0.42.x patches), "Release tag … Switch to {tag}".
- Setup wizard (`ui/src/features/setup/setup-wizard.tsx` ~:671-700) offers "Latest release" / "Specific tag"; IPC
  `ComfyRefDto::Branch` exists (`env_dto.rs:892-897`) and `source.rs` clones a branch with
  `checkout -B <b> --track origin/<b>`, but the UI never sends it.

## Proposed plan
1. Domain: `UpdateTarget::LatestCommit` ("the head of the remote default branch"). `can_update`: same refusals as `Tag`
   (dirty, unfinished operation, diverged); no branch/upstream requirement.
2. Application: `FrozenUpdate.branch: Option<String>` (Some(default branch) only for LatestCommit). Persist with the
   update run (serde default None → no migration if stored as JSON; coder confirms). `finish` unchanged except that a
   LatestCommit run whose HEAD already equals the target but is detached is re-placed exactly like G2c.
3. Infrastructure `git.rs::freeze(LatestCommit)`: default branch D from `refs/remotes/origin/HEAD` (reuse the placement
   helper's default-branch fact, `valid_ref_name`), commit = `rev-parse --verify refs/remotes/origin/<D>^{commit}`
   (after the workflow's fetch); no default branch → `RuntimeError::Failed("The remote has no default branch.")`.
   `apply`: the existing non-ff arm (`check_out(tag = None)`) — placement puts it on `Branch(D)` (D contains its own
   tip) unless local D has unpublished commits (then detached, reason logged).
4. Manifest: `comfyui_reference` = the actual branch after apply (e.g. `master`) — existing behaviour.
5. DTO `UpdateTargetDto::LatestCommit` (kind `latest_commit`), regenerate bindings.
6. UI Version card: two clearly labelled choices —
   - "Update to latest commit (git pull)": helper "Moves ComfyUI to the newest commit on master, like `git pull` on
     master. Development code can be less stable than a release." Confirmation dialog names the branch.
   - "Update to latest release" + "Release tag … Switch to {tag}" (existing; tag checkout).
   Keep "Fast-forward {branch}" (pull on the current branch).
7. Wizard (optional, question 2): ComfyUI version choice adds "Latest commit (master)" → `comfyuiRef: { kind: 'branch',
   value: 'master' }`.
8. Tests: domain `latest_commit_update_has_the_tag_refusals`; real git (placement fixtures):
   `latest_commit_update_moves_a_release_branch_install_to_the_default_branch_head`,
   `latest_commit_update_with_unpublished_default_branch_commits_is_detached`; application
   `latest_commit_update_records_the_branch_as_reference`; Vitest `offers_latest_commit_and_tag_updates`
   (+ wizard `latest_commit_choice_sends_the_master_branch` if 7 is accepted).
9. Docs: DOCUMENTATION ComfyUI updates; REQUIREMENTS FR-COMFY-6/7 note (owner decision 2026-10-20).

## Questions for the advisor
1. Is `LatestCommit` as a new update target (vs. reusing BranchFfOnly after attaching to master) the minimal correct
   design? Any persistence/migration need for `FrozenUpdate.branch` (check `crates/infrastructure/src/comfy_runtime/sqlite.rs`)?
2. Include the wizard choice (7) now? (Owner earlier: "make updating ComfyUI by git pull easy"; open owner question
   (c) from the G2b ruling.)
3. Recovery: a crash between apply and publish for a LatestCommit run — any special handling vs. tag runs?
4. Anything in the update policy (diverged on a release branch when moving to master, unfinished operations) that
   must differ for LatestCommit?
