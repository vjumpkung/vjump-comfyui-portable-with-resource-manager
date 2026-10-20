# Session 6 D1 implementation — partial, blocked on item 3

Date: 2026-10-20. Working copy: `D:\vjump\vjump-comfyui-portable-with-resource-manager`, branch `feat/m0-skeleton`, HEAD `e384d43`.

**D1 is not complete; residual 6 remains open before consent.** The independent changes below are implemented and checked. Item 3 was stopped under the coder instruction to report code conflicts instead of improvising. No advisor amendment was received during this run.

The goal was the approved D1 invariant: before exact-source consent on Windows, newly acquired node content must not be a tool's working directory while its top level contains an entry with a native-image extension; uv's interpreter query must not import Python modules from that folder. Installed and user-placed nodes retain the documented residual. Success required the specified refusal/cleanup tests, pinned-uv isolation evidence, and the final workspace checks.

## Blocking gap and deferred work

`crates/infrastructure/src/custom_nodes/git.rs:355` defines `read(...) -> Result<String, NodeAppError>`. It calls `run`, which returns the shared runner's processed `Output.stdout`. In `crates/infrastructure/src/shared/process.rs:267`, the collector uses `String::from_utf8_lossy`; it also redacts and bounds lines, and at `:300`/`:301` reconstructs stdout with an appended newline. The line assembler additionally handles CR/LF as line boundaries. This is not a raw, complete NUL-delimited byte stream.

Consequently, the prescribed `self.read(... ls-tree --full-tree -z --name-only ...)` cannot reliably reject invalid UTF-8 names or validate the exact original names. Redaction can also change a valid filename containing a credential-shaped value. Splitting this processed string on NUL or checking replacement characters would not implement the approved contract. No workaround or raw-output API redesign was introduced. An advisor-approved raw-output contract is needed before item 3 can continue.

Deferred with that item:

- The Windows pre-checkout `ls-tree` admission check and its specified error mappings.
- `git_install_refuses_root_native_image_before_checkout` and `git_update_refuses_root_native_image_in_frozen_target_before_checkout`. They were not added or counted as skipped/passing tests. Pre-checkout refusal, Git refusal cleanup, and unchanged live-update digest are not established by this package.
- The requested `docs/DOCUMENTATION.md` native-library row, because its claim that Git sources are checked before checkout would be false. That file's pre-existing session-6 changes were preserved.
- Changing residual row 6 to "Closed before consent (D1); residual after consent and for installed/user-placed nodes". Its existing status was preserved because closure is not established.

The registry tests use the existing infrastructure adapter fixture: they assert `prepare` returns the exact `Failed` refusal and removes staging/ZIP/hoist. They do not add a full `NodeService` package-preview recorder. The inspected workflow returns on adapter preparation failure before reaching package preview (`crates/application/src/custom_nodes/workflow.rs:256` and `:301`), but a recorded end-to-end "zero preview calls" assertion remains unimplemented.

## Per-file changes and key diffs

| File:line | Change |
|---|---|
| `crates/infrastructure/src/custom_nodes/tree.rs:23` | Added the exact `ROOT_NATIVE_MESSAGE`; `is_native_image_name` checks the last extension with Unicode `to_uppercase()` against DLL/EXE/COM/CPL/DRV/OCX/SCR/SYS. `refuse_root_native` reuses the bounded, validated top-level names from `frame`, includes files and directories, returns `Failed` on Windows, and does nothing elsewhere. The name test at `:454` includes `x.ſcr`, `.dll`, and all requested nonmatches. |
| `crates/infrastructure/src/custom_nodes/files.rs:385` | Added the archive refusal after single-root hoisting and tree validation, before the prepared result can reach preview. Added defense-in-depth checks after fresh Git validation at `:480` and `:614`; these do not close the missing pre-checkout boundary. |
| `crates/infrastructure/src/custom_nodes/git.rs:237` | Documented why `GIT_CONFIG_NOSYSTEM=1` is load-bearing: MinGit's required LFS process filter could otherwise execute during checkout. Item 3's functional change is deferred. |
| `crates/infrastructure/src/shared/process.rs:46` | Added a once-cached executable-parent default cwd. At `:493`, every command receives either its explicit cwd or that default; failure to determine the default becomes `RunError::Spawn`. No inherited-cwd fallback was added. |
| `crates/infrastructure/src/environment/packages.rs:107` | Unguarded preflight now uses `default_working_directory()`, matching the runner's default. Guarded node-directory semantics are unchanged. |
| `crates/infrastructure/src/shared/process/windows.rs:27` | Corrected the DLL-policy comment to say it covers this process only and is not passed to children (residual 6). |
| `crates/infrastructure/src/shared/process/tests.rs:674` | Renamed the DLL test; helper at `:741` now requires exactly error 193 before initialization and 126 after. Added the default-cwd parent test at `:694` and its ignored child at `:716`, which prints its cwd. |
| `crates/infrastructure/src/custom_nodes/tests.rs:1195` | Added archive root-file/root-directory refusal and cleanup, hoisted-root refusal (`:1234`), nested DLL acceptance (`:1266`), Git nested DLL/root-script acceptance (`:3161`), existing-node scan/status (`:3175`), Git environment policy (`:3200`), and the live Python-import trap test (`:5549`). |
| `README.md:170` | Added the exact requested Windows top-level native-image refusal and post-consent/non-sandbox security note. |
| `docs/handoff/evidence/s3-residuals.md:32` | Appended rows 26–28: executable-directory cwd closure, unchanged latent setup IPC source-build acceptance, and the unverified Git UnsafeSource/staging/reconcile cleanup residual. Row 6 was not marked closed. |
| `docs/handoff/plans/m5-h-hardening-approved-plan.md:161` | Appended the dated correction replacing the false propagation premise with D1's invariant. Added the dated test-premise correction at `:193`; historical text remains visible with the corrections. |
| `docs/handoff/evidence/s6-d1-implementation.md:1` | This report records the implemented subset, precise gaps, commands, counts, and limitations. |

The Git environment test records real clone, `-C`, config-preflight, and `ls-tree` specs. It checks `GIT_CONFIG_NOSYSTEM=1`, an absolute, empty retained app config for `GIT_CONFIG_GLOBAL`, and absence of `GIT_CONFIG_SYSTEM`, `GIT_CONFIG_PARAMETERS`, `GIT_CONFIG_COUNT`, `GIT_DIR`, and `GIT_WORK_TREE`. Because item 3 is blocked, this test explicitly invokes `ls-tree` on the already staged safe fixture; it is environment-policy evidence, not evidence of a pre-checkout admission check.

## Pinned uv and stop condition

The initial PATH contained uv 0.12.13. For acceptance, uv was downloaded into a temporary test directory from the exact Windows artifact URL in `pins.rs`. Its ZIP SHA-256 matched `3b2661a463a5baea7f5076ea2b25ab71d77f36a6c0d809e5350651792f80a672` before extraction. Only the test command's PATH was prefixed; no tool installation, RUSTUP_TOOLCHAIN, or CARGO_TARGET_DIR setting was changed.

Printed version: **`uv 0.13.1 (62faed335 2026-10-13 x86_64-pc-windows-msvc)`**.

The isolation test actually ran, without skipping, using a new fixture root and fresh app-owned UV cache, a pip-less venv, a local wheel with no dependency resolution, guarded requirements, and root `json.py`, `platform.py`, and `sysconfig.py` traps. Preview returned success and none of `<root>/trap-json`, `<root>/trap-platform`, or `<root>/trap-sysconfig` existed. The pinned-uv stop condition did not fire. The unchanged editable-backend and relative-include/local-path fixtures also passed in the 85-test custom-node run with pinned uv.

Git was available and executed: **`git version 2.55.0.windows.3`**. The new Git tests require the real binary rather than silently skipping it.

## Final acceptance command results

Commands ran from the repository root with the pinned default MSVC toolchain and default `target/`. Cargo builds used `-j 6`; no two cargo commands ran concurrently. The uv isolation command and workspace test command used the verified uv 0.13.1 first on PATH.

| Command | Exit | Exact result |
|---|---:|---|
| `cargo fmt --all -- --check` | 0 | No formatting differences. |
| `cargo clippy --workspace --all-targets --locked -j 6 -- -D warnings` | 0 | No warnings/errors. |
| `cargo test -p infrastructure --locked -j 6 root_native` | 0 | **4 passed, 0 failed, 0 ignored**, 458 unit tests filtered out; integration targets ran 0 tests (1 and 9 filtered out). |
| `cargo test -p infrastructure --locked -j 6 git_specs_always_disable_system_and_global_config` | 0 | **1 passed, 0 failed, 0 ignored**, 461 unit tests filtered out; integration targets ran 0 tests (1 and 9 filtered out). |
| `cargo test -p infrastructure --locked -j 6 node_preview_never_imports_python_modules_from_node_directory -- --nocapture` | 0 | **1 passed, 0 failed, 0 ignored**, 461 unit tests filtered out; actually ran with printed uv 0.13.1 above; integration targets ran 0 tests (1 and 9 filtered out). |
| `cargo test --workspace --locked -j 6 --no-fail-fast` | 0 | **792 passed, 0 failed, 12 ignored**, 0 filtered out, across unit/integration targets; all four doc-test targets ran 0 tests successfully. |
| `node scripts/check-arch-deps.mjs` | 0 | `Architecture dependencies: OK`. |

Workspace breakdown:

| Target | Passed | Failed | Ignored |
|---|---:|---:|---:|
| application unit | 211 | 0 | 0 |
| desktop lib unit | 55 | 0 | 0 |
| desktop main unit | 0 | 0 | 0 |
| desktop ACL | 11 | 0 | 0 |
| desktop bindings | 1 | 0 | 1 |
| desktop navigation | 2 | 0 | 0 |
| desktop stack reserve | 2 | 0 | 0 |
| desktop startup smoke | 0 | 0 | 1 |
| domain unit | 48 | 0 | 0 |
| infrastructure unit | 453 | 0 | 9 |
| infrastructure real setup | 0 | 0 | 1 |
| infrastructure workflow | 9 | 0 | 0 |
| Total | **792** | **0** | **12** |

## Iteration and additional verification

- `cargo fmt --all`: two successful runs, exit 0. Edited files were normalized to LF.
- Initial `root_native` run: exit 0, 4 passed / 0 failed / 0 ignored, 458 unit tests filtered out. Repeated at final acceptance with the same result.
- Initial Git environment-policy run: exit 0, 1 passed / 0 failed / 0 ignored, 461 unit tests filtered out. Repeated at final acceptance with the same result.
- Initial isolation-test run on system uv **0.12.13 (0ebbd9274 2026-09-10 x86_64-pc-windows-msvc)**: exit 1, 0 passed / 1 failed / 0 ignored, 461 unit tests filtered out. Preview had succeeded and all trap checks had passed; an extra assertion I added about the parsed package inventory failed. That unrequested format-dependent assertion was removed. No production workaround or trap suppression was added.
- First corrected isolation-test run with verified uv 0.13.1: exit 0, 1 passed / 0 failed / 0 ignored, 461 unit tests filtered out. Repeated at final acceptance with the same result.
- `cargo test -p infrastructure --locked -j 6 shared::process::tests`: exit 0, **24 passed / 0 failed / 2 ignored**, 436 unit tests filtered out. The two ignored helper definitions actually execute in subprocesses of their parent tests; DLL initialization observed exactly 193 then 126, and the cwd helper printed the executable directory. Integration targets ran 0 tests, with 1 and 9 filtered out.
- `cargo test -p infrastructure --locked -j 6 custom_nodes::tests -- --nocapture` with verified uv 0.13.1: exit 0, **85 passed / 0 failed / 0 ignored**, 377 unit tests filtered out. Integration targets ran 0 tests, with 1 and 9 filtered out. Both uv-version messages identified 0.13.1. This includes the unchanged editable-backend and relative-includes/local-paths fixtures.
- Scoped `git diff --check`: exit 0, no whitespace errors. No Git history/index operations were performed.
- Final line-ending check: exit 0; all 12 D1-edited/created files contain LF line endings and a final newline.
- Preservation review: the 80 pre-existing tracked-file diffs outside D1's scope exactly match the verified cumulative D4 patch. A first comparison incorrectly treated patch-added/untracked files as missing from `git diff`; a second also mishandled a file without a final newline. Content-aware review confirms 41 of 42 baseline-added files match, respecting that newline marker. `s6-native-register.md` has additional content relative to the cumulative patch; it was not edited by D1 and was left intact. No prior session-6 work was reverted.

No native Linux checks were run. The two blocked Git refusal tests and the end-to-end zero-preview-call assertion remain missing evidence despite the passing final checks. This report does not claim pre-consent release closure.

## Completion addendum — package N, 2026-10-20

The historical partial results above remain unchanged. The advisor's corrected ruling in `s6-advisor-n.md` supplied the missing raw-output contract, and package N completed D1 item 3. See `s6-n-implementation.md` for the complete per-file report and command results.

- `shared/process.rs:532` now captures exact stdout bytes through the existing process lifecycle, without decoding, redaction, line assembly, sink/observer forwarding or diagnostic-tail insertion. Capture overflow, read failure and incomplete drain fail closed. The two required raw tests passed, and the workspace run also passed raw cancellation, timeout and retained-cleanup coverage.
- `custom_nodes/git.rs:423` shares metadata/spec/configuration preflight for raw reads, uses the caller's cancellation token, a 120-second timeout and an 8 MiB cap. Strict Windows cloning lists `ls-tree --full-tree -z --name-only <resolved.commit>` with the staged directory's `-C` after commit validation and before either checkout branch. Exact NUL framing, UTF-8 names, `SafeFileName` and the existing entry bound are validated before native-name classification.
- Both Git failure paths in `custom_nodes/files.rs:515` and `:628` release the staging-bound Git owner and directory before identity-checked cleanup. Existing UnsafeSource preservation and survivor retention remain in place.
- `git_install_refuses_root_native_image_before_checkout` and `git_update_refuses_root_native_image_in_frozen_target_before_checkout` actually executed on Windows with Git 2.55.0.windows.3. Both pass for a native root file and a native-suffixed directory. They assert the exact refusal, resolved/frozen commit listing, absence of staging checkout/publication, and removal of owned staging. The update test also proves the live HEAD and content digest remain unchanged.
- Final `root_native`: **6 passed, 0 failed, 0 ignored** (462 unit tests filtered out); `git_root_tree_admission`: **1 passed, 0 failed, 0 ignored** (467 filtered out); Git environment/raw-path policy: **1 passed, 0 failed, 0 ignored** (467 filtered out). Each filtered infrastructure command also ran zero integration tests, with 1 and 9 filtered out.
- With `D:\vjc-s6\r1\tools\uv` first on PATH, `node_preview_never_imports_python_modules_from_node_directory -- --nocapture` actually ran: **1 passed, 0 failed, 0 ignored** (467 filtered out), printing **`uv 0.13.1 (62faed335 2026-10-13 x86_64-pc-windows-msvc)`**. The permitted tool directory was used read-only.
- Final workspace checks: fmt and clippy pass; Rust **802 passed, 0 failed, 12 ignored**; frontend lint/typecheck pass, Vitest **30 files / 202 tests passed**; architecture dependencies OK.

Residual row 6 is therefore now marked **Closed before consent (D1); residual after consent and for installed/user-placed nodes**. The exception for selected curated Git sources with persisted first-run consent is explicitly post-consent. Row 28 remains open: these native-refusal cleanup tests do not settle the broader UnsafeSource/reconcile staging question. The adapter-level refusal still returns before the application package-preview call; this package does not add a separate end-to-end preview-recorder test for archive refusal. No native Linux or real GPU setup/launch acceptance was run.
