# Session 6 N implementation — verified

Date: 2026-10-20. Working copy: `D:\vjump\vjump-comfyui-portable-with-resource-manager`, branch `feat/m0-skeleton`, HEAD `e384d43415087d670d5f7e76647e3801c3d87163`.

Implemented the corrected advisor ruling `s6-advisor-n.md`, sections 1–5 and its required test table. The goal was the persisted-consent exception for selected curated Git nodes, completion of D1's strict Windows pre-checkout admission boundary, and approved-hook-before-requirements execution throughout the shared node workflow. Success required the exact named assertions, pinned-uv isolation evidence and every requested final command. No product decisions were reopened and no implementation items remain blocked.

## Per-file changes and key diffs

References below describe this package's changes, preserving the earlier session-6 work in the same files.

| File:line | Change |
|---|---|
| `crates/application/src/custom_nodes/model.rs:326` | Added internal `NodePreparePolicy::{Strict, ConsentedFirstRun}`; `PreparedPlan` retains `prepare_policy` at line 338. No IPC/DTO switch was added. |
| `crates/application/src/custom_nodes/ports.rs:69` | Added policy to `NodeFiles::prepare`, after `prior` and before `ctx`. |
| `crates/application/src/custom_nodes/setup.rs:53` | Derives the exception only from persisted FirstRun consent and a selected normalized catalog repository with no requested reference. Invalid consented requests fail before adapter preparation. Revalidates authorization at line 78; setup commit at line 201 permits the hook with persisted consent and clears remembered trust/source-build permissions. The shared commit workflow rejects protected-package authorization. |
| `crates/application/src/custom_nodes/workflow.rs:46` | Factors protected-preview validation; derives/stores policy at line 175 and revalidates relaxed plans before publication at line 551. `has_install_py` is exposed for consented setup only when execution is applicable. `dependencies` at line 809 freezes validated requirements before the hook, runs hook first, reasserts pins with a fresh cleanup token, restores profile constraints, previews the identical frozen spec, installs requirements, reasserts pins and performs final verification/provenance/containment validation. Stable markers are at lines 873, 892 and 905. Skipped-hook warning at line 26 uses the exact approved text. |
| `crates/application/src/custom_nodes/tests.rs:1170` | Updates hook order/trust coverage; records adapter policy in the fake; adds hook failure/cancellation (1977), profile constraint restoration (2030), frozen snapshot/version/post-hook preview (2066), live guard order (2174), persisted setup authorization/revocation (3247), and current-review auto consent/full source/warning/no-note assertions (4343). Retains final digest persistence at line 2263 and non-consented note/retry tests. |
| `crates/application/src/environment/execute.rs:1154` | Synthesizes the decision for every current review, replacing older covering decisions that omit the hook. Logs full repository and commit immediately before commit at line 1207 and at preparation at line 1347. Skipped-hook notes depend on warning presence at line 1360; decision uses `review.has_install_py` at line 1587. |
| `crates/application/src/environment/tests/fakes.rs:215` | Records preview specs and profile constraint extras without retaining execution owners, for frozen-input and constraint/order assertions. |
| `crates/infrastructure/src/shared/process.rs:243` | Adds `RawOutputError::{Run, LimitExceeded, Incomplete}`, bounded exact-byte capture, raw reader (460), drain completion (478), and `run_raw_stdout` (532). Reuses `run_inner` process management with an optional raw collector. Raw stdout bypasses decoding, redaction, line assembly, sinks, tails and observers. Overflow drains/discards and fails; read/drain failure fails closed. Text API signatures/callers remain unchanged. |
| `crates/infrastructure/src/shared/process/tests.rs:135` | Adds both exact raw-output tests, deterministic chunk/read-error/unfinished-drain fixtures, exact-cap/overflow and diagnostic exclusion assertions. Existing timeout (478), cancellation (560) and retained-cleanup coverage now exercise raw and text modes. Windows byte-output fixture uses a temporary cwd and ASCII basename to avoid shell quoting ambiguity. |
| `crates/infrastructure/src/custom_nodes/git.rs:121` | Parses exact NUL-delimited names with complete framing, UTF-8, SafeFileName and existing entry-bound checks; native classification uses the shared Unicode-aware extension function. `checked_spec` (317) shares metadata/spec/local-config preflight. `read_raw` (423) uses caller cancellation, no StepSink, 120 seconds and 8 MiB. Strict Windows clone lists the resolved commit after cat-file and before checkout (602); consented setup skips only this admission listing. Cancellation/offline classification and prescribed failure mappings are retained. |
| `crates/infrastructure/src/custom_nodes/files.rs:420` | Threads install policy to clone and fresh-tree validation; findings become staged warnings. Updates/registry paths remain strict. Both Git failure paths release bound Git and directory owners before identity-checked `remove_owned` at lines 515 and 628; UnsafeSource preservation/survivor retention remain unchanged. |
| `crates/infrastructure/src/custom_nodes/tree.rs:24` | Adds the two exact warning constants. Shared root-name inspection (44) backs both refusal and finding behavior. The complete bounded non-following walker records shipped cache, including empty cache directories. Policy-aware fresh-Git validation (220) deduplicates findings; strict refusals and every containment/resource/name/provenance boundary remain. |
| `crates/infrastructure/src/custom_nodes/tests.rs:3255` | Adds exact tree parser coverage, Windows install admission/cleanup (3350), Windows frozen-update admission/unchanged-live-state (3386), consented content/strict equivalence/containment (3423), and production/raw-path Git environment/order assertions (3544). Refusal specs are recorded through `TEST_RELEASE_OWNERS` without keeping staging pinned during cleanup. Existing nested-native/root-script/installed-node tests remain. All 37 older adapter prepare calls receive strict policy. |
| `crates/infrastructure/src/environment/data/curated_nodes.json:63` | Removes only PlagueKind's unavailable field; catalog order/schema/defaults and future-unavailability mechanism remain. |
| `crates/infrastructure/src/environment/curated/tests.rs:122` | Renames the catalog test to `bundled_catalog_has_no_unavailable_entries`; asserts all 22 available and PlagueKind at index 19. |
| `ui/src/features/setup/setup-wizard.tsx:59` | Review disclosure and options disclosure (823) cover selected-node cache/native exceptions, hook-before-reviewed-requirements, default-branch head and one setup confirmation. Manager's separate path is not described as running a hook. |
| `ui/src/features/setup/setup-wizard.test.tsx:390` | Adds selectable PlagueKind/sent-consent test and updates exact review-disclosure coverage at line 427. Unticked curated defaults and future-unavailability behavior remain covered. |
| `ui/src/features/custom-nodes/review-dialog.tsx:88` | Approved-hook label explicitly says before requirements.txt; all consent controls/defaults remain. |
| `ui/src/features/custom-nodes/review-dialog.test.tsx:82` | Adds exact approved-hook disclosure test and verifies unchecked defaults. |
| `README.md:169` | Documents setup exceptions, strict Windows pre-checkout refusal, shared hook order, post-hook frozen validation and wheel-only app-managed installs. |
| `docs/DOCUMENTATION.md:150` | Corrects setup behavior, Custom Nodes/dependencies (440/463), and Windows native-library boundary (1006), including installed/user-placed and post-consent residuals. |
| `docs/REQUIREMENTS.md:285` | Updates FR-SETUP-9; FR-NODE-4 (379) and NFR-SEC-3 (475) specify the exact exception, shared order, pins/profile constraints, frozen post-hook validation and wheel-only behavior. |
| `docs/handoff/plans/s6-n-setup-node-policy.md:3` | Records the authoritative corrected plan; historical draft is explicitly superseded. |
| `docs/handoff/evidence/s6-advisor-rulings.md:84` | Records corrected N scope, exact findings, raw contract, execution/validation order and conditional D1 closure. |
| `docs/handoff/evidence/s3-residuals.md:12` | Row 6 receives only the approved pre-consent closure after both Windows admission/cleanup tests and pinned-uv isolation pass. Row 15 (21) distinguishes consented setup from the remaining skipped-hook path. Row 28 stays open. |
| `docs/handoff/evidence/s6-d1-implementation.md:100` | Appends completion evidence without rewriting historical partial results, blocked-item explanation or old counts. |
| `docs/handoff/evidence/s6-n-implementation.md:1` | This implementation/verification report. |

The exact warning texts are:

- `This repository ships __pycache__ content; consented first-run setup allows it.`
- `This node has top-level native-image entries that tools may load; consented first-run setup allows them.`

The skipped-hook warning is `This setup will not run this node’s install.py. If needed, reinstall it in Custom Nodes and approve install.py.` Successful consented installs do not create a skipped-hook note. App-managed resolution/install stays wheel-only; an approved arbitrary hook can run its own installers/builds and is not sandboxed.

## Final acceptance commands

All commands ran from the repository root, using the pinned default MSVC toolchain and default `target/`. No RUSTUP_TOOLCHAIN or CARGO_TARGET_DIR override was set. Cargo builds used `-j 6`, and no two Cargo commands ran concurrently. All final commands exited **0**.

| Command | Exact result |
|---|---|
| `cargo fmt --all -- --check` | Pass; no formatting differences. |
| `cargo clippy --workspace --all-targets --locked -j 6 -- -D warnings` | Pass; no warnings/errors. |
| `cargo test -p infrastructure --locked -j 6 raw_stdout` | **2 passed, 0 failed, 0 ignored**, 466 unit tests filtered out. |
| `cargo test -p infrastructure --locked -j 6 root_native` | **6 passed, 0 failed, 0 ignored**, 462 unit tests filtered out. |
| `cargo test -p infrastructure --locked -j 6 git_root_tree_admission` | **1 passed, 0 failed, 0 ignored**, 467 unit tests filtered out. |
| `cargo test -p infrastructure --locked -j 6 consented_setup_git_prepare` | **1 passed, 0 failed, 0 ignored**, 467 unit tests filtered out. |
| `cargo test -p application --locked -j 6 install_py` | **3 passed, 0 failed, 0 ignored**, 212 filtered out. |
| `cargo test -p application --locked -j 6 curated` | **25 passed, 0 failed, 0 ignored**, 190 filtered out. |
| `cargo test --workspace --locked -j 6 --no-fail-fast` | **802 passed, 0 failed, 12 ignored**, 0 filtered out; all four doc-test targets ran zero tests successfully. |
| `pnpm -C ui lint` | Pass; no lint diagnostics. |
| `pnpm -C ui typecheck` | Pass; no type diagnostics. |
| `VITEST_MAX_WORKERS=2 pnpm -C ui test` | **30 files passed; 202 tests passed; 0 failed, 0 skipped**. PowerShell set the environment variable before invoking pnpm. |
| `node scripts/check-arch-deps.mjs` | `Architecture dependencies: OK`. |

Every filtered infrastructure command above also ran zero tests in the integration targets, with 1 real-setup and 9 workflow tests filtered out.

Additional final checks:

| Command | Exact result |
|---|---|
| `cargo test -p infrastructure --locked -j 6 git_specs_always_disable_system_and_global_config` | Exit 0; **1 passed, 0 failed, 0 ignored**, 467 unit tests filtered out; integration targets 0 tests, 1 and 9 filtered out. |
| `cargo test -p infrastructure --locked -j 6 node_preview_never_imports_python_modules_from_node_directory -- --nocapture` with `D:\vjc-s6\r1\tools\uv` first on PATH | Exit 0; **1 passed, 0 failed, 0 ignored**, 467 unit tests filtered out; integration targets 0 tests, 1 and 9 filtered out. **Actually ran, did not skip.** Printed `preview tool: uv 0.13.1 (62faed335 2026-10-13 x86_64-pc-windows-msvc)`. Only read-only tool use of that folder occurred. |
| `git --version` | Exit 0; `git version 2.55.0.windows.3`. The new Git tests require the real binary and actually executed. |
| `git diff --check` | Exit 0; no whitespace errors. |
| Package LF/final-newline/trailing-whitespace byte check | Exit 0; **27 files passed**. |
| `git rev-parse --abbrev-ref HEAD`; `git rev-parse HEAD` | Exit 0; unchanged branch/HEAD shown at the top of this report. |

Final raw logs are in the system temporary directory under `vjump-s6-n-final-*.log`; they include all individual test names, counts and the printed uv version.

Workspace breakdown:

| Target | Passed | Failed | Ignored |
|---|---:|---:|---:|
| application unit | 215 | 0 | 0 |
| desktop lib unit | 55 | 0 | 0 |
| desktop main unit | 0 | 0 | 0 |
| desktop ACL | 11 | 0 | 0 |
| desktop bindings | 1 | 0 | 1 |
| desktop navigation | 2 | 0 | 0 |
| desktop stack reserve | 2 | 0 | 0 |
| desktop startup smoke | 0 | 0 | 1 |
| domain unit | 48 | 0 | 0 |
| infrastructure unit | 459 | 0 | 9 |
| infrastructure real setup | 0 | 0 | 1 |
| infrastructure workflow | 9 | 0 | 0 |
| Total | **802** | **0** | **12** |

Ignored definitions include subprocess helpers that execute through their parent tests, binding export, real interpreter/tool/keyring/HF tests, desktop startup smoke and the GPU/download-heavy real setup. No ignored real setup was silently counted as executed.

## Required assertions

Every exact name required by the advisor exists and passed in the final workspace/UI runs:

- `raw_stdout_preserves_exact_bytes`: byte-by-byte NUL, CR/LF, invalid UTF-8 and credential-shaped bytes; actual child capture; stdout absent from sink/tail/observer; stderr remains redacted in diagnostics.
- `raw_stdout_refuses_overflow_and_incomplete_capture`: exact cap succeeds; large overflow drains/discards and refuses; injected read failure and unfinished drain refuse. Existing timeout, cancellation and retained-resource cleanup tests also exercise raw mode.
- `git_root_tree_admission_validates_exact_nul_delimited_names`: empty tree, exact framing, invalid/empty/UTF-8/SafeFileName records, entry bound, Unicode native extension and credential-shaped native filename.
- `git_install_refuses_root_native_image_before_checkout` and `git_update_refuses_root_native_image_in_frozen_target_before_checkout`: each tests a native root file and native-suffixed directory, exact Failed message, listing of resolved/frozen commit after cat-file, no staging checkout/publication, owned staging removal; update preserves live HEAD and tree digest.
- `git_specs_always_disable_system_and_global_config`: exercises raw path and Windows production listing-before-checkout; checks allow-listed Git environment and retained configuration.
- `consented_setup_git_prepare_allows_content_findings_but_preserves_containment`: real Git cache/native preparations produce one deduplicated warning only under relaxed policy; strict equivalents refuse; excessive depth and actual Windows junctions remain refused under both policies. Existing nested-native, root-script and installed-node coverage passes.
- `setup_content_policy_requires_persisted_consent_and_selected_curated_source`: records strict Active/non-consented preparation, relaxed selected consented preparation, invalid selection/reference before adapter calls, and stale commit on consent/selection revocation.
- `install_py_runs_only_with_consent_before_requirements_and_trust_is_namespaced`: hook precedes actual node install; trust behavior remains unchanged.
- `hook_failure_skips_requirements_and_retains_repair_state`: both hook failure and cancellation run fresh-token pin cleanup before guard release/failure return, skip node requirements and retain failure/repair state.
- `hook_pin_repair_restores_profile_constraints_before_requirements`: repair's pins-only constraints are followed by restored profile extras before post-hook preview and requirements.
- `requirements_after_hook_use_reviewed_snapshot_and_frozen_versions`: post-hook preview/install share guarded bytes and exact versions despite environment mutation; protected drift/preflight refusal prevents install. Invalid frozen versions fail before hook; absence of reviewed requirements prevents discovering hook-created inputs.
- `node_execution_guard_spans_preview_hook_install_and_cleanup` and retained `consented_hook_final_digest_is_saved_before_verified`: new order/cleanup lifetime and final digest persistence.
- `curated_auto_consent_installs_every_node_and_logs_warnings`: hooks iff present, hook before requirements, no consent pause, current decision replaces an older hook-omitting decision, findings acknowledged, full normalized repository/full commit logged, no skipped-hook note, no remembered trust/source builds. Non-consented skipped-hook and note-survives-retry coverage remains passing.
- `bundled_catalog_has_no_unavailable_entries`: all 22 available, PlagueKind index 19.
- `plaguekind_is_selectable_and_sent_with_review_consent`, `review_page_discloses_third_party_code_and_sends_consent`, `approved_hook_discloses_install_py_before_requirements`: disclosures, selectable catalog entry, sent consent and unchecked defaults pass.

## Iteration results, corrections and preservation

These were development checks, before the final passing acceptance sequence above:

| Command/check | Result and correction |
|---|---|
| `cargo test -p infrastructure --locked -j 6 raw_stdout`, attempts 1 and 2 | Both exit 101 before tests: three SafeFileName Display errors in new application log lines. Attempt 2 was an unnecessary early retry before the fix landed. Changed the log arguments to `.as_str()`. No tests counted as passed. |
| Same raw command, attempt 3 | Exit 101 before tests: one raw-path test call still supplied `Option<&Path>` after the method changed to `&Path`; corrected the argument. |
| Same raw command, attempt 4 | Exit 101; **0 passed, 2 failed, 0 ignored**, 466 filtered out. Deterministic byte capture passed, but Windows shell fixture quoting emitted empty stdout. Changed only the fixture to an ASCII basename in a temporary cwd; final raw and workspace runs pass. |
| `cargo test -p infrastructure --locked -j 6 root_native` | Exit 0; **6 passed, 0 failed, 0 ignored**, 462 unit tests filtered out; integration targets 0, 1/9 filtered out. |
| `cargo test -p infrastructure --locked -j 6 git_root_tree_admission` | Exit 0; **1 passed, 0 failed, 0 ignored**, 467 unit tests filtered out; integration targets 0, 1/9 filtered out. |
| `cargo test -p infrastructure --locked -j 6 consented_setup_git_prepare` | Exit 101; **0 passed, 1 failed, 0 ignored**, 467 filtered out. A thread-local injected read callback could not reach the blocking worker, so no link substitution occurred. Replaced the invalid fixture with an actual junction checked through the same policy-aware fresh-tree validation under both policies. Final policy and workspace runs pass. |
| `cargo test -p application --locked -j 6 custom_nodes::tests` | Exit 0; **65 passed, 0 failed, 0 ignored**, 150 filtered out. |
| `cargo fmt --all` | Exit 0 after the last Rust edit; final fmt check passes. |
| `VITEST_MAX_WORKERS=2 pnpm -C ui test src/features/setup/setup-wizard.test.tsx src/features/custom-nodes/review-dialog.test.tsx` | Initial run: **25 passed, 1 failed**, from a test expecting a nonexistent DTO field. Removed that test-only assertion. Corrected run: exit 0, **2 files / 26 tests passed**, zero failed/skipped. |
| Manual Windows PowerShell/.NET ProcessStartInfo byte fixture | Exit 0; **1 fixture passed** in a temporary path with spaces. `cmd.exe /d /c` with ASCII file basename emitted exact stdout `000D0AFF41`, expected stderr marker and exit 0. Temporary files removed; no production change. |
| Application scoped diff/line-ending review | Exit 0; no whitespace errors; all seven application files use LF. |
| Checkpoint content reconstruction/comparison | Initial comparison exited 1 on two outside-scope mismatches, `HANDOFF.md` and `s6-native-register.md`. Both were already present before N and were not edited here (last-write times 15:56:14 and 15:46:41). Of 137 checkpoint files, 115 matched, 20 had intended N changes, and these two pre-existing differences were left intact. D1 completion evidence was appended afterward. No checkpoint reconstruction was applied to disk. |

No plan deviations remain. The Windows link test fixture correction uses the exact policy-aware validation path rather than an ineffective cross-thread injection; this does not relax or replace production containment. The read-only checkpoint comparison does not claim those two pre-existing files equal the older checkpoint.

All edited/created package files were checked for LF line endings and final newlines. Existing session-6 changes were preserved; no Git index/history writes, resets, checkouts, stashes or fixture operations against this repository were performed. Git writes occurred only inside disposable test repositories. `D:\comfyui-test\` was not touched, `D:\vjc-s6\` was used only for the explicitly permitted read-only uv executable, and no desktop process was killed.

## Closure and remaining acceptance

D1 item 3 is complete. Residual row 6 is now **Closed before consent (D1); residual after consent and for installed/user-placed nodes** because the two Windows admission/cleanup tests and pinned-uv isolation ran and passed. Row 28 remains open. Consented first-run setup is already post-consent before tools run in selected curated node content.

No real NVIDIA FirstRun/ComfyUI launch or native UI/manual Custom Nodes acceptance was performed, and no Linux checks were run. The advisor's remaining main-machine acceptance is a fresh FirstRun with PlagueKind selected, plus an approved dual-file node install through Custom Nodes to capture hook/requirements order and unchanged pins. Hook failures due to requirements not yet being installed, arbitrary nontransactional hook effects and incompatible/unavailable repositories remain the authorized behavior/limitations described in the ruling.
