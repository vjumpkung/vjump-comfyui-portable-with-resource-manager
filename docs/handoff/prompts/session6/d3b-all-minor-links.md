ROLE: coder (write-capable). Mechanically implement the correction below. Do not redesign or expand scope. If it conflicts with the code, stop and report the gap precisely.

## Build environment and repository rules (mandatory)
- WORKING COPY: `D:\vjump\vjump-comfyui-portable-with-resource-manager` (branch `feat/m0-skeleton`, HEAD `e384d43`). Uncommitted working-copy changes = verified session-6 packages Q, S0, F, G, X, G2b, G2c, D3+D5 + docs: preserve them; never revert files you did not change.
- Read `AGENTS.md` and `docs/handoff/tooling/coder-env.md`. Default MSVC toolchain, default `target/`; do NOT set RUSTUP_TOOLCHAIN or CARGO_TARGET_DIR. `-j 6`; never two cargo commands at once. Run `cargo fmt --all` after your last Rust edit (an editor hook may reformat). LF line endings.
- No git write commands on this repository. Do not touch `D:\vjc-s6\` or `D:\comfyui-test\`; do not kill `vjumpkung-comfyui-desktop` processes.

## Native finding (main, 2026-10-20 14:36, binary s6-08 = through D3+D5, root `D:\vjc-s6\r1-copy` = `robocopy /E` copy of a healthy py312 install whose uv cache holds 3.12.15 and 3.13.16)
Rebuild → `tools` step: D3 logged "Removing a copied Python version folder (cpython-3.13-windows-x86_64-none); uv recreates its link." then `uv.exe --no-config python install 3.13.16 --no-bin --no-registry …` failed (exit 2): "Failed to create Python minor version link directory; cause: failed to remove directory `…\uv-python\cpython-3.12-windows-x86_64-none`: The directory is not empty. (os error 145)". uv (re)creates the minor-version links of EVERY managed installation it finds, not only the requested version, so a copied link folder of any other installed version breaks the install too. (`--no-registry` is present: D5 confirmed in the command line.)

## Correction (main; AI-decidable correction of the approved D3 plan, same safety rules)
In `crates/infrastructure/src/environment/python.rs`:
1. Add `fn minor_link_names(os: &str, arch: &str) -> Result<Vec<SafeFileName>, ProvisionError>`: every distinct minor-link name derivable from the pinned `PYTHON_DOWNLOADS_JSON` for this os/arch family with `variant` null (same name format as `minor_link_name`), sorted, deduplicated. Keep `minor_link_name` (it still validates that the requested version exists and is used for that check).
2. In `ensure_managed_python`, after the existing `minor_link_name(version, …)` validation, call `clear_copied_minor_link(layout, &name, ctx)` for EVERY name from `minor_link_names` (the existing function already leaves missing entries, symlinks and reparse points alone, removes only real copied directories through `pin_absolute` + `remove_owned_tree`, and fails closed otherwise). Update the doc comment: uv recreates the minor links of all managed installations on every install.
3. Tests (python tests module, exact names): `minor_link_names_cover_every_pinned_minor_version` (contains the 3.12, 3.13 and 3.14 names for the current platform; deduplicated); `copied_minor_links_of_other_versions_are_removed_too` (fixture managed-python dir with real directories `cpython-3.12-<platform>` and `cpython-3.13-<platform>` each holding a nested file, plus the patch directories; running the clearing for version 3.13 removes BOTH copied minor directories and keeps both patch directories) — use whatever fixture style the existing `copied_minor_link_directory_is_removed_and_patch_directory_kept` test uses.
4. Docs: in `docs/DOCUMENTATION.md` §4 change "The tools step deletes that copied folder" to "The tools step deletes every such copied folder"; in `docs/handoff/evidence/s3-residuals.md` row 23 append "D3b: uv recreates the links of every managed version, so every copied minor-link folder is cleared (native finding on a copy with 3.12 + 3.13)."

## Success criteria (report exact counts)
```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -j 6 -- -D warnings
cargo test -p infrastructure --locked -j 6 minor_link
cargo test --workspace --locked -j 6 --no-fail-fast
node scripts/check-arch-deps.mjs
```
OUTPUT: per-file summary with file:line, key diffs, command results with counts, deviations. Also write it to `docs/handoff/evidence/s6-d3b-implementation.md`.
