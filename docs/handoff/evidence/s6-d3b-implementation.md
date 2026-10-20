# Session 6, package D3b: implementation report (coder)

Implemented the approved correction for uv recreating minor-version links for every managed installation on each install. The requested Python version is still validated first; clearing now covers every distinct pinned minor-link name for the current OS and architecture family.

## Scope and environment

- Working copy: `D:\vjump\vjump-comfyui-portable-with-resource-manager`, branch `feat/m0-skeleton`, HEAD `e384d43`.
- Default pinned toolchain: `1.99.0-x86_64-pc-windows-msvc`; default `target/`. No `RUSTUP_TOOLCHAIN` or `CARGO_TARGET_DIR` overrides. Cargo build/test checks used `-j 6` and ran sequentially.
- Read `AGENTS.md` and `docs/handoff/tooling/coder-env.md` before implementation.
- Preserved the existing session-6 working-copy changes. Captured a SHA-256 baseline of 704 existing repository files; the initial post-edit comparison showed only the four intended existing files changed. This task also added this report. Unrelated concurrent documentation changes found at final verification are recorded below.
- No git write commands. Nothing under `D:\vjc-s6\` or `D:\comfyui-test\` was touched, and no `vjumpkung-comfyui-desktop` processes were killed.
- Edited source and documentation use LF line endings. Verification logs and baseline snapshots are under `target/s6-d3b-checks/`.

## Per-file summary and key diffs

| File:line | Change |
|---|---|
| `crates/infrastructure/src/environment/python.rs:284` | Added `minor_link_names(os, arch) -> Result<Vec<SafeFileName>, ProvisionError>`. Parses `PYTHON_DOWNLOADS_JSON`, filters by OS, architecture family and null variant, derives names using the existing format, validates each through `SafeFileName`, sorts and deduplicates. |
| `crates/infrastructure/src/environment/python.rs:316` | Updated the clearing doc comment to explain that uv recreates the minor links of all managed installations on every install. The removal implementation and its safety checks are unchanged. |
| `crates/infrastructure/src/environment/python.rs:391` | Keeps `minor_link_name(version, os, "x86_64")?` as the requested-version validation, then loops over every result of `minor_link_names` and calls `clear_copied_minor_link` before uv installation. D5 install arguments are unchanged. |
| `crates/infrastructure/src/environment/python/tests.rs:430` | Added `minor_link_names_cover_every_pinned_minor_version`: checks the current platform's 3.12, 3.13 and 3.14 names and strict ordering, which also rejects duplicates. |
| `crates/infrastructure/src/environment/python/tests.rs:526` | Added `copied_minor_links_of_other_versions_are_removed_too` using the existing Python-cache fixture style: creates real copied 3.12 and 3.13 minor directories with nested files and both patch directories; validates 3.13 and clears all pinned minor links; verifies both copied directories disappear and both patch interpreter files survive unchanged. |
| `docs/DOCUMENTATION.md:221` | In section 4, changed "The tools step deletes that copied folder" to "The tools step deletes every such copied folder". |
| `docs/handoff/evidence/s3-residuals.md:29` | Appended to row 23: "D3b: uv recreates the links of every managed version, so every copied minor-link folder is cleared (native finding on a copy with 3.12 + 3.13)." |
| `docs/handoff/evidence/s6-d3b-implementation.md:1` | Added this implementation and verification report. |

The existing clearing function still leaves missing entries, symlinks and Windows reparse points alone. It removes only real copied directories through `pin_absolute` and `remove_owned_tree` and otherwise fails closed.

Task-only diffs against the saved baseline: `python.rs` +27/-4 lines; `python/tests.rs` +49/-0; `DOCUMENTATION.md` +1/-1; `s3-residuals.md` +1/-1. The final whitespace check passed, and all five task-edited files passed the LF check.

## Commands and exact results

All commands ran from the repository root. `cargo fmt --all` ran successfully after the last Rust edit, before the required formatting check.

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | Exit 0; 0 formatting errors. |
| `cargo clippy --workspace --all-targets --locked -j 6 -- -D warnings` | Exit 0; 0 warnings. |
| `cargo test -p infrastructure --locked -j 6 minor_link` | Exit 0; infrastructure library: 7 passed, 0 failed, 0 ignored, 436 filtered out. `real_setup`: 0 run, 1 filtered out. `workflow`: 0 run, 9 filtered out. Aggregate: 7 passed, 0 failed, 0 ignored, 446 filtered out. Both new tests passed. |
| `cargo test --workspace --locked -j 6 --no-fail-fast` | Exit 0; 770 passed, 0 failed, 10 ignored, 0 filtered out. Per-target counts below. |
| `node scripts/check-arch-deps.mjs` | Exit 0; `Architecture dependencies: OK`. |

Workspace test counts:

| Target | Passed | Failed | Ignored |
|---|---:|---:|---:|
| application library | 208 | 0 | 0 |
| desktop library | 54 | 0 | 0 |
| desktop main | 0 | 0 | 0 |
| desktop `acl` | 11 | 0 | 0 |
| desktop `bindings` | 1 | 0 | 1 |
| desktop `navigation` | 2 | 0 | 0 |
| desktop `stack_reserve` | 2 | 0 | 0 |
| desktop `startup_smoke` | 0 | 0 | 1 |
| domain library | 47 | 0 | 0 |
| infrastructure library | 436 | 0 | 7 |
| infrastructure `real_setup` | 0 | 0 | 1 |
| infrastructure `workflow` | 9 | 0 | 0 |
| application doc-tests | 0 | 0 | 0 |
| desktop doc-tests | 0 | 0 | 0 |
| domain doc-tests | 0 | 0 | 0 |
| infrastructure doc-tests | 0 | 0 | 0 |
| **Total** | **770** | **0** | **10** |

## Deviations and remaining verification

No implementation deviations or scope expansions. All five required checks passed on their first run.

The final baseline comparison also found an unrelated concurrent modification to `docs/handoff/evidence/s6-advisor-rulings.md` and new files `docs/handoff/evidence/s6-advisor-u.md` and `docs/handoff/prompts/session6/u-update-modes.md`. This task did not edit those files and left them untouched.

The native multi-version Rebuild finding was supplied by main; this task did not rerun a native Rebuild or access the protected installation roots. Native acceptance of D3b remains pending. Linux-specific filesystem paths and the Unix symlink test were not compiled or run on this Windows machine.
