ROLE: advisor (read-only). Do NOT edit files, do NOT run git write commands, do NOT run cargo builds/tests (another job holds the build lock). Read code and docs only.

REPO: D:\vjump\vjump-comfyui-portable-with-resource-manager (Rust/Tauri 2 Windows+Linux desktop app that installs/runs ComfyUI and runs uv, git and python child processes, including inside user custom-node directories). HEAD a4175d1 + uncommitted P0 startup fix (unrelated).

## Question: residual #6 (docs/handoff/evidence/s3-residuals.md row 6) — decide its release disposition
Statement (H2 coder, measured): `SetDllDirectoryW("")` in the app process (crates/infrastructure/src/shared/process/windows.rs ~19-40, `initialize_tool_policy` in crates/infrastructure/src/shared/process.rs:40, called first thing in crates/desktop/src/lib.rs `run()`) removes the current directory from THIS process's DLL search, but it is NOT inherited by child processes. Measured: a child started with a node directory as cwd still loads a DLL from its cwd (test `windows_children_exclude_cwd_from_default_dll_search` / fixture `dll_lookup_child` in crates/infrastructure/src/shared/process/tests.rs ~674-716 — read what that test actually asserts now). uv/git/python children started with a custom-node directory as cwd therefore keep the default DLL search order (with SafeDllSearchMode the cwd is searched after the system directories, but before PATH).

Context: approved hardening plan docs/handoff/plans/m5-h-hardening-approved-plan.md (search "DLL", "cwd", "stdin"): H2 kept "stdin + node-dir cwd for guarded requirements". Custom-node directories are user-chosen third-party content the app installs only after explicit consent; their code runs inside ComfyUI anyway once installed. Release 1 = Windows x64 + Linux x64, NVIDIA only.

Decide (classify AI-decidable vs user-only):
R6-1. Is this a release blocker, an accepted documented residual, or does it need a code change before release? Consider the threat model (who controls a DLL dropped into a node directory, when do children run with that cwd: before consent (preview/preflight) vs after consent), and which child processes are affected (uv pip compile/install with cwd = node dir? git with cwd = node dir? python BOOT/install.py?). Use file:line evidence for each spawn site that sets current_dir to a node/untrusted directory.
R6-2. If a code change is needed: the minimal fix (e.g. never use an untrusted directory as cwd for pre-consent children and pass paths explicitly; or PROC_THREAD_ATTRIBUTE / process mitigation policy; or a launcher), exact files/functions/tests, and whether it can be done safely now.
R6-3. Exact wording for docs (README Security notes / DOCUMENTATION) and docs/RELEASE_ACCEPTANCE.md if accepted.

Required output: go/no-go for release on this point; decision + reasoning with file:line evidence; corrected plan if code changes are needed; doc wording; only indispensable user questions.
