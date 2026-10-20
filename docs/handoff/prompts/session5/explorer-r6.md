ROLE: explorer (read-only). Do NOT edit files, do NOT run git write commands, do NOT run cargo/pnpm builds or tests (a coder holds the build lock). Use rg / file reads (LSP if available). Report exact evidence; do not design the fix.

REPO: D:\vjump\vjump-comfyui-portable-with-resource-manager (Rust/Tauri 2 app installing ComfyUI custom nodes). Context: residual #6 — on Windows, a child process started with (or chdir-ing into) a node-controlled directory may search that directory for DLLs (and for bare-name executables). Pinned uv 0.13.1, MinGit 2.55 and CPython do not call SetDefaultDllDirectories. NFR-SEC-3 requires explicit consent BEFORE first execution of third-party code. See docs/handoff/evidence/s3-residuals.md row 6, docs/handoff/plans/m5-h-hardening-approved-plan.md (F9, "Windows DLL policy"), docs/handoff/plans/m5-backend-approved-plan.md (WP1/WP2), docs/handoff/plans/m6-approved-plan.md (E2 curated).

QUESTION (bounded): Map EVERY child process the app spawns for custom nodes and curated setup nodes, in execution order, and classify each as pre-consent or post-consent.

For each of these flows: (a) Registry install (archive download → staging → preview → review → commit), (b) Git install from URL, (c) Git update (incl. frozen target / staged checkout), (d) node reinstall/repair of requirements, (e) curated setup nodes (E2: NodeTarget::Setup, prepare → batch review → commit), (f) manual/scanned nodes discovered on disk (rescan / git_status / inspect), (g) enable/disable/uninstall, list:
  1. the exact command (program, key args such as -C, --git-dir, --work-tree, --dry-run, -I/-B/-c for python, stdin use), file:line of the spawn spec;
  2. the process working directory (CommandSpec.cwd or inherited) and whether the tool itself chdirs into a node-controlled directory (git -C, git clone checkout, submodules, setup_work_tree);
  3. whether the directory can contain attacker-supplied files at that moment (staged archive/clone contents already written? before or after checkout?);
  4. pre-consent vs post-consent, with the file:line of the consent check that separates them (workflow.rs commit/execution consent, setup.rs curated decisions);
  5. any child that resolves an executable by bare name (e.g. "python", "git", "sh") rather than an absolute path.
Also report:
  6. How guarded requirements are passed to uv preview and install (stdin vs file), which relative constructs are supported/refused today (nested -r/-c, local wheel paths, ./pkg, file: URLs, editable), with the tests that pin this (e.g. the compatibility fixture around crates/infrastructure/src/custom_nodes/tests.rs ~3699 and ~5642/5681) and the refusal code (packages.rs ~104).
  7. How uv is invoked for the interpreter (does the app pass --python <abs path>?) and whether the app ever runs python itself pre-consent with cwd = node dir.
  8. The test `windows_children_exclude_cwd_from_default_dll_search` / fixture `dll_lookup_child` (crates/infrastructure/src/shared/process/tests.rs ~670-720): what exactly it asserts.
  9. The existing tree-validation hooks where a new pre-consent rule over a staged/pinned node tree could be enforced (H1 validate_tree / read_tree in custom_nodes/tree.rs, NodeLocationGuard), with file:line.

OUTPUT: Markdown, one table per flow (columns: step, command, cwd/chdir, attacker files present?, pre/post consent, file:line), then sections 5-9. State remaining uncertainty explicitly.
