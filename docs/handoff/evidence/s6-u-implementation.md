# S6-U implementation evidence

Date: 2026-10-20. Working copy: `D:/vjump/vjump-comfyui-portable-with-resource-manager`, branch `feat/m0-skeleton`, HEAD `e384d43415087d670d5f7e76647e3801c3d87163`.

Implemented sections 1–7 of `docs/handoff/evidence/s6-advisor-u.md` and all 12 additions in its Required tests table. The draft was not used where it conflicts with the ruling. All requested automated checks pass.

## Per-file summary and key diffs

Counts below compare against the starting working-copy snapshot, including the existing verified session-6 work, rather than against HEAD. This package changes 17 existing files (+797/-49 lines) and adds this report. All changed files use UTF-8 and LF.

| File:line | Package +/− | Change |
|---|---:|---|
| [crates/domain/src/comfy_runtime/update.rs:34](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/domain/src/comfy_runtime/update.rs:34) | +60/−0 | Add `UpdateTarget::LatestCommit`; keep `can_update` unchanged. Add refusal test at :171. |
| [crates/application/src/comfy_runtime/model.rs:130](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/application/src/comfy_runtime/model.rs:130) | +16/−0 | Add `FrozenUpdate.branch` and `policy_target()` (:134). Only FF/no refs, tag-only, and branch-only shapes are valid; branch/tag strings use domain `Tag` safe-reference constraints. |
| [crates/application/src/comfy_runtime/ports.rs:28](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/application/src/comfy_runtime/ports.rs:28) | +6/−1 | Pass `&UpdateTarget` into `ComfyRepository::fetch`. |
| [crates/application/src/comfy_runtime/update_workflow.rs:69](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/application/src/comfy_runtime/update_workflow.rs:69) | +7/−14 | Pass the target to fetch; reconstruct policy through `policy_target()` (:134). Intent + LatestCommit applies even at the frozen SHA on an attached branch (:143); retain detached-tag re-placement and later-phase safeguards. Existing pin checks, verification, exact-SHA checks, and actual-reference publication remain intact. |
| [crates/application/src/comfy_runtime/tests.rs:680](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/application/src/comfy_runtime/tests.rs:680) | +252/−4 | Update the fake fetch/freeze/apply ports (:427); record applied frozen targets and simulate detached placement. Add the three required policy/manifest/recovery tests (:680, :747, :811). |
| [crates/application/src/settings/diagnostics/tests.rs:175](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/application/src/settings/diagnostics/tests.rs:175) | +6/−1 | Update the diagnostics fake fetch signature for `&UpdateTarget`. |
| [crates/infrastructure/src/environment/source/placement.rs:33](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/environment/source/placement.rs:33) | +23/−4 | Extract `default_branch` from the existing validated symbolic-ref probe and `placement_with_default` (:51). Existing placement discovers then delegates; LatestCommit supplies the persisted branch. |
| [crates/infrastructure/src/comfy_runtime/git.rs:231](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/comfy_runtime/git.rs:231) | +56/−22 | LatestCommit fetches explicit origin branch refs; freeze (:255) records the validated default branch and exact origin tip. Apply (:330) validates the frozen target shape and passes the persisted branch into checkout (:92). Existing modes retain their fetch and FF/tag execution behavior. |
| [crates/infrastructure/src/comfy_runtime/git/placement_tests.rs:332](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/comfy_runtime/git/placement_tests.rs:332) | +210/−0 | Add all five required real-Git tests. Add `branch: None` to the existing tag fixture (:124); retain existing G2b/G2c tests. |
| [crates/infrastructure/src/comfy_runtime/sqlite.rs:175](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/comfy_runtime/sqlite.rs:175) | +47/−0 | Add optional defaulted/skipped `target_branch`, map both ways, retain document version 1 and existing phases. No SQL migration. Add new/legacy document coverage (:335). |
| [crates/desktop/src/ipc/comfy_dto.rs:379](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/desktop/src/ipc/comfy_dto.rs:379) | +33/−0 | Add `LatestCommit` wire variant/conversion (:388) and exact wire/domain roundtrip test (:552). Existing inputs, commands, results, ACL, and gateway implementations are unchanged. |
| [crates/desktop/src/state/recovery_tests.rs:170](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/desktop/src/state/recovery_tests.rs:170) | +1/−0 | Add `branch: None` to the existing frozen update constructor only. |
| [ui/src/shared/ipc/bindings.ts:1204](D:/vjump/vjump-comfyui-portable-with-resource-manager/ui/src/shared/ipc/bindings.ts:1204) | +1/−1 | Generated through `export_bindings`; `UpdateTargetDto` gains `{ kind: "latest_commit" }` only relative to the package baseline. |
| [ui/src/features/comfy-runtime/versions-panel.tsx:49](D:/vjump/vjump-comfyui-portable-with-resource-manager/ui/src/features/comfy-runtime/versions-panel.tsx:49) | +13/−0 | Add the exact confirmation target, latest-commit button (:168), and development helper (:191). Retain release/tag/current-branch FF controls and requirements/verification/stop confirmation text. |
| [ui/src/features/comfy-runtime/comfy-page.test.tsx:187](D:/vjump/vjump-comfyui-portable-with-resource-manager/ui/src/features/comfy-runtime/comfy-page.test.tsx:187) | +64/−0 | Add `offers_latest_commit_and_tag_updates`: cancellation/confirmation, exact new/tag request payloads, existing release/FF controls, and development helper. |
| [docs/DOCUMENTATION.md:299](D:/vjump/vjump-comfyui-portable-with-resource-manager/docs/DOCUMENTATION.md:299) | +1/−1 | Describe latest development commit, release/tag checkout, and current-branch FF. Replace the blanket fast-forward-only claim with mode-specific behavior. |
| [docs/REQUIREMENTS.md:310](D:/vjump/vjump-comfyui-portable-with-resource-manager/docs/REQUIREMENTS.md:310) | +1/−1 | FR-COMFY-6 adds the explicit development-head option and owner decision date. FR-COMFY-7 and section 2.4 remain unchanged. |

The new wire request is exactly:

```json
{"target":{"kind":"latest_commit"},"stopRunning":false,"retry":null}
```

LatestCommit alone fetches:

```text
git fetch --tags origin refs/heads/*:refs/remotes/origin/*
```

Freeze records `{ commit: C, tag: None, branch: Some(D), fast_forward: false }`. Apply uses persisted `D`, preserves unpublished branches/user branches through detached fallback, and retains `checkout -B D C --` plus origin upstream setup. Retry never fetches or freezes again. Manifest reference remains tag or the actual branch, including `None` after detached LatestCommit placement.

## Required tests and assertions

All 11 required Rust additions passed in the workspace run; the required UI addition passed in the full Vitest run. Git fixtures require an installed Git executable and fail if it is unavailable; these tests executed real Git commands in temporary directories, with no skip fallback.

| File:line / exact test name | Verified assertions |
|---|---|
| [crates/domain/src/comfy_runtime/update.rs:171](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/domain/src/comfy_runtime/update.rs:171) — `latest_commit_update_has_the_tag_refusals` | Dirty, unfinished operation, and divergence are refused, matching tag refusals; clean detached and attached/no-upstream checkouts are allowed. |
| [crates/application/src/comfy_runtime/tests.rs:680](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/application/src/comfy_runtime/tests.rs:680) — `frozen_update_policy_rejects_invalid_target_shapes` | All three valid policy shapes map correctly; every remaining boolean/ref shape and invalid safe-reference input fails with InvalidRecord. |
| [crates/application/src/comfy_runtime/tests.rs:747](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/application/src/comfy_runtime/tests.rs:747) — `latest_commit_update_records_the_actual_reference` | Completion publishes frozen SHA and actual branch; detached fallback publishes no branch. Constraint file/protected pins/no source builds/requirements reinstall/final verification are checked. Protected preview changes and installed pin drift fail before verification/publication. |
| [crates/application/src/comfy_runtime/tests.rs:811](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/application/src/comfy_runtime/tests.rs:811) — `latest_commit_retries_the_frozen_target` | Intent before apply, Intent after apply, same-SHA attached release, SourceChanged, Dependencies, Verifying, Publishing; exact persisted SHA/branch; no retry fetch/freeze; later phases do not re-place; unrelated HEAD refused; pending failure blocks Start; successful retry verifies the environment. |
| [crates/infrastructure/src/comfy_runtime/git/placement_tests.rs:332](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/comfy_runtime/git/placement_tests.rs:332) — `latest_commit_update_moves_a_release_branch_install_to_the_default_branch_head` | Fixture default main; frozen SHA/branch and non-FF/tag shape; correct HEAD, branch, upstream; preserved release pointer; later real pull --ff-only reaches a newly published development commit. |
| [crates/infrastructure/src/comfy_runtime/git/placement_tests.rs:371](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/comfy_runtime/git/placement_tests.rs:371) — `latest_commit_update_with_unpublished_default_branch_commits_is_detached` | Both unpublished default-branch and current user-branch cases reach the target detached; unpublished branch SHA and clean working tree are preserved. |
| [crates/infrastructure/src/comfy_runtime/git/placement_tests.rs:403](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/comfy_runtime/git/placement_tests.rs:403) — `latest_commit_apply_preserves_the_frozen_commit_and_branch` | Advance origin/main and change origin/HEAD after freeze; apply still uses the frozen SHA/main and origin/main upstream, preserving the release branch. |
| [crates/infrastructure/src/comfy_runtime/git/placement_tests.rs:453](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/comfy_runtime/git/placement_tests.rs:453) — `latest_commit_fetches_origin_even_when_current_upstream_is_elsewhere` | Current upstream is elsewhere/main and origin has a restricted release-only fetch refspec; LatestCommit refreshes/freezes origin/main while elsewhere/main stays old. |
| [crates/infrastructure/src/comfy_runtime/git/placement_tests.rs:501](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/comfy_runtime/git/placement_tests.rs:501) — `latest_commit_without_a_default_branch_fails_before_checkout` | Missing default yields exactly "The remote has no default branch."; missing/unresolvable default tip also fails; HEAD and local branch pointers stay unchanged. |
| [crates/infrastructure/src/comfy_runtime/sqlite.rs:335](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/comfy_runtime/sqlite.rs:335) — `update_documents_preserve_latest_commit_and_read_legacy_v1` | New target_branch roundtrips through SQLite; version remains 1; legacy tag and FF JSON without target_branch reads; None is omitted on serialization. |
| [crates/desktop/src/ipc/comfy_dto.rs:552](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/desktop/src/ipc/comfy_dto.rs:552) — `latest_commit_update_target_roundtrips` | Exact latest_commit wire spelling and full input JSON roundtrip; domain conversion; existing latest_release/tag/branch_ff_only serialization and conversion remain valid. |
| [ui/src/features/comfy-runtime/comfy-page.test.tsx:187](D:/vjump/vjump-comfyui-portable-with-resource-manager/ui/src/features/comfy-runtime/comfy-page.test.tsx:187) — `offers_latest_commit_and_tag_updates` | Development helper visible; release and FF actions remain; no gateway call before confirmation or after cancellation; exact latest_commit and selected-tag payloads; confirmation retains requirements/verification wording. |

## Command results

Final runs use default `1.99.0-x86_64-pc-windows-msvc` and default `target/`, with no RUSTUP_TOOLCHAIN/CARGO_TARGET_DIR override. `pnpm --version` reported `10.24.0`. Cargo commands ran sequentially; build/test commands use `-j 6`, including the bindings commands. Vitest used `VITEST_MAX_WORKERS=2`.

Every final command below exited 0. Test counts are aggregated across the binaries/doc-test suites invoked by that command; all measured counts are 0.

| Command | Exact result |
|---|---|
| `cargo fmt --all -- --check` | 0 formatting differences |
| `cargo clippy --workspace --all-targets --locked -j 6 -- -D warnings` | 0 warnings, 0 errors |
| `cargo test -p domain --locked -j 6 latest_commit` | 1 passed, 0 failed, 0 ignored, 47 filtered out |
| `cargo test -p application --locked -j 6 latest_commit` | 2 passed, 0 failed, 0 ignored, 209 filtered out |
| `cargo test -p infrastructure --locked -j 6 latest_commit` | 6 passed, 0 failed, 0 ignored, 453 filtered out |
| `cargo test -p infrastructure --locked -j 6 update_documents_preserve_latest_commit_and_read_legacy_v1` | 1 passed, 0 failed, 0 ignored, 458 filtered out |
| `cargo test -p desktop --locked -j 6 --lib latest_commit_update_target_roundtrips` | 1 passed, 0 failed, 0 ignored, 54 filtered out |
| `cargo test -p desktop --test bindings --locked -j 6` | 1 passed, 0 failed, 1 ignored, 0 filtered out; bindings_up_to_date passed |
| `cargo test --workspace --locked -j 6 --no-fail-fast` | 781 passed, 0 failed, 10 ignored, 0 filtered out |
| `pnpm -C ui lint` | 0 diagnostics |
| `pnpm -C ui typecheck` | 0 type errors |
| `VITEST_MAX_WORKERS=2 pnpm -C ui test` | 30 files passed; 195 tests passed, 0 failed, 0 skipped |
| `pnpm -C ui build` | Production build passed; 2,068 modules transformed; 1 chunk-size warning |
| `node scripts/check-arch-deps.mjs` | Architecture dependencies OK; 0 violations |

Infrastructure latest_commit totals include 443 unit-test filters + 1 real-setup filter + 9 workflow filters = 453. The persistence-only command filters 448 + 1 + 9 = 458. Workspace totals are 781 passed, 0 failed, 10 ignored, 0 filtered out over 16 result suites.

Additional development commands/results:

| Command / run | Result |
|---|---|
| `cargo fmt --all` (3 runs, last immediately after the final Rust edit) | All exited 0 |
| `cargo test -p desktop --test bindings --locked -j 6 -- --ignored export_bindings` | Exit 0; 1 passed, 0 failed, 0 ignored, 1 filtered out; generated bindings |
| First `cargo test -p application --locked -j 6 latest_commit` | Exit 101; 0 tests executed; one E0425 missing UpdateTarget name in the diagnostics fake. Fixed with the qualified domain type; final run passed |
| First `cargo test -p infrastructure --locked -j 6 latest_commit` | Exit 0; 6 passed, 0 failed, 0 ignored, 453 filtered out |
| First `cargo fmt --all -- --check` | Exit 0; 0 formatting differences |
| First `cargo clippy --workspace --all-targets --locked -j 6 -- -D warnings` | Exit 101; one test-only cloned_ref_to_slice_refs error. Replaced clone with std::slice::from_ref, formatted, then reran the full Rust check list successfully |
| `git diff --check` | Exit 0; 0 whitespace errors |
| Preservation assertions | 4 passed: FR-COMFY-7, section 2.4, wizard implementation, wizard tests unchanged from baseline |

Workspace ignored tests (existing explicit ignores): `export_bindings` (run explicitly above), `fresh_root_starts_and_closes_cleanly`, `shared_bridge_child`, `shared_bridge_grandchild`, `a_real_interpreter_is_probed`, `real_tools_are_provisioned`, `real_hf_xet_obeys_relay_with_direct_egress_denied`, `real_keyring_round_trip`, `dll_lookup_child`, and `real_first_run_setup`. Helper ignores are invoked by their parent tests as defined by the existing suite.

Raw command logs and structured result summaries are retained locally at `C:/Users/vjump/AppData/Local/Temp/vjump-s6-u-hx7n7fxj/` (`rust-results.json`, `ui-results.json`, and per-command logs).

## Preservation, deviations, and limits

- No implementation-plan deviation or unresolved specification gap. No wizard, SQL migration, command, ACL, gateway implementation, or result-field additions.
- Starting snapshot contained 708 files. Of these, 690 remain byte-identical, 17 contain this package's changes, and `docs/handoff/evidence/s6-native-register.md` changed independently during the session. That unrelated edit was not written or reverted by this package. Existing session-6 changes remain preserved.
- No repository git write commands were run. Git writes occur only inside temporary test fixtures. The forbidden installation paths were not accessed or modified by package work, and no vjumpkung-comfyui-desktop process was stopped.
- Native installed-app update/Start/Stop validation from the advisor's post-plan notes was not run: the owner explicitly forbids touching its installation paths in this task. Automated completion does not claim a native installed-environment update.
- Vite reports one >500 kB chunk warning (main JavaScript chunk 716.99 kB). Build succeeds; bundle restructuring is outside this mechanical package.
