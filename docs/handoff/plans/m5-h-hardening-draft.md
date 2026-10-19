# M5 package H — hardening from the thinker review (main's draft for advisor)

Input: `docs/handoff/evidence/thinker-wp1-wp6-review.md` (findings 1–16, line numbers at commit 4749d8a). WP2+WP3 is now verified on top (`docs/handoff/patches/s3-01-wp23-verified.patch`); WP4/WP5 are next and touch `custom_nodes/{model,workflow,use_cases,tests}.rs`, `files.rs` recovery, environment ports/execute, sqlite, stack, desktop state.rs. P (updater/diagnostics) is being merged now (desktop state/lib/ipc/UI).

Constraints: this machine is Windows-only (no Linux target, no Linux C cross compiler — Linux-only code cannot be compiled here), MSVC builds, ~8–10 GB free RAM. Release 1 = Windows x64 + Linux x64.

## Proposed scope (main's draft — advisor to correct)

| # | Proposed action | Files |
|---|---|---|
| F1 HIGH depth | Fix now. Hard caps in `extract_zip` and `validate_tree` (Git trees too): ≤ 64 path components, each component ≤ 255 bytes, total relative path ≤ 1024 bytes; refuse deeper/longer as `UnsafeSource`. `remove_owned_tree` becomes iterative (heap stack, at most O(depth) open handles, depth already capped); case-insensitive collision detection with memory O(total entry-name bytes) (e.g. map of (parent id, folded name) → id instead of storing every prefix string). Tests: deep archive refused before writing; deep Git tree refused; removal of a 64-level tree on a small-stack thread; many long names bounded. | `shared/model_paths.rs`, `custom_nodes/tree.rs`, archive extraction module, tests |
| F2 HIGH recycle | Fix now. Windows: `IFileOperationProgressSink` (`PreDeleteItem`) aborts any delete without `TSF_DELETE_RECYCLE_IF_POSSIBLE`; abort → error, caller keeps `CleanupPending` / model delete reports trash failure (FR-MODEL-3 offers explicit permanent delete separately). Applies to node recycling and model-library delete. Test: a forced "cannot recycle" path (e.g. injected sink decision) yields error and the item still exists. | `shared/trash.rs` (+tests), possibly Cargo features of the `windows` crate |
| F3 MED `.git` | Fix now. Registry archive extraction refuses any `.git` path component (case-insensitive). Scan: app marker wins — a Registry-marked node with `.git` is a provenance conflict, never run Git on it; run `git status` only for Git-marked nodes and unmarked/manual nodes (FR-NODE-5 needs remote/branch/commit/dirty for manual clones; their `.git/config` is local, never cloned). | archive extraction, `custom_nodes/scan.rs`, `files.rs` scan |
| F4 MED close | Fix now. Close/StopThenExit treats `NodeRescanFailed` as "ComfyUI stopped" and exits (log the rescan failure). | `desktop/src/app_lifecycle.rs` |
| F5 Linux rename | Write post-`renameat2` destination identity verification (rename back + refuse on mismatch) and `compile_error!` for non-Linux Unix; record as uncompiled until native Linux run. | `shared/model_paths.rs`, `shared/trash.rs` |
| F6 stdin | Fix now: BOOT receives expected byte length + SHA-256 (argv) and verifies before `exec`; stdin writer errors fail the step. Close during a node op: refuse immediate exit while the maintenance gate is held by node/env work (show "maintenance in progress") rather than orphaning children. `PR_SET_PDEATHSIG` on Linux = write, uncompiled. | `custom_nodes/hook.rs`, `shared/process.rs`, `desktop/src/app_lifecycle.rs` |
| F7 git pins | Fix now: git command specs carry the retained pins (`CommandSpec.retained`). | `custom_nodes/git.rs`, callers in `files.rs` |
| F8 BOOT | Fix now: delete helper globals after `compile`; replace `sys.path[0]` only when it is `''`. | `custom_nodes/hook.rs` |
| F9 uv cwd | Fix now on Windows: pre-consent uv preview (and installs?) run with an app-owned cwd and `-r <pinned path>` (Windows read pin freezes the file); keep stdin + anchored cwd on Linux. Open question: relative local-path requirements then resolve against the requirements file/cwd differently — decide. | `environment/packages.rs`, `custom_nodes/files.rs` guard |
| F10 digest | Fix: `__pycache__` exclusion tests only the relative path. Shipped `__pycache__` in registry archives: refuse? strip? leave? — decide. | `custom_nodes/tree.rs` |
| F11 staging | Fix: staging directories are created create-new and their identity recorded. | `shared/model_paths.rs`, `files.rs` |
| F12 Linux reads | `O_NONBLOCK` + refuse non-regular after open; `sha256_file`/`read_text` read through pinned handles. Linux part uncompiled. | `shared/model_paths.rs`, `custom_nodes/tree.rs` |
| F13 git stderr | Fix: git failures surface a fixed message plus bounded node-relative text; never absolute paths / profile names in IPC. | `custom_nodes/git.rs`, `desktop/src/ipc/node_dto.rs` |
| F14 DOS remap | Residual; document in release acceptance. | — |
| F16 relay auth | Decide: per-download random credential in the child's proxy URL, relay requires matching `Proxy-Authorization`; or record as accepted residual (loopback-only, lifetime-bound, allow-listed hosts). | `models/acquisition/hf_proxy.rs` |
| False refusals | Decide: refuse only name-surrogate reparse tags (`IsReparseTagNameSurrogate`: symlink, mount point/junction, and other surrogates) and keep non-surrogate tags (WOF/compact, OneDrive/cloud placeholders, dedup) as ordinary files/dirs? | `shared/model_paths.rs`, `custom_nodes/tree.rs` |

Sequencing: H after WP4/WP5? or before? H touches `files.rs`, `tree.rs`, `model_paths.rs`, `trash.rs`, `hook.rs`, `process.rs`, `git.rs`, `scan.rs`, `packages.rs`, `app_lifecycle.rs`, `hf_proxy.rs`; WP4 touches `files.rs` recovery heavily.
