# Session 6 — D4 implementation and verification

Implemented the advisor-approved D4 plan on 2026-10-20 in `D:\vjump\vjump-comfyui-portable-with-resource-manager`, branch `feat/m0-skeleton`, HEAD `e384d43`. Used the pinned default MSVC toolchain and default `target/`, with `-j 6` for compilation/tests and no concurrent cargo commands. No Git commit, push, stash, reset, checkout, apply, clean or history changes were made. The protected installation roots were not accessed and desktop processes were not terminated.

The goal was to supply the isolated `hf` child with an app-owned home, record bounded and redacted diagnostics for otherwise-unclassified runtime failures, and retain code-only download errors. Success criteria were the five requested local checks plus the named tests and documentation changes. Native acceptance remains assigned to main.

| File:line | D4 change |
|---|---|
| `crates/infrastructure/src/models/acquisition/hf_engine.rs:62` | Creates `<root>/data/cache/hf/home` with `create_dir_all`; a creation failure maps to `DownloadErrorCode::Disk`. Sets both `USERPROFILE` and `HOME` to that directory on every platform. Adds no other child variables and does not copy the parent profile. |
| `crates/infrastructure/src/models/acquisition/hf_engine.rs:157` | Classifies `RunError::Failed` as before, and emits `tracing::warn!` only when the result is `ToolRuntime`. The returned download error still contains only its code. |
| `crates/infrastructure/src/models/acquisition/hf_engine.rs:233` | Adds `runtime_failure_summary`: selects the last 20 tail entries, applies the existing `logging::redact` before truncation, limits each line to 500 Unicode characters, and truncates the combined summary to at most 4,096 bytes at a UTF-8 boundary. Redacting first prevents a truncation boundary from exposing a partial registered secret. |
| `crates/infrastructure/src/models/acquisition/hf_engine.rs:386` | Adds `hf_spec_uses_an_app_owned_home`: the directory exists; both variables occur once, equal the private directory, and differ from the corresponding parent value. |
| `crates/infrastructure/src/models/acquisition/hf_engine.rs:407` | Adds `hf_runtime_failure_summary_is_bounded_and_redacted`: registered secret and `hf_` token redaction, last-20 selection, overlong Unicode lines, the 4 KiB bound, and a secret crossing the character limit. |
| `crates/infrastructure/src/models/acquisition/hf_engine.rs:439` | Adds ignored `real_hf_download_succeeds_without_user_profile_variables`. Uses `VJ_REAL_HF_EXE`, an isolated temporary root and `NoCredentials`; constructs the real `HfDownloadCommand` spec, substituting only the executable path to select the installed pinned tool. Asserts no `HOMEDRIVE`, `HOMEPATH` or `HF_TOKEN` in the child, runs with a 600-second timeout, then checks 334,641,190 bytes and the approved SHA-256 using the existing streaming file-digest helper. |
| `docs/DOCUMENTATION.md:350` | Adds the exact approved private-home/keyring-only credential paragraph to Downloads. |
| `docs/handoff/evidence/s3-residuals.md:31` | Appends next-free row 25 for the native `Path.home()` failure and missing diagnostics; marks D4 fixed with native evidence pending. |
| `docs/handoff/evidence/s6-d4-implementation.md:1` | Adds this implementation and verification report. |

The ignored live test fixes the source to `stabilityai/sd-vae-ft-mse-original@629b3ad3030ce36e15e70c5db7d91df0d60c627f:vae-ft-mse-840000-ema-pruned.safetensors`, expected SHA-256 `735e4c3a447a3255760d7f86845f09f937809baa529c17370d83e4c3758f3c75`. It does not mutate the parent environment. Other child-environment builders, UI errors and persisted job documents were not changed by D4. The Rust diff is +197/-2 lines in `hf_engine.rs`.

**Verification commands and exact results**

All commands ran from the repository root. The results below describe the final source state; earlier failed attempts are recorded immediately afterward.

| Command | Final result |
|---|---|
| `cargo fmt --all -- --check` | Exit 0; 0 formatting errors. Four invocations total, all passed. |
| `cargo clippy --workspace --all-targets --locked -j 6 -- -D warnings` | Exit 0; 0 warnings. Final run completed in 6.82 s. Four invocations total: two failed during development, then two passed (the last after the home-path correction). |
| `cargo test -p infrastructure --locked -j 6 hf_` | Exit 0; library: 11 passed, 0 failed, 2 ignored, 439 filtered out. `real_setup`: 0 run, 1 filtered out. `workflow`: 0 run, 9 filtered out. Aggregate: 11 passed, 0 failed, 2 ignored, 449 filtered out. Both new unit tests passed. Two invocations total. |
| `cargo test --workspace --locked -j 6 --no-fail-fast` | Exit 0; 783 passed, 0 failed, 11 ignored, 0 measured, 0 filtered out across 16 targets. One invocation. |
| `node scripts/check-arch-deps.mjs` | Exit 0; `Architecture dependencies: OK`, 0 reported violations. One invocation. |
| `rustfmt --edition 2024 crates/infrastructure/src/models/acquisition/hf_engine.rs` | Exit 0 on all three invocations; formatted only the scoped Rust file. |
| `git diff --check -- crates/infrastructure/src/models/acquisition/hf_engine.rs docs/DOCUMENTATION.md docs/handoff/evidence/s3-residuals.md docs/handoff/evidence/s6-d4-implementation.md` | Exit 0; 0 whitespace errors. |
| LF-only check of all four D4 files | Passed: 4 files, 0 carriage-return characters. |

| Workspace target | Passed | Failed | Ignored |
|---|---:|---:|---:|
| application library | 211 | 0 | 0 |
| desktop library | 55 | 0 | 0 |
| desktop main | 0 | 0 | 0 |
| desktop `acl` | 11 | 0 | 0 |
| desktop `bindings` | 1 | 0 | 1 |
| desktop `navigation` | 2 | 0 | 0 |
| desktop `stack_reserve` | 2 | 0 | 0 |
| desktop `startup_smoke` | 0 | 0 | 1 |
| domain library | 48 | 0 | 0 |
| infrastructure library | 444 | 0 | 8 |
| infrastructure `real_setup` | 0 | 0 | 1 |
| infrastructure `workflow` | 9 | 0 | 0 |
| application doc-tests | 0 | 0 | 0 |
| desktop doc-tests | 0 | 0 | 0 |
| domain doc-tests | 0 | 0 | 0 |
| infrastructure doc-tests | 0 | 0 | 0 |
| **Total** | **783** | **0** | **11** |

**Corrected check failures, deviations and remaining evidence**

No implementation scope deviations or unresolved plan gaps. Checks were repeated only after changes or failures requiring verification:

- First clippy attempt: exit 1, one `E0277` compile error in the ignored live test because this repository's `sha2` result does not implement `LowerHex`. Replaced the custom hash loop/formatter with the existing `shared::model_paths::digest` helper.
- Second clippy attempt: exit 1, one `cloned_ref_to_slice_refs` diagnostic promoted to an error by `-D warnings`. Replaced the test's one-element cloned slice with `std::slice::from_ref`.
- First filtered HF test attempt: exit 1; 10 passed, 1 failed, 2 ignored, 439 filtered out. The new home assertion compared raw Windows environment strings and exposed different slash spellings from `cache.join("home")` versus the expected root join. Changed production construction to `root.join("data/cache/hf/home")`, matching the existing cache/temp path convention. The succeeding filtered run and workspace run verify the corrected source.

Existing session-6 work was preserved. A baseline comparison detected concurrent unrelated work: `ui/src/features/setup/model.ts` and `model.test.ts` became modified, and the ComfyUI-version option sentence in `docs/DOCUMENTATION.md` changed to describe the latest-commit wizard choice. This task did not edit the UI files or that sentence. Removing D4's new paragraph and substituting only that concurrent sentence in an in-memory comparison exactly reproduced the LF-normalized documentation baseline SHA-256 `96a27d0b0237389d7cf712f3bd5dc17639cbe0f31840244d48e8c4ade00f907b`. Removing only row 25 likewise reproduced the residuals baseline SHA-256 `4c809a212b84e63722ae1b34682e8e85618d18f2caad3eb9df7a62996d348c86`. No prior documentation content was reverted.

The live HF test was compiled and remained ignored in both passing suites, as requested; main must run it explicitly with `VJ_REAL_HF_EXE` set to the absolute pinned tool path:

```powershell
cargo test -p infrastructure --locked -j 6 real_hf_download_succeeds_without_user_profile_variables -- --ignored --nocapture
```

No native download or app-UI acceptance was performed by this package. Linux-specific behavior was not compiled or run on this Windows machine. The supplied native failure finding is the problem evidence, not a claim of post-fix native acceptance.
