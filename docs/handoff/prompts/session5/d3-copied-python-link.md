ROLE: coder (write-capable). Mechanically implement the advisor-approved plan below. Do not redesign or expand scope. If the plan is underspecified or conflicts with the code, stop that item and report the gap precisely instead of improvising.

## Build environment and repository rules (mandatory)
- WORKING COPY: `D:\vjump\vjump-comfyui-portable-with-resource-manager` (branch `feat/m0-skeleton`, HEAD `a4175d1`). Uncommitted working-copy changes are verified packages P0, I and D2 (see `docs/handoff/patches/` and `docs/handoff/evidence/s5-*`): preserve them; never revert files you did not change for this package.
- Read `AGENTS.md` and `docs/handoff/tooling/coder-env.md` first. Default MSVC toolchain from `rust-toolchain.toml`, default `target/`; do NOT set RUSTUP_TOOLCHAIN or CARGO_TARGET_DIR.
- Memory: 32 GB machine, ~10 GB free. Use `-j 6`; targeted `-p infrastructure` runs with test filters while iterating; workspace-wide commands once at the end. Never run two cargo commands at once.
- Rust: rustfmt, clippy `-D warnings`; match surrounding style and comment density. Normalize edited files to LF.
- Do not commit, push, stash, reset, checkout, apply, clean or touch git history. Do not modify files outside the stated scope. Do not touch `D:\vjc-s5\` or kill `vjumpkung-comfyui-desktop` processes.

## Problem (native finding, verified by main 2026-10-20, scenario 4)
A healthy installed root copied with `robocopy /E` (follows junctions, like an Explorer copy) → app shows `rebuild_required` (moved) correctly → Rebuild fails in the `tools` step:
`uv.exe --no-config python install 3.13.16 --no-bin --python-downloads-json-url <root>\data\cache\python-downloads.json` → exit 2, "Failed to create Python minor version link directory; cause: failed to remove directory `<root>\data\cache\uv-python\cpython-3.13-windows-x86_64-none`: The directory is not empty. (os error 145)". In the original root that entry is uv's minor-version link (a junction → `cpython-3.13.16-windows-x86_64-none`); the copy turned it into a real directory. venv `pyvenv.cfg` `home` points at the minor-link path, so the link must be uv's own, not a stale copy. `ensure_managed_python` (crates/infrastructure/src/environment/python.rs:238-277) runs on every setup mode (from `ensure_hf`, tools.rs:391, and `python_venv`, python.rs:444), so Repair/first-run on a copied root break too. Pinned uv 0.13.1 has no option to disable minor links. Separately, `uv python install` registers each managed Python under `HKCU\Software\Python\Astral\CPython<ver>` (PEP 514) because `--no-registry` is not passed (verified in the registry).

## Approved plan (advisor stand-in rulings D3 + D5: GO; one package; python.rs only)
### D3 — clear a copied minor-version link before `uv python install`
File: `crates/infrastructure/src/environment/python.rs` (+ its `tests` module).
1. `fn minor_link_name(version: PythonVersion, os: &str, arch: &str) -> Result<SafeFileName, ProvisionError>`: parse the app's pinned `PYTHON_DOWNLOADS_JSON`; find exactly one entry with matching major/minor/patch, `os`, `arch.family` and `variant` null; return `format!("{name}-{major}.{minor}-{os}-{family}-{libc}")` (today `cpython-3.13-windows-x86_64-none` / `cpython-3.13-linux-x86_64-gnu`). Zero or several matches → `failed("The pinned Python download metadata has no entry for this platform.")`. os/arch from `cfg!(windows)` → `"windows"`/`"linux"`, and `"x86_64"` (Release-1 targets only).
2. `fn clear_copied_minor_link(layout, name, ctx) -> Result<(), ProvisionError>`:
   - Missing → nothing.
   - Symbolic link, or on Windows ANY reparse point (`file_attributes() & 0x400`, same test as `reject_reparse`, crates/infrastructure/src/shared/model_paths.rs:502) → leave it for uv; never follow it. Do not rely on `FileType::is_symlink()` for junctions.
   - Real directory → log "Removing a copied Python version folder (<name>); uv recreates its link." (through the existing step log/ctx sink), pin the cache folder with `pin_absolute(&layout.managed_python_dir())` (model_paths.rs:61) and remove with `remove_owned_tree(&name)` (model_paths.rs:348: non-following, refuses inner links, identity-checked). Failure → `failed("A copied Python version folder (<name>) in the app's Python cache could not be removed. Close programs that use it and retry.")`.
   - A file or anything else → `failed("The app's Python cache contains an unexpected item named <name>. Remove it and retry.")`; leave it untouched.
3. Call order in `ensure_managed_python`: after `ensure_cache_dirs` and the metadata write, run `minor_link_name` then `clear_copied_minor_link`, then the install command.
Tests (exact names): `minor_link_name_is_derived_from_pinned_metadata` (every bundled entry: derived name == its key without `.{patch}`; unknown version → error); `missing_minor_link_is_left_alone`; `copied_minor_link_directory_is_removed_and_patch_directory_kept` (nested files); `#[cfg(windows)] minor_link_junction_is_left_for_uv` (create via `cmd /C mklink /J`, skip if unavailable like crates/infrastructure/src/custom_nodes/scan.rs:404; assert junction remains and target files unchanged); `#[cfg(windows)] junction_inside_copied_minor_directory_is_not_followed` (fails closed with the removal message; a sentinel file outside the cache survives); `unexpected_file_at_minor_link_path_fails_closed` (exact message; file still there); `#[cfg(unix)] minor_link_symlink_is_left_for_uv` (compiled only on Unix).

### D5 — never register managed Python with Windows
In python.rs extract a pure `managed_install_args(version, metadata) -> Vec<OsString>` used by the install command; it adds `--no-registry` next to `--no-bin` (always). Test: `managed_python_install_never_registers_with_windows`. The app performs NO registry cleanup.

### Docs (only these)
- `docs/DOCUMENTATION.md` §4 (moved install): add "Copy tools that follow links turn uv's Python version link (a junction on Windows, a symbolic link on Linux) into a real folder. The tools step deletes that copied folder inside the app's Python cache, `data/cache/uv-python`, and uv creates the link again. Links are never followed or deleted by the app."
- `docs/DOCUMENTATION.md` environment section: "The app's private Python interpreters are not registered with Windows, so the `py` launcher and IDEs do not list them. Development builds before this change registered them under `HKCU\Software\Python\Astral\CPython<version>`. If such an entry's `InstallPath` points into an install root's `data\cache\uv-python`, you can remove it with `reg delete "HKCU\Software\Python\Astral\CPython<version>" /f`."
- `docs/handoff/evidence/s3-residuals.md`: append two rows using the next free row numbers: "(E3/native) A copied root's materialized minor-version link made `uv python install` fail (os error 145). Fixed in package D3; native evidence pending. Linux symlink branch not run." and "(native) Managed Python was registered under HKCU\Software\Python\Astral (no `--no-registry`). Fixed in D3/D5; stale keys from earlier builds are user-removed."

## Success criteria (run at the end; report exact counts)
```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -j 6 -- -D warnings
cargo test -p infrastructure --locked -j 6 minor_link
cargo test -p infrastructure --locked -j 6 managed_python_install_never_registers_with_windows
cargo test --workspace --locked -j 6 --no-fail-fast
node scripts/check-arch-deps.mjs
```
OUTPUT: per-file summary with file:line, key diffs, every command result with counts, deviations with reasons. Also write it to `docs/handoff/evidence/s5-d3-implementation.md`.
