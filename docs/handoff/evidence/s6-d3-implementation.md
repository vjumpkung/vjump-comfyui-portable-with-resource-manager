# Session 6, package D3 + D5: implementation report (coder)

Scope: `crates/infrastructure/src/environment/python.rs` (+ its `tests` module) and the three docs named in the plan. No git write commands were run. No `RUSTUP_TOOLCHAIN` / `CARGO_TARGET_DIR` set (default MSVC toolchain, default `target/`). Nothing under `D:\vjc-s6\` or `D:\comfyui-test\` was touched; no `vjumpkung-comfyui-desktop` process was killed.

## Per-file summary

### `crates/infrastructure/src/environment/python.rs`
- Imports (lines 3-30): added `HashMap`, `OsString`, `domain::shared::model_values::SafeFileName`, `crate::shared::model_paths::pin_absolute`.
- `PinnedPython` / `PinnedArch` (242-257): private serde structs for the pinned `PYTHON_DOWNLOADS_JSON` entries (`name`, `arch.family`, `os`, `libc`, `major`, `minor`, `patch`, `variant`).
- `minor_link_name(version, os, arch)` (260-282) [D3 step 1]: parses the pinned metadata, finds exactly one entry with the same major/minor/patch, `os`, `arch.family` and `variant == null`, and returns `format!("{name}-{major}.{minor}-{os}-{family}-{libc}")`. Zero or several matches, or a parse failure, return `failed("The pinned Python download metadata has no entry for this platform.")`. The name comes from the pinned data, so it is right for 3.12.15, 3.13.16 and 3.14.8 (the recommended profile is now 3.12).
- `is_link(&Metadata)` (285-295): symlink, or on Windows any reparse point (`file_attributes() & 0x400`, same test as `reject_reparse`). A junction is detected by the attribute, not by `FileType::is_symlink()`.
- `clear_copied_minor_link(layout, name, ctx)` (299-334) [D3 step 2], using `symlink_metadata` (never follows):
  - missing: `Ok(())`;
  - link/reparse point: `Ok(())`, left for uv;
  - not a directory: `failed("The app's Python cache contains an unexpected item named <name>. Remove it and retry.")`, item untouched;
  - real directory: logs `Removing a copied Python version folder (<name>); uv recreates its link.` through `ctx.log`, then `pin_absolute(&layout.managed_python_dir())` and `remove_owned_tree(name)`. Any failure maps to `failed("A copied Python version folder (<name>) in the app's Python cache could not be removed. Close programs that use it and retry.")`.
- `managed_install_args(version, metadata) -> Vec<OsString>` (337-348) [D5]: `--no-config python install <version> --no-bin --no-registry --python-downloads-json-url <metadata>`.
- `ensure_managed_python` (368-374) [D3 step 3 + D5]: after `ensure_cache_dirs` and the metadata write, computes `os` (`cfg!(windows)` -> `"windows"` else `"linux"`, arch `"x86_64"`), runs `clear_copied_minor_link(layout, &minor_link_name(..)?, ctx)?`, then builds the install command from `managed_install_args`. The `uv python find` command is unchanged.

### `crates/infrastructure/src/environment/python/tests.rs`
- Imports (1-4): `Mutex`, `SetupEvent`, `SetupProgressSink`.
- New tests (378-607, appended before the ignored real-interpreter test), exact names from the plan:
  - `minor_link_name_is_derived_from_pinned_metadata` (378): every bundled entry's derived name equals its key with `{major}.{minor}.{patch}` replaced by `{major}.{minor}`; literal checks `cpython-3.13-windows-x86_64-none` and `cpython-3.12-linux-x86_64-gnu`; unknown patch (3.13.99), unsupported minor (3.11.9), `macos`, and `aarch64` all return the exact error message.
  - `missing_minor_link_is_left_alone` (469).
  - `copied_minor_link_directory_is_removed_and_patch_directory_kept` (479): nested files, a non-ASCII subfolder name, the sibling patch directory kept; asserts the exact log line through a small recording sink.
  - `#[cfg(windows)] minor_link_junction_is_left_for_uv` (512): `cmd /C mklink /J`, skip if unavailable; asserts the junction remains and the target file is unchanged.
  - `#[cfg(windows)] junction_inside_copied_minor_directory_is_not_followed` (530): fails closed with the exact removal message; the sentinel file outside the cache survives.
  - `unexpected_file_at_minor_link_path_fails_closed` (559): exact message; file still there with its content.
  - `#[cfg(unix)] minor_link_symlink_is_left_for_uv` (581): compiled only on Unix (not compiled or run here).
  - `managed_python_install_never_registers_with_windows` (595): full-vector equality of `managed_install_args` (includes `--no-bin` then `--no-registry`).
  - Helpers: `LogSink` (429), `python_cache` (440), `patch_directory` (450), `directory_link` (459, Windows).

### Docs
- `docs/DOCUMENTATION.md:221` (section 4, Moved install and guided rebuild): the copied-link paragraph, verbatim from the plan.
- `docs/DOCUMENTATION.md:237` (section 5, Environment page): the PEP 514 registration paragraph, verbatim from the plan.
- `docs/handoff/evidence/s3-residuals.md:29-30`: rows 23 and 24 (next free numbers; 22 was the last).

## Commands and results (all from the repo root)

| # | Command | Result |
|---|---|---|
| 1 | `cargo fmt --all -- --check` | exit 0 (after a final `cargo fmt --all`) |
| 2 | `cargo clippy --workspace --all-targets --locked -j 6 -- -D warnings` | exit 0, no warnings (a first run flagged `needless_borrows_for_generic_args` on `Layout::new(&...)` in my test helper; fixed) |
| 3 | `cargo test -p infrastructure --locked -j 6 minor_link` | 5 passed, 0 failed, 0 ignored, 436 filtered (lib); 0 tests in `real_setup` and `workflow` |
| 4 | `cargo test -p infrastructure --locked -j 6 managed_python_install_never_registers_with_windows` | 1 passed, 0 failed, 440 filtered (lib) |
| 5 | `cargo test --workspace --locked -j 6 --no-fail-fast` | exit 0, 0 failures. Passed per target: application 208, desktop lib 54, desktop `main` 0, `acl` 11, `bindings` 1 (+1 ignored), `navigation` 2, `stack_reserve` 2, `startup_smoke` 0 (+1 ignored), domain 47, infrastructure lib 434 (+7 ignored), `real_setup` 0 (+1 ignored), `workflow` 9; doc-tests 0. Total 768 passed, 0 failed, 10 ignored |
| 6 | `node scripts/check-arch-deps.mjs` | `Architecture dependencies: OK`, exit 0 |

Extra runs:
- `cargo test -p infrastructure --locked -j 6 --lib -- --nocapture junction managed_python_install_never_registers_with_windows`: 5 passed, no "junction creation is unavailable" line, so the two Windows junction tests really ran and did not skip.
- `cargo test -p infrastructure --locked -j 6 --lib junction_inside_copied_minor_directory_is_not_followed`: 1 passed (see deviation 1).

## Deviations and notes

1. The plan's exact test name `junction_inside_copied_minor_directory_is_not_followed` does not contain the substring `minor_link`, so the success command `... minor_link` runs 5 of the 7 Windows-applicable D3 tests (the junction-inside test is not matched; the D5 test has its own command). I kept the exact names and ran the junction-inside test separately (extra run above). The workspace run covers all of them.
2. One message the plan did not specify: if `symlink_metadata` on the minor-link path fails with anything other than NotFound, the function fails closed with "The app's Python cache item named <name> could not be inspected. Close programs that use it and retry." (modeled on `remove_venv_dir`'s "could not be inspected" message). This path has no test (hard to provoke portably).
3. `pin_absolute` refuses a reparse point anywhere in the install root's ancestor chain, so a root that lives under a junction fails closed with the removal message when a copied folder has to be removed. This follows from the plan's required use of `pin_absolute` and matches the app's existing reparse policy.
4. The editor hook's rustfmt run reformatted two untouched `assert!(adapter ...)` blocks in `tests.rs` differently from `cargo fmt`; the final `cargo fmt --all` restored the project format. The `tests.rs` diff now has only my two hunks (imports and appended tests).
5. Not verified here: the Linux code paths (symlink test, `pin_absolute` on Unix) are not compiled or run on this machine (as the residual row states). `--no-registry` was confirmed in `uv python install --help` of the locally installed uv 0.12.13; the pinned uv 0.13.1 was not run (no pinned uv was used, to keep out of the other roots). Native evidence (copy a healthy root with `robocopy /E`, Rebuild, and confirm no new `HKCU\Software\Python\Astral` key) is pending, as recorded in residual rows 23 and 24.
6. Files outside the stated scope were not modified. `DOCUMENTATION.md` and `s3-residuals.md` also carry earlier session-6 packages' uncommitted edits; only the three hunks listed above are from this package.
