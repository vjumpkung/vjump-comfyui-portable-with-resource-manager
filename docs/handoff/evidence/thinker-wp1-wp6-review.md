# Thinker security review — M5 WP1 + WP6 (and B relay)

Opus thinker, read-only, 2026-10-19, on commit `4749d8a` (line numbers refer to it). Nothing compiled or run. Verdict: 2 high, 2 medium; stdin/BOOT deviation **accept with fixes**; WP6 ordering correct.

## Ranked findings

1. **HIGH — Unbounded tree depth → stack overflow / handle exhaustion → crash loop at every startup (WP1 regression; old code used `fs::remove_dir_all`).** `model_paths.rs:329-353` `remove_owned_tree` recurses per directory level (Windows frame holds `ReadDir` + `DirEntry`, ~600 B, 2 handles/level); `model_paths.rs:198-202` every `child()` holds its parent `Arc` (chain as deep as the tree, recursive drop); `tree.rs:185-201,222-227` no component cap, `seen` map stores every prefix (quadratic in name length). Scenario: registry ZIP entry `a/a/…/a/f` with 2k–16k levels (64 KiB name allowed) or a Git repo with 4096 levels extracts fine; abandon after failed preview (`workflow.rs:315`), Cancel (`use_cases.rs:194-201`) or prepare-failure cleanup (`files.rs:383,509,614`) overflows the 2 MiB blocking-thread stack → abort; op stays `Prepared`; startup reconcile re-runs discard (`use_cases.rs:379` → `files.rs:1093-1095`) → crash on every launch. On Linux the fd limit hits first → staging left, every Start blocked by `node_recovery_required`. Separately, a few 64 KiB names under distinct top dirs make `seen` take GiB → OOM before consent. **Fix:** cap components and name length in `extract_zip` (e.g. 64 components); `validate_tree` refuses deeper trees; iterative removal with a heap stack and bounded open handles; collision check without storing every prefix.

2. **HIGH — Windows recycle can permanently delete and report success.** `trash.rs:148-155,165-171` (via `trash.rs:16-19`): `FOF_NOCONFIRMATION` + `FOFX_RECYCLEONDELETE` without nuke protection silently destroys items the Recycle Bin cannot take (USB stick, network share, oversize item, "remove files immediately"), then `!path.exists()` → `Ok`. Update/Uninstall permanently deletes the prior node tree and records `Completed`; violates "never fall back to permanent deletion" (also FR-MODEL-3). **Fix:** `IFileOperationProgressSink::PreDeleteItem` aborts when `TSF_DELETE_RECYCLE_IF_POSSIBLE` is absent; treat as failure → `CleanupPending`, backup kept. Verify natively.

3. **MEDIUM — Registry archive may ship an unreviewed `.git` → command execution during scans (even disabled nodes).** `tree.rs:41-48,185` top-level `.git` skipped by `validate_tree` and digest, accepted by extraction; `scan.rs:87-101` any `.git` → classified Git even with a Registry marker; `files.rs:1003-1018` runs `git status` for every Git-classified node. A `.git/config` `[filter "x"] clean = <cmd>` + index + `.gitattributes` runs `<cmd>` on every startup scan / after-stop rescan, outside ComfyUI, with proxy vars and `SSH_AUTH_SOCK`. `core.fsmonitor=false`/`hooksPath` do not cover filters. **Fix:** refuse `.git` entries in registry archives / non-Git trees; run Git only when the app marker says Git without conflict; skip `.disabled`.

4. **MEDIUM — App cannot close when the after-stop rescan fails.** `app_lifecycle.rs:168-177` + `comfy_runtime/use_cases.rs:152-154`: StopThenExit exits only on `StopComfy` `Ok`; WP6 returns `node_rescan_failed` although ComfyUI verifiably stopped (e.g. `ComfyUI` is a junction to another drive → `NodePaths::open` refuses; non-empty `.vjump-no-hooks`; unresolved recovery op). **Fix:** close path treats `NodeRescanFailed` as stopped and exits (or shows error + offers exit).

5. **LOW–MEDIUM — Linux rename/unlink check identity then act by name, no post-verification.** `model_paths.rs:501-557` (`renameat2`), `:394-410` (`unlinkat`), `trash.rs:99`. Same-user swap of the final entry between check and call → publish/backup/trash a different entry (e.g. a symlink published as `custom_nodes/<name>`). **Fix:** after `renameat2`, `O_NOFOLLOW`-open destination and compare identity; rename back + refuse on mismatch. Also: non-Linux Unix `rename_no_replace` returns `Ok` without renaming → `compile_error!` for those targets.

6. **LOW — Script bytes on stdin are not verified by the child.** `process.rs:479-484`, `hook.rs:22-30`: fire-and-forget `let _ = write_all`; BOOT compiles whatever arrives before EOF. Closing the window during a node op (immediate `Exit`; `app_lifecycle.rs:148-167` only considers ComfyUI) aborts the writer mid-way for a >64 KiB script; on Linux the process group outlives the app and runs a truncated prefix of the reviewed script unsupervised (also orphans uv/hook after the lease is gone). **Fix:** pass expected length + SHA-256 in argv, BOOT verifies before `exec`; writer errors are failures; Linux `PR_SET_PDEATHSIG` or block exit during node ops.

7. **LOW — Git commands do not retain pins after `CleanupFailed`.** `git.rs:118-186`, callers e.g. `files.rs:440-467`: git specs carry no retained pins; surviving git processes may lazily resolve `/proc/<pid>/fd/<n>/.` (`hooksPath`, `-C`) after fd reuse. **Fix:** attach pins to git command specs, or a non-descriptor-anchored `core.hooksPath`.

8. **LOW — BOOT vs `python install.py` differences.** `hook.rs:22-30`: `os`, `sys`, `__main__`, `path`, `source` leak into script globals; `sys.path[0]` overwritten unconditionally (wrong with `-P`/`PYTHONSAFEPATH`); `__loader__` differs; multiprocessing spawn re-reads `__file__` by name. **Fix:** delete helper names after `compile`; replace `sys.path[0]` only when it is `''`.

9. **LOW — Pre-consent uv preview uses the untrusted staging folder as cwd.** `packages.rs:84-91`, `workflow.rs:344-361`: on Windows cwd is in the DLL search order. Windows read pin already freezes the file (`FILE_SHARE_READ`), so `-r <pinned path>` with an app-owned cwd is equivalent; stdin only needed on Linux.

10. **LOW — `tree_digest` gaps.** `tree.rs:96-116`: skips `__pycache__` at any depth (shipped unchecked-hash `.pyc` run unreviewed code); checks components of the absolute path (an install root containing `__pycache__` digests nothing). **Fix:** test only the relative path; refuse/strip `__pycache__` in staged trees.

11. **LOW — Staging creation accepts an existing directory.** `files.rs:451-454,579-582` via `child(…, true)`; "already exists" swallowed at `model_paths.rs:168-171,180-184`. A directory created between check and create is adopted as staging and later deleted. **Fix:** create-new directory and record its identity.

12. **LOW — Linux reads that can block or follow swapped links.** `PinnedDirectory::open` lacks `O_NONBLOCK` (`model_paths.rs:231-239`): a FIFO at a single-file node path blocks a thread forever under the lease (`files.rs:800,1060`). `sha256_file`/`read_text` re-open by path after `lstat` (`tree.rs:73-75,140-146`).

13. **LOW — Raw git error text reaches the UI.** `Failed("Git failed: …")` passes the stderr tail to IPC with only credentials redacted, including absolute paths with the user profile name (`git.rs:183-186`, `node_dto.rs:589`).

14. **LOW / residual — Windows containment is path-based inside pinned parents.** Opens/creates/rename targets/recycle re-resolve DOS paths (`model_paths.rs:164-175,220,510-513`; `trash.rs:18`); pins block rename/replace but not per-session drive-letter remap (`DefineDosDevice`, `subst`, `net use`).

15. **INFO** — `repair_requirements` (`custom_nodes/use_cases.rs:460-478`) returns unguarded `files.path()` joins; must not be wired into Repair as-is (WP5 amendment).

16. **LOW (package B)** — HF relay on `127.0.0.1` has no authentication (`hf_proxy.rs:179,423`): while an HF job runs, any local process/user can tunnel to `*.huggingface.co`/`*.hf.co` through the user's authenticated upstream proxy. Per-download token checked via `Proxy-Authorization` would close it. Host checks, public-IP check, connect-to-checked-address, header validation and timeouts look correct.

## Verified correct
- Unix `pin_absolute` starts at `/`, one component at a time with `openat(O_NOFOLLOW|O_DIRECTORY)`.
- Windows `NodePaths` pins the whole ancestor chain plus `ComfyUI`, `custom_nodes`, `.disabled`, `.vjump-no-hooks` without delete sharing; reparse refused after `OPEN_REPARSE_POINT` open.
- Every node mutation goes through `NodePaths` entries; renames never replace (`ReplaceIfExists=false` / `RENAME_NOREPLACE`) and use an identity-checked handle; no path-based `tree::rename_no_replace` remains.
- Registry archives: create-new pinned file, same handle reused for extraction, identity-checked cleanup.
- Linux trash uses only directory-relative operations; nodes never call `trash::delete(path)`; cross-device fails safely; recycler error → `CleanupPending`, backup kept.
- Guard reads `requirements.txt`/`install.py` once through non-following read pins; digests computed from those bytes; `pin()` compares tree/requirements/hook digests with the plan.
- Hook runs only with consent `run_install_py` and a planned hook; guard lives through install, pin reassertion, hook and final verification; no early drop.
- Cleanup-failure path hands retained resources to a task that drops them only when the Job/process group is empty.
- `--python`, `-c`, uv and git program paths are absolute in release builds.
- WP6: Start try-acquire → node check → inventory → spawn, lease moved into the supervisor; Restart single acquisition; Stop ungated → FIFO-fair acquire → stop again → awaited after-stop; no deadlock found; update-with-stop runs after-stop under its lease; `node_recovery_required`/`node_rescan_failed` not swallowed; startup reconcile + scan before services.
- BOOT: `__name__ == '__main__'`, `argv = ['install.py']`, absolute `__file__`, `sys.path[0]` = node dir, cwd = node dir; `compile` honours encoding cookies/BOMs; `site`/`encodings` imported before `sys.path[0]` is set.
- Git hardening: fsmonitor off, empty hooks path, https only, credential helper cleared, `core.symlinks=false`, no submodules.

## Deviation verdict (stdin/BOOT): accept with fixes 6, 8, 9
Executed bytes are bound to the reviewed digest at guard time; emulation is faithful for common scripts. Nested `-r`/`-c` includes, local wheels and imports from `install.py` are still read by path, protected only by the tree digest at guard time — consistent with the documented guarantee.

## Residual risks needing native verification
- Linux: none of the Linux code has compiled or run — run the 14 named tests natively; `renameat2` on musl; `/proc/<pid>/fd` under Flatpak/AppArmor/SELinux; whether uv/git resolve anchored paths to real paths; uv 0.13 `-r -` relative includes on Linux; `setsid` children escaping the group.
- Windows: file-symlink cases skipped (privilege) — rerun elevated/Developer Mode; recycle behaviour on USB/network/oversize (finding 2); drive-letter remap (14).
- Both: deep-tree stack/handle exhaustion (1).
- False refusals: reparse check by attribute refuses OneDrive placeholders, `compact`/WOF-compressed files and dedup volumes; Linux roots with symlinked ancestors are refused → every node op and rescan fails (and triggers finding 4).
