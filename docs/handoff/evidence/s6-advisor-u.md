**GO with corrections. Do not implement the draft verbatim.** The new target is appropriate, but the draft misses mandatory recovery and validation changes.

Reviewed the working copy at `e384d43` through G2c. No files changed, builds run, or tests executed.

The citations check out with these corrections:

| Evidence | Finding |
|---|---|
| [domain/update.rs:32](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/domain/src/comfy_runtime/update.rs:32), `:67`, `:134` | Target/policy citations correct. Placement also detaches when currently on a user branch; unpublished default-branch commits are not its only exception. |
| [application/model.rs:127](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/application/src/comfy_runtime/model.rs:127) | `FrozenUpdate` citation correct. |
| [update_workflow.rs:134](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/application/src/comfy_runtime/update_workflow.rs:134) | Every non-FF run currently requires a tag. `finish` must change. G2c’s same-commit logic is at `:143–157`. Manifest publication is at `:273–274`. |
| [use_cases.rs:92](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/application/src/comfy_runtime/use_cases.rs:92) | Pending updates already block Start. Version listing at `:229` exposes no default-branch name. |
| [git.rs:231](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/comfy_runtime/git.rs:231) | Fetch is implicit-remote `fetch --tags`. Freeze occupies `:236–285`. Apply at `:296` independently requires a tag for non-FF runs. |
| [placement.rs:28](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/environment/source/placement.rs:28) | Default-branch discovery exists inside `placement`; extraction is required for reuse. |
| [sqlite.rs:168](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/comfy_runtime/sqlite.rs:168) | Runs use `UpdateDoc` JSON, saved at `:303`; no SQL column addition is needed. |
| [comfy_dto.rs:377](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/desktop/src/ipc/comfy_dto.rs:377), [comfy_commands.rs:182](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/desktop/src/ipc/comfy_commands.rs:182) | Enum is `:377–381`, conversion `:383–392`, input `:397–403`. Existing command can remain unchanged. |
| [versions-panel.tsx:172](D:/vjump/vjump-comfyui-portable-with-resource-manager/ui/src/features/comfy-runtime/versions-panel.tsx:172) | Fast-forward visibility checks **branch presence only**, not upstream presence. |
| [setup-wizard.tsx:687](D:/vjump/vjump-comfyui-portable-with-resource-manager/ui/src/features/setup/setup-wizard.tsx:687) | Actual choice is `:687–724`; payload is `:1004–1009`. `ComfyRefDto::Branch` is [env_dto.rs:891](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/desktop/src/ipc/env_dto.rs:891); clone command is [source.rs:308](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/environment/source.rs:308). |

**Corrected implementation plan**

1. **Domain/application target.** Add `UpdateTarget::LatestCommit` in `crates/domain/src/comfy_runtime/update.rs`. Keep `can_update` unchanged: dirty, unfinished and diverged repositories remain refused; branch/upstream requirements remain exclusive to `BranchFfOnly`.

   Add `FrozenUpdate.branch: Option<String>` in application `model.rs`. Add `FrozenUpdate::policy_target()` returning:

   | `fast_forward` | `tag` | `branch` | Policy target |
   |---|---|---|---|
   | true | None | None | `BranchFfOnly` |
   | false | Some(valid tag) | None | `Tag` |
   | false | None | Some(valid branch) | `LatestCommit` |
   | otherwise | | | `InvalidRecord` |

   Validate branch strings using the existing domain safe-reference constraints. Use this method in both `UpdateComfy::finish` and `GitRepository::apply`, replacing their tag-only reconstruction.

2. **Persistence.** In infrastructure `sqlite.rs`, add:

   ```rust
   #[serde(default, skip_serializing_if = "Option::is_none")]
   target_branch: Option<String>,
   ```

   Map it both ways to `FrozenUpdate.branch`. Keep `UpdateDoc.version = 1`, existing fields and phases. **No SQL migration:** [003_comfy_runtime.sql:11](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/migrations/003_comfy_runtime.sql:11) stores the document in `json`.

   Add `branch: None` to existing constructors in runtime Git, application tests, infrastructure placement tests, and `crates/desktop/src/state/recovery_tests.rs`. Reject invalid target combinations before mutation; do not introduce unrelated legacy-document validation.

3. **Fetch and freeze.** Change `ComfyRepository::fetch` in application `ports.rs` to accept `target: &UpdateTarget`; update its workflow call, Git implementation and the fakes in runtime tests and `application/src/settings/diagnostics/tests.rs`.

   For `LatestCommit`, explicitly fetch origin’s branch refs:

   ```text
   git fetch --tags origin refs/heads/*:refs/remotes/origin/*
   ```

   Preserve existing fetch behavior for existing modes. This avoids fetching another configured upstream remote or leaving a default branch stale under a restricted refspec.

   Extract `placement.rs::default_branch` from the existing symbolic-ref probe. Freeze `LatestCommit` using:

   ```text
   git symbolic-ref --quiet refs/remotes/origin/HEAD
   git rev-parse --verify --end-of-options refs/remotes/origin/<D>^{commit}
   ```

   Validate `<D>` with `valid_ref_name`. Save `{commit: C, tag: None, branch: Some(D), fast_forward: false}`. Missing default branch yields the draft’s error; missing/unresolvable tip fails before saving Intent.

4. **Apply and recovery.** Extract `placement_with_default` so existing `placement` discovers the default and delegates, while LatestCommit supplies its **persisted** branch. Extend runtime `check_out` accordingly. Do not rediscover a different default branch on retry.

   Preserve G2b placement safeguards and commands:

   ```text
   git checkout -B <D> <frozen-C> --
   git branch --set-upstream-to=origin/<D> <D>
   ```

   Otherwise use `checkout --detach <frozen-C> --`, retaining the existing logged reason. Leave existing FF execution as `merge --ff-only <frozen-C>`.

   In `finish`, also apply when **Intent + LatestCommit + HEAD already equals C**, including an attached checkout. This establishes placement/upstream and safely repeats an apply interrupted before SourceChanged was saved. Preserve the existing G2c tag condition. Later phases must not re-place source.

   Retain exact-commit checks, pin guard, verification and manifest publication. `comfyui_reference` remains **tag or actual branch**, therefore `None` for a detached LatestCommit result; never substitute the requested branch.

5. **IPC.** Add `UpdateTargetDto::LatestCommit` and its conversion. Wire input:

   ```json
   {"target":{"kind":"latest_commit"},"stopRunning":false,"retry":null}
   ```

   Preserve `latest_release`, `tag` with `name`, and `branch_ff_only`. No command, ACL, gateway implementation, or result-field additions are needed. Regenerate bindings through `export_bindings`.

6. **UI.** Update `VersionsPanel::describeTarget` and add the button alongside existing actions:

   - Button: **“Update to latest commit (git pull)”**
   - Helper: **“Switches to origin’s default branch, normally master. Development code may be less stable than a release.”**
   - Confirmation target: **“the latest commit on origin’s default branch”**

   Retain the existing requirements/verification and stop confirmation text. Use generic branch wording: the current version response cannot name the default branch. Keep latest-release, tag and current-branch FF actions. Preserve backend refusal when upstream is missing.

7. **Docs/scope.** Update `docs/DOCUMENTATION.md:299–303` and FR-COMFY-6 with the explicit development-head option. Correct “Updates use fast-forward only” to distinguish current-branch FF from source switching. Preserve FR-COMFY-7 and §2.4’s release default. **Exclude wizard changes.**

**Decisions on questions 1–4**

| Question | Classification and decision |
|---|---|
| 1 | **AI-decidable:** add LatestCommit. BranchFfOnly resolves the current upstream, so it cannot express release-line → default-head selection. Persist the branch; no SQL migration. |
| 2 | **User-only:** defer wizard development choice. G2b explicitly reserves it for the owner; the present request authorizes update choices. |
| 3 | **AI-decidable:** retain normal journal recovery, with the policy reconstruction and Intent placement corrections above. Retry must never fetch/refreeze “latest.” |
| 4 | **AI-decidable:** keep all existing refusals, including divergence on the current release branch. FR-COMFY-6 provides no exception when changing branches. |

**Required tests** — names below are proposed additions; assertions are acceptance requirements.

| File | Exact test names and assertions |
|---|---|
| `crates/domain/src/comfy_runtime/update.rs` | `latest_commit_update_has_the_tag_refusals`: dirty/unfinished/diverged refused; clean detached/no-upstream allowed. |
| `crates/application/src/comfy_runtime/tests.rs` | `frozen_update_policy_rejects_invalid_target_shapes`: valid combinations map correctly; invalid combinations/references fail. |
| Same | `latest_commit_update_records_the_actual_reference`: completed update publishes frozen SHA and actual branch; detached fallback publishes no branch; constraints and pins remain enforced. |
| Same | `latest_commit_retries_the_frozen_target`: cover Intent before/after apply and subsequent phases; no retry fetch/freeze, exact persisted SHA/branch retained, unrelated HEAD refused, pending failure blocks Start, successful retry verifies environment. Include same-SHA attached-release placement. |
| `crates/infrastructure/src/comfy_runtime/git/placement_tests.rs` | `latest_commit_update_moves_a_release_branch_install_to_the_default_branch_head`: use fixture default `main`; assert frozen SHA, branch/upstream, preserved release pointer and subsequent FF pull. |
| Same | `latest_commit_update_with_unpublished_default_branch_commits_is_detached`: target reached; unpublished branch SHA preserved. Also cover current user-branch fallback. |
| Same | `latest_commit_apply_preserves_the_frozen_commit_and_branch`: advance remote/change origin/HEAD after freeze; apply retains frozen selection. |
| Same | `latest_commit_fetches_origin_even_when_current_upstream_is_elsewhere`: origin/default is refreshed and frozen. |
| Same | `latest_commit_without_a_default_branch_fails_before_checkout`: explicit failure, unchanged HEAD/branches. |
| `crates/infrastructure/src/comfy_runtime/sqlite.rs` | `update_documents_preserve_latest_commit_and_read_legacy_v1`: roundtrip new branch; legacy tag/FF JSON without `target_branch` still reads. |
| `crates/desktop/src/ipc/comfy_dto.rs` | `latest_commit_update_target_roundtrips`: exact `latest_commit` wire spelling and domain conversion. |
| `ui/src/features/comfy-runtime/comfy-page.test.tsx` | `offers_latest_commit_and_tag_updates`: confirmation required; exact new/tag payloads; latest-release and FF actions retained; development helper shown. |

Retain the existing G2b/G2c placement and recovery tests.

**Success checks for main:** regenerate bindings; run workspace tests, formatting, Clippy, UI lint/typecheck/tests, and architecture dependency checks. Verify `bindings_up_to_date`.

Native baseline verified read-only: clean `release/v0.42`, upstream `origin/release/v0.42`, HEAD `63be2f9b` (`v0.42.1`); default `origin/master` at `b1a247c9`; local master has no unpublished commits.

Launch the updated app with `VJUMPKUNG_COMFYUI_HOME=D:\vjc-s6\r1`. Stop ComfyUI, choose Latest commit, confirm, then check:

```powershell
git -c safe.directory=* --no-optional-locks -C D:\vjc-s6\r1\ComfyUI status --short --branch
git -c safe.directory=* -C D:\vjc-s6\r1\ComfyUI rev-parse HEAD origin/master
git -c safe.directory=* -C D:\vjc-s6\r1\ComfyUI rev-parse --abbrev-ref '@{upstream}'
```

Expect clean `master`, matching SHAs and `origin/master`; verify manifest SHA/reference, unchanged protected pins, and Start/Stop. After successful completion, use the app to switch to `v0.42.1`; expect `release/v0.42` tracking origin again.

Material risks: development requirements may conflict with protected pins and must fail closed; safe placement can remain detached; older binaries reject new journal fields because of `deny_unknown_fields`. A native dependency refusal proves guarding, not a successful end-to-end update.