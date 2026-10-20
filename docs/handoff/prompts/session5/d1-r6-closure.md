ROLE: coder (write-capable). Mechanically implement the advisor-approved plan below. Do not redesign or expand scope. If the plan is underspecified or conflicts with the code, stop that item and report the gap precisely instead of improvising.

## Build environment and repository rules (mandatory)
- WORKING COPY: `D:\vjump\vjump-comfyui-portable-with-resource-manager` (branch `feat/m0-skeleton`, HEAD `a4175d1`). Uncommitted working-copy changes are verified packages P0, I, D2, D3(+D5), D4 (see `docs/handoff/patches/` and `docs/handoff/evidence/s5-*`): preserve them; never revert files you did not change for this package.
- Read `AGENTS.md` and `docs/handoff/tooling/coder-env.md` first. Default MSVC toolchain from `rust-toolchain.toml`, default `target/`; do NOT set RUSTUP_TOOLCHAIN or CARGO_TARGET_DIR.
- Memory: 32 GB machine, ~10 GB free. Use `-j 6`; targeted `-p infrastructure` runs with test filters while iterating; workspace-wide commands once at the end. Never run two cargo commands at once. Git tests must actually execute (git is on PATH).
- Rust: rustfmt, clippy `-D warnings`; match surrounding style and comment density. Normalize edited files to LF.
- Do not commit, push, stash, reset, checkout, apply, clean or touch git history. Do not modify files outside the stated scope. Do not touch `D:\vjc-s5\` or kill `vjumpkung-comfyui-desktop` processes.

## Background (read these first)
`docs/REQUIREMENTS.md` NFR-SEC-3; `docs/handoff/evidence/s5-advisor-r6.md` (Codex advisor: residual #6 is a pre-consent release blocker); `docs/handoff/evidence/s5-exp-r6.md` (every child spawn per flow with cwd and pre/post-consent); `docs/handoff/plans/m5-h-hardening-approved-plan.md` F9 (premise "SetDllDirectoryW propagates to children" is FALSE — measured). Facts: the manager's `SetDllDirectoryW("")` is process-local; uv 0.13.1 / MinGit 2.55 / CPython do not call SetDefaultDllDirectories; on Windows a child's DLL search and CreateProcess bare-name lookup consult its current directory (top level only). Before consent the app runs `uv pip install --dry-run` with cwd = staged node dir (packages.rs:91/414) and Git `-C <staging>` commands after checkout.

## Approved plan (advisor stand-in re-ruling D1: option B accepted, replaces the Codex invariant)
New invariant: "Before exact-source consent, on Windows, no tool runs with newly acquired node content as its working directory while that content's top level holds an entry with a native-image extension; uv's interpreter query must not import Python modules from that folder." Installed/user-placed nodes are not re-checked (documented residual).

1. `crates/infrastructure/src/custom_nodes/tree.rs`:
   - `pub(crate) const ROOT_NATIVE_MESSAGE: &str = "This node has programs or libraries (.dll, .exe and similar) in its top-level folder. Tools could load them before you approve the node, so it cannot be installed.";`
   - `pub(crate) fn is_native_image_name(name: &str) -> bool`: extension = text after the last `.`, `to_uppercase()` (Unicode — NTFS folds `ſ` to `S`), member of {DLL, EXE, COM, CPL, DRV, OCX, SCR, SYS}; `.dll` alone matches.
   - `pub(crate) fn refuse_root_native(dir: &Arc<PinnedDirectory>) -> Result<(), NodeAppError>`: list top-level entries only (files AND folders), names validated as in `frame`; when `cfg!(windows)` a match returns `NodeAppError::Failed(ROOT_NATIVE_MESSAGE)` (Failed, NOT UnsafeSource — Failed makes staging cleanup run: files.rs:385-388 registry, 509-515/621-627 git); on other OSes it does nothing.
2. `crates/infrastructure/src/custom_nodes/files.rs:~383` — call `tree::refuse_root_native(&directory)?` right after `validate_tree(&directory, false)?` (after the single-root hoist, before any uv preview).
3. `crates/infrastructure/src/custom_nodes/git.rs::clone_checkout` — Windows only, after `cat-file` (~527) and before BOTH checkout branches (~535/~563): run `self.read(root, hooks, staging, &["ls-tree", "--full-tree", "-z", "--name-only", &resolved.commit])` (with `Some(staging)` and no ctx), split on NUL; a name that is not UTF-8 or fails `SafeFileName::new` → `UnsafeSource`; a native-image name → `failed(ROOT_NATIVE_MESSAGE)`; run errors: `Cancelled` and `Offline` pass through, anything else → `failed("The repository tree could not be listed")`.
4. `files.rs:~478` and `~611` — call `refuse_root_native` after `validate_fresh_git` as defense in depth.
5. `git.rs:~236` — comment that `GIT_CONFIG_NOSYSTEM=1` is load-bearing: MinGit's `etc/gitconfig` defines `filter "lfs"` with `process = git-lfs filter-process` and `required = true`; without it a malicious `.gitattributes` would spawn a filter during pre-consent checkout.
6. `crates/infrastructure/src/shared/process.rs`: add `pub(crate) fn default_working_directory() -> io::Result<PathBuf>` = parent of `std::env::current_exe()`, cached once (OnceLock). `run_inner` ALWAYS calls `current_dir(spec.cwd.unwrap_or(default))`; failure to determine it → `RunError::Spawn` (fail closed). `crates/infrastructure/src/environment/packages.rs:~107` (preflight resolution relative to `current_dir()`) must use the same function so preflight and uv agree. Tests that relied on the inherited cwd: make their program/argument paths absolute; never add a production fallback.
7. `crates/infrastructure/src/shared/process/windows.rs:~27` — comment: the DLL setting covers "this process only; Windows does not pass it to children (residual 6)".
8. STOP CONDITION: if the live isolation test (below) shows uv's interpreter query importing a trap module from the node folder with the pinned uv, STOP and report — do not work around it.

Tests (infrastructure; exact names):
- Rename `windows_children_exclude_cwd_from_default_dll_search` → `windows_dll_initializer_is_process_local`; the helper asserts exactly 193 before calling the initializer and 126 after.
- `children_without_cwd_start_in_the_executable_directory` (ignored helper child prints its cwd).
- `root_native_names_match_pe_extensions_case_insensitively` (match: `a.dll`, `A.DLL`, `c.Exe`, `x.ſcr`, `.dll`; no match: `x.pyd`, `x.so`, `x.bat`, `check.bat`, `install.bat`, `x.dll.txt`, `dll`, `x.whl`).
- `#[cfg(windows)] registry_prepare_refuses_root_native_image_before_preview` (no preview recorded; staging, zip and hoist removed).
- `#[cfg(windows)] registry_prepare_refuses_root_native_image_after_hoisting_single_root`.
- `registry_prepare_accepts_nested_native_image`.
- `#[cfg(windows)] git_install_refuses_root_native_image_before_checkout` (recorded specs show `ls-tree` and no `checkout` for the staging folder; staging removed).
- `#[cfg(windows)] git_update_refuses_root_native_image_in_frozen_target_before_checkout` (live digest unchanged).
- `git_install_accepts_nested_native_image_and_root_scripts` (root `check.bat`, `x.ps1`, `x.pyd`; nested `lib/x.dll`).
- `existing_node_with_root_native_image_still_scans_and_reports_status`.
- `git_specs_always_disable_system_and_global_config` (clone, `-C` commands, config preflight and `ls-tree` each carry `GIT_CONFIG_NOSYSTEM=1` and `GIT_CONFIG_GLOBAL` = the app config, and none of `GIT_CONFIG_SYSTEM`, `GIT_CONFIG_PARAMETERS`, `GIT_CONFIG_COUNT`, `GIT_DIR`, `GIT_WORK_TREE`).
- Live (skips when uv is unavailable, pattern of `node_preview_never_executes_editable_build_backend`, tests.rs ~5226): `node_preview_never_imports_python_modules_from_node_directory` — fresh fixture root ⇒ fresh `UV_CACHE_DIR`; guarded requirements use a local wheel (offline, like the compatibility fixture ~5614); trap `json.py`, `platform.py`, `sysconfig.py` at the node root each write `<root>/trap-<name>` and raise; assert preview OK and no trap file exists; print `uv --version`. Main will run it with the pinned uv 0.13.1 first on PATH.
- Keep unchanged and passing: `node_preview_never_executes_editable_build_backend`, `guarded_requirements_preserve_relative_includes_and_local_paths` (compatibility fixture).

Docs (only these):
- `README.md` Security notes — add: "On Windows, the application refuses to stage a new custom node whose top-level folder contains programs or libraries (`.dll`, `.exe`, `.com`, `.cpl`, `.drv`, `.ocx`, `.scr`, `.sys`), because Git and uv run in that folder before you approve the node. After you approve a node, its requirements and an approved `install.py` run in the node folder with your permissions and may load native libraries the node ships; a Python venv is not a sandbox."
- `docs/DOCUMENTATION.md` security section (§27 or the closest equivalent table) — new row "Native-library boundary (Windows)": "The manager's DLL-search setting applies to the manager process only; Windows does not pass it to uv, Git or Python. Before consent, tools run in a newly downloaded or cloned node folder only if its top level has no entry with a native-image extension: archives are checked after extraction, Git sources from the commit's tree listing before checkout and again after it. uv queries Python in isolated mode. Installed and user-placed nodes are not re-checked. Tools started without a working directory start in the application's own folder, never the folder it was launched from."
- `docs/handoff/evidence/s3-residuals.md`: row 6 status → "Closed before consent (D1); residual after consent and for installed/user-placed nodes" with the statement above; append rows (next free numbers): "(D1) Children without an explicit cwd inherited the manager's launch folder (Python `sys.path[0]`, `git ls-remote` repo discovery) — closed: they start in the executable's folder."; "(latent) setup IPC still accepts `allow_source_builds=true` although the UI never sends it — no change."; "(to verify) Git `UnsafeSource` keeps the staging folder and reconcile skips Failed operations (use_cases.rs:386), so a refused attempt may leave `.disabled/.vjump-staging-*` behind — needs verification."
- `docs/handoff/plans/m5-h-hardening-approved-plan.md` F9 and line ~188 — append a dated correction note (2026-10-20): the propagation premise was false; replaced by the D1 invariant.

## Success criteria (run at the end; report exact counts)
```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -j 6 -- -D warnings
cargo test -p infrastructure --locked -j 6 root_native
cargo test -p infrastructure --locked -j 6 git_specs_always_disable_system_and_global_config
cargo test -p infrastructure --locked -j 6 node_preview_never_imports_python_modules_from_node_directory -- --nocapture   (report the printed uv version and whether it ran or skipped)
cargo test --workspace --locked -j 6 --no-fail-fast
node scripts/check-arch-deps.mjs
```
OUTPUT: per-file summary with file:line, key diffs, every command result with counts, deviations with reasons. Also write it to `docs/handoff/evidence/s5-d1-implementation.md`.
