**GO for one corrected A+B coder package. NO-GO for the draft unchanged or for closing residual 6 now.** The owner has authorized both product decisions; no further user decision is needed. This was a read-only review; I did not edit, build, or run tests.

The code confirms four corrections are necessary: setup suppresses `has_install_py`; pin repair overwrites the shared constraints file without profile constraints; the package preview describes a pre-hook environment; and the bound Git object retains the staging pin during failure cleanup.

**Questions 1–5**

| Question | Authority and ruling |
|---|---|
| 1. Policy boundary | **AI-decidable implementation of an authorized exception.** Relax exactly shipped `__pycache__` and D1’s Windows top-level native-image refusal for selected curated Git sources in consented FirstRun setup. Keep every containment, provenance, Git configuration, package-preflight and resource-limit check. |
| 2. Order scope | **User-only product choice, already answered.** Hook first applies throughout the shared workflow, including Custom Nodes installs, updates and dependency retries when the current review approves the hook. Page refusals and consent remain unchanged. |
| 3. Pins and preview | **AI-decidable.** Reassert pins after each mutation; restore profile constraints after hook cleanup; validate the original frozen requirements specification again against the post-hook environment. Details below. |
| 4. Source builds | **AI-decidable scope preservation: keep false.** App-managed resolution/install remains wheel-only. An approved arbitrary hook can itself invoke installers/builds; this flag does not sandbox it. |
| 5. Outstanding user choices | **None.** “Allow every curated node” removes the two specified admission refusals; it does not promise that incompatible requirements, unavailable repositories or failing scripts will succeed. |

**One corrected implementation plan**

All paths below are relative to the repository root. Implement and verify these together; preserve existing session-6 changes and make no Git history changes.

1. **Thread an internal preparation policy, without adding an IPC switch.**

   In `crates/application/src/custom_nodes/model.rs`, add:

   ```rust
   pub enum NodePreparePolicy {
       Strict,
       ConsentedFirstRun,
   }
   ```

   Add this argument to `NodeFiles::prepare` in `ports.rs`, its implementation in infrastructure `files.rs`, and the application fake.

   In `workflow.rs::prepare_target`, derive the policy from persisted application state. `Active` always receives `Strict`. `Setup` receives `ConsentedFirstRun` only when:

   - `setup_binding` has validated the running FirstRun and candidate;
   - the persisted request has `third_party_consent`;
   - the request is Git, `reference == None`, and its repository matches a selected catalog entry using the existing repository normalization.

   A consented setup request outside that curated selection returns `InvalidRequest` **before adapter preparation**. Non-consented setup remains strict.

   Store the policy in `PreparedPlan`. Before committing a relaxed plan, revalidate the persisted consent and selected repository; changed authorization returns `StalePlan` before publication. Keep the existing target-binding check.

   Pass the policy through `LocalNodeFiles::prepare_git_install` and `GitNodes::clone_checkout`. Updates from the page always use strict policy. Registry preparation remains strict; no archive exception is needed.

2. **Close D1 with a genuine raw-output contract.**

   In `crates/infrastructure/src/shared/process.rs`, add an infrastructure-only API:

   ```rust
   pub(crate) async fn run_raw_stdout(
       spec: &CommandSpec,
       cancel: &CancelToken,
       limits: &RunLimits,
   ) -> Result<Vec<u8>, RawOutputError>
   ```

   Use `RawOutputError::{Run(RunError), LimitExceeded, Incomplete}`. Reuse the existing runner lifecycle: allow-listed environment, executable-directory default cwd, process-tree ownership, timeout, cancellation, draining, and retained-resource cleanup.

   Contract:

   - stdout bytes bypass decoding, redaction, line assembly and newline insertion;
   - stdout never enters a sink, diagnostic tail or observer;
   - stderr retains the existing bounded, redacted diagnostics;
   - success requires exit zero, stdout EOF and complete capture;
   - read failure or drain timeout is `Incomplete`;
   - capture exceeding `max_stdout_bytes` is `LimitExceeded`, never truncated success. Continue draining/discarding within the existing timeout/cleanup lifecycle.

   Keep existing text APIs and their callers unchanged. A capture option inside `run_inner` avoids duplicating process management or expanding `RunError` across unrelated adapters.

   In `custom_nodes/git.rs`, add `GitNodes::read_raw(..., cancel)` returning bytes. Share the existing metadata/spec/local-config preflight logic with `run`; retain every environment/configuration restriction. Use the caller’s cancellation token, no `StepSink`, a 120-second timeout and an **8 MiB raw cap**.

   In strict Windows `clone_checkout`, after `cat-file` and before either checkout branch, run:

   ```text
   ls-tree --full-tree -z --name-only <resolved.commit>
   ```

   Use `Some(staging)` through the existing `-C` mechanism. Do not add `-r`: directory entries such as `lib.dll` must also be examined.

   Parse exact bytes: empty output is a valid empty tree; otherwise require a trailing NUL, validate every record as UTF-8 and `SafeFileName`, and enforce the existing tree entry bound.

   | Condition | Result |
   |---|---|
   | Cancellation / classified offline error | Preserve `Cancelled` / `Offline` |
   | Any other command, preflight, capture or incomplete-framing failure | `Failed("The repository tree could not be listed")` |
   | Invalid UTF-8, invalid/empty filename or entry-count excess | `UnsafeSource` |
   | Native-image name | `Failed(ROOT_NATIVE_MESSAGE)` |

   Perform parsing outside the command-error mapping so `UnsafeSource` is preserved. Use the existing Unicode-aware `is_native_image_name`.

   `ConsentedFirstRun` skips this admission listing. Full containment validation still runs after checkout.

   In both Git preparation failure branches in `files.rs`, release the staging-bound `git` object and directory before `remove_owned`. Preserve identity-checked cleanup, survivor retention and the existing `UnsafeSource` preservation behavior.

3. **Apply only the two authorized content exceptions.**

   In `tree.rs`, make fresh-Git validation policy-aware and return deduplicated findings. Strict validation keeps the existing cache refusal and root-native refusal. Relaxed validation:

   - runs the same complete bounded, non-following tree walk with cache rejection disabled;
   - records shipped cache content;
   - on Windows, records top-level native-image entries instead of refusing them.

   Reuse the root-name inspection underlying `refuse_root_native`; do not duplicate extension rules. Propagate findings through `StagedNode.warnings`.

   Keep name validation, links/reparse refusal, depth/entry/byte limits, `.git` placement, marker/provenance checks, clean-checkout checks, safe Git configuration and disabled hooks/filters unchanged.

   Suggested exact warnings:

   - `This repository ships __pycache__ content; consented first-run setup allows it.`
   - `This node has top-level native-image entries that tools may load; consented first-run setup allows them.`

   F already logs each as:

   ```text
   {name}: {warning} (acknowledged on the setup review page).
   ```

   Remove only PlagueKind’s `unavailable` field from `environment/data/curated_nodes.json`. Preserve catalog order, schema, unticked defaults and the future-unavailability mechanism in `curated.rs`.

4. **Run hooks first and preserve reviewed requirements.**

   In application `workflow.rs`, expose `has_install_py` for consented setup when the hook exists and dependencies are needed. Keep it unavailable for non-consented setup’s existing skipped-hook path.

   In `environment/execute.rs::setup_review_decision`, set `run_install_py = review.has_install_py`; retain false for remembered trust, source builds and protected-package authorization. In auto mode, synthesize the current decision for each prepared review rather than accepting an older covering decision that omits the hook.

   In `custom_nodes/setup.rs::commit_for_setup_held`, stop forcing hooks off when persisted FirstRun consent exists. Continue forcing them off without that consent and clearing the other permissions.

   Correct `workflow.rs::dependencies` execution order:

   ```text
   acquire live guard and validate reviewed inputs
   persist DependenciesInProgress / active environment repair intent
   prepare validated frozen InstallSpec
   run approved install.py
   reassert protected pins, including after hook failure/cancellation
   stop if hook or pin restoration failed
   rewrite constraints with protected pins AND profile constraints
   validate frozen InstallSpec against post-hook environment
   install requirements
   reassert protected pins, including after install failure
   cancellation check → final pin assertion → verify_final
   validate final provenance/containment → save refreshed digest → success
   ```

   Keep the live guard through execution and cleanup. Validate frozen package names/versions before running the hook.

   **Preview correction:** retain the initial preview before consent. Build the later `InstallSpec` from registry requirements, guarded reviewed root-file bytes and the original preview’s exact `name==version` additions. After a successful hook and pin restoration, call `preview_install` again on **that same specification**, with restored profile constraints. Reuse/factor the existing protected-change validation; any post-hook validation failure stops before requirements installation and leaves dependency failure/repair state.

   Do not replace frozen versions with a fresh unconstrained plan or reread the root requirements file. The second preview also reruns the existing include/local-path preflight. The initial preview is a delta, not a complete environment snapshot; `PlanReview` exposes requirements rather than a guaranteed package delta, so no additional consent pause is required for resolving those same approved inputs.

   If the reviewed source had no requirements, do not discover hook-generated requirements in this package. Keep final digest refresh permitted only when the hook ran, with final provenance and containment validation intact.

   Add stable log markers:

   ```text
   {name}: running approved install.py before node requirements.
   {name}: checking reviewed requirements after install.py.
   {name}: installing reviewed node requirements.
   ```

   Log the full normalized repository URL and full commit before commit, rather than relying on F’s shortened branch/commit message.

5. **Correct warnings, UI and documentation.**

   `SETUP_HOOK_WARNING` must describe only the skipped-hook path. Replace “Setup never runs it” with:

   > This setup will not run this node’s install.py. If needed, reinstall it in Custom Nodes and approve install.py.

   In `execute.rs::prepare_curated`, derive the persisted hook note from that warning’s presence, not merely `hook_digest`. Successful consented installs must not acquire or retain a new skipped-hook note.

   Update both disclosure locations in `ui/src/features/setup/setup-wizard.tsx`. Suggested review copy:

   > Installing ComfyUI-Manager and selected custom nodes runs third-party code on this computer; a Python environment is not a sandbox. Setup installs the latest commit of each repository’s default branch. For selected custom nodes, setup allows shipped Python cache files and top-level native programs or libraries, runs install.py if present, then installs the reviewed requirements.txt. It does not ask again. Clicking Start setup confirms this.

   This wording deliberately limits hook execution to curated custom nodes: Manager currently uses a separate source/requirements path.

   In `custom-nodes/review-dialog.tsx`, change the hook label to state “before requirements.txt”; preserve unchecked defaults and every consent control.

   Update `README.md`, `docs/DOCUMENTATION.md`, requirements **NFR-SEC-3 / FR-NODE-4 / FR-SETUP-9**, the N plan and advisor-rulings record. Document the setup exception, shared order, app-managed wheel-only behavior and post-hook validation. Amend residual 15 for consented setup. Append completion evidence to the D1 report without rewriting its historical partial results.

**Required tests and assertions**

| File/group | Exact test names and required assertions |
|---|---|
| Shared process | `raw_stdout_preserves_exact_bytes`: chunked NUL, CR/LF, invalid UTF-8 and credential-shaped bytes survive exactly; stdout is absent from diagnostics. `raw_stdout_refuses_overflow_and_incomplete_capture`: exact cap succeeds; excess, read failure and unfinished drain fail. Exercise raw mode in existing cancellation, timeout and retained-cleanup tests. |
| Git parser | `git_root_tree_admission_validates_exact_nul_delimited_names`: empty tree accepted; invalid names/UTF-8 refused; framing fails closed; Unicode native extensions and credential-shaped native filenames detected. |
| Missing D1 tests | Windows `git_install_refuses_root_native_image_before_checkout`: test both root file and native-suffixed directory; exact `Failed(ROOT_NATIVE_MESSAGE)`; recorded `ls-tree` targets the resolved commit and precedes checkout; **no checkout for staging**, no published node, owned staging removed. |
| Missing D1 update test | Windows `git_update_refuses_root_native_image_in_frozen_target_before_checkout`: install safe commit, add native entry remotely, freeze unsafe target; assert exact refusal, listing names frozen commit, no staging checkout/publication, staging removed, live HEAD and content digest unchanged. |
| Git policy | Update `git_specs_always_disable_system_and_global_config` to exercise the raw path and assert the production Windows listing precedes checkout. Record command specs without retaining their owners during cleanup, using `TEST_RELEASE_OWNERS`. |
| Infrastructure policy | `consented_setup_git_prepare_allows_content_findings_but_preserves_containment`: relaxed cache/native fixtures prepare with warnings; strict equivalents refuse; links/reparse points and excessive paths remain refused under both policies. Keep existing nested-native, root-script and installed-node tests. |
| Application boundary | `setup_content_policy_requires_persisted_consent_and_selected_curated_source`: record adapter policy; Active/non-consented are strict; selected consented source is relaxed; invalid selection/reference fails before adapter calls; revoked consent cannot commit relaxed plan. |
| Order and pins | Rename to `install_py_runs_only_with_consent_before_requirements_and_trust_is_namespaced`; assert hook precedes actual node install and trust behavior is unchanged. Add `hook_failure_skips_requirements_and_retains_repair_state` and `hook_pin_repair_restores_profile_constraints_before_requirements`; assert cleanup precedes failure return and profile extras are restored after pin repair. |
| Preview and guard | `requirements_after_hook_use_reviewed_snapshot_and_frozen_versions`: post-hook preview and install consume identical guarded bytes/exact versions despite environment changes; protected drift or preflight failure prevents install. Update `node_execution_guard_spans_preview_hook_install_and_cleanup` for the new order; retain `consented_hook_final_digest_is_saved_before_verified`. |
| Setup execution | Update `curated_auto_consent_installs_every_node_and_logs_warnings`: hook runs iff present, before requirements, without pauses; findings acknowledged; full source logged; no skipped-hook note; no remembered trust/source builds. Keep non-consented skipped-hook and note-survives-retry coverage. |
| Catalog/UI | Rename catalog test to `bundled_catalog_has_no_unavailable_entries`: all 22 available, PlagueKind still index 19. Add wizard `plaguekind_is_selectable_and_sent_with_review_consent`; update `review_page_discloses_third_party_code_and_sends_consent`. Add review-dialog `approved_hook_discloses_install_py_before_requirements`; consent defaults remain unchecked. |

**Risks and closure**

Hooks can fail because their dependencies are not installed yet; that is an authorized consequence of the chosen order. Arbitrary hook effects remain nontransactional. Initial resolution can become stale, hence the second constrained validation. Linux containment remains subject to the existing native-validation gap.

Residual **row 6 may be marked “Closed before consent (D1); residual after consent and for installed/user-placed nodes” only after the Windows admission/cleanup tests pass**, alongside the pinned-uv isolation evidence. Consented setup is already post-consent before these tools run. Do not close row 28’s broader `UnsafeSource` cleanup question from the native-refusal tests.

**Success checks for coder/main**

Run targeted application, infrastructure/process and UI tests first, then once:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -j 6 -- -D warnings
cargo test --workspace --locked -j 6 --no-fail-fast
pnpm -C ui lint
pnpm -C ui typecheck
pnpm -C ui test
node scripts/check-arch-deps.mjs
```

Re-run `node_preview_never_imports_python_modules_from_node_directory -- --nocapture` with verified **uv 0.13.1** first on PATH; report whether it actually ran. Use the prescribed MSVC/default target environment, one Cargo command at a time, and report counts.

On the RTX 3060 machine, main should perform a fresh FirstRun with **PlagueKind ticked**: verify selectable catalog entry, disclosure, no consent pause, acknowledged cache warning, successful dependency/final verification and ComfyUI launch. Separately install a vetted node with both files through Custom Nodes, explicitly approve its hook, and capture the hook-before-requirements log order and unchanged protected pins. A Windows page install of a root-native fixture must still refuse before checkout.