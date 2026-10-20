# M5 package H — hardening: advisor-approved plan (gpt-6.1-sol xhigh, read-only, 2026-10-19)

Input: docs/handoff/evidence/thinker-wp1-wp6-review.md and docs/handoff/plans/m5-h-hardening-draft.md. Accepted by main without changes.

**No-go on the draft as written. Go for the corrected implementation sequence below.** Release acceptance remains conditional on native Linux checks and the Windows recycle tests.

This review was read-only. I changed no files and ran no builds or tests. HEAD is `4749d8a`; evidence below uses current working-copy lines where WP2/WP3 changed them. Package P’s unresolved `state.rs` merge is an integration prerequisite, not an additional finding.

1. **Finding decisions**

| Finding | Verdict and evidence | Decision |
|---|---|---|
| **F1: depth/resource exhaustion** | **Confirmed, high.** Recursive cleanup at [model_paths.rs:329](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/shared/model_paths.rs:329); recursive parent ownership at `:198`; prefix-string accumulation at [tree.rs:185](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/custom_nodes/tree.rs:185). Component length is already capped by [archive.rs:92](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/shared/archive.rs:92), but depth and total path length are not. | Fix before WP4. Bound admission, traversal, cleanup and planning memory. |
| **F2: permanent deletion through recycle** | **Confirmed safety gap, high; destructive cases were not reproduced here.** [trash.rs:148](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/shared/trash.rs:148) supplies no rejecting sink; `:165` accepts disappearance. Model deletion uses this same helper at [file_ops.rs:200](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/models/library/file_ops.rs:200). | Fix for both nodes and models. A fake recycler test alone is insufficient. |
| **F3: archive `.git` and scan execution** | **Confirmed, medium.** The walker excludes root `.git` at [tree.rs:47](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/custom_nodes/tree.rs:47); Git metadata overrides classification at [scan.rs:87](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/custom_nodes/scan.rs:87); every Git-classified directory can invoke status at [files.rs:1039](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/custom_nodes/files.rs:1039). | Reject archive `.git`; preserve marker provenance; safely inspect ordinary manual clones. A marker alone does not make executable Git configuration safe. |
| **F4: cannot close after rescan failure** | **Confirmed but downgraded.** The first Stop-and-exit request stays open at [app_lifecycle.rs:174](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/desktop/src/app_lifecycle.rs:174). It is **not permanently unclosable**: subsequent close sees Stopped and chooses Exit at [close.rs:25](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/application/src/comfy_runtime/close.rs:25). | Treat only `NodeRescanFailed` as a nonfatal shutdown warning, while retaining verified-stop and maintenance protection. |
| **F5: Linux final-entry race** | **Confirmed residual.** `renameat2` follows an identity check at [model_paths.rs:501](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/shared/model_paths.rs:501); unlink similarly acts by name at `:397`. The unsupported-Unix fallthrough also exists. | Write the unsupported-target guard now. Defer syscall changes to a native Linux package. Reject the draft’s unconditional “rename back”: it can move another substituted entry. |
| **F6: partial stdin / close during maintenance** | **Confirmed.** Detached writer ignores errors at [process.rs:479](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/shared/process.rs:479); BOOT accepts arbitrary EOF at [hook.rs:22](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/custom_nodes/hook.rs:22); idle-runtime Exit ignores maintenance at [app_lifecycle.rs:167](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/desktop/src/app_lifecycle.rs:167). | Verify length/hash, supervise stdin, refuse idle exit during maintenance. Also retain the maintenance lease through surviving-process cleanup. |
| **F7: Git loses pins** | **Confirmed.** [git.rs:122](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/custom_nodes/git.rs:122) builds commands without retained resources. [process.rs:403](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/shared/process.rs:403) deliberately does nothing for empty retention. | Attach complete pin ownership to every Git subprocess, including read-only status and recovery inspection. |
| **F8: BOOT compatibility** | **Confirmed, low.** Helper globals and unconditional path replacement are visible at [hook.rs:23](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/custom_nodes/hook.rs:23). | Use a local bootstrap function, clean `__main__`, respect safe-path mode. Multiprocessing reopening `__file__` remains an explicit limitation. |
| **F9: uv cwd** | **Confirmed exposure, low; DLL execution was not demonstrated.** Guarded cwd is assigned at [packages.rs:84](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/environment/packages.rs:84). The existing compatibility fixture exercises relative includes and wheels at [tests.rs:3699](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/custom_nodes/tests.rs:3699). | **Keep cwd and stdin semantics.** Remove cwd from Windows default DLL lookup through a one-time process policy; do not introduce a requirements rewriter. |
| **F10: digest/cache exclusions** | **Confirmed.** Absolute ancestors affect exclusion at [tree.rs:110](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/custom_nodes/tree.rs:110); incoming caches are accepted. | Test relative components only. Refuse shipped `__pycache__` in registry archives and fresh Git checkouts; retain exclusion for runtime-generated caches in installed trees. |
| **F11: adopted staging** | **Partially confirmed.** Git install/update use adopting `child(..., true)` at [files.rs:458](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/custom_nodes/files.rs:458) and `:586`. Registry staging already uses exclusive creation and records identity at `:371–373`. | Add a distinct create-new-directory helper; use it consistently. Do not conflate layout creation with operation ownership. |
| **F12: blocking/following reads** | **Confirmed in the stated remaining paths.** `open()` lacks `O_NONBLOCK` at [model_paths.rs:231](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/shared/model_paths.rs:231); tree reads reopen names at [tree.rs:73](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/custom_nodes/tree.rs:73) and `:140`. `open_read_pin` and `open_entry` already use `O_NONBLOCK`. | Add the missing flag; move tree/provenance reads to pinned-parent handles. |
| **F13: raw Git errors** | **Confirmed, low.** [git.rs:190](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/custom_nodes/git.rs:190) embeds stderr; [node_dto.rs:590](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/desktop/src/ipc/node_dto.rs:590) forwards it. | Fixed public failure messages. Preserve only separately constructed, bounded repository-relative refusal status. |
| **F14: DOS-device remapping** | **Confirmed residual.** Windows uses stored DOS paths in [model_paths.rs:164](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/shared/model_paths.rs:164), `:510`, and [trash.rs:18](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/shared/trash.rs:18). | Document the same-user remapping boundary. Do not claim complete Windows namespace isolation. |
| **F15: repair paths** | **Confirmed informational; already covered by WP5.** [use_cases.rs:460](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/application/src/custom_nodes/use_cases.rs:460) returns informational paths without execution guards. | Remove it under WP5, as already specified in [wp5.md:13](D:/vjump/vjump-comfyui-portable-with-resource-manager/docs/handoff/prompts/session3/wp5.md:13). |
| **F16: unauthenticated HF relay** | **Confirmed, low residual.** Loopback listener at [hf_proxy.rs:179](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/models/acquisition/hf_proxy.rs:179); unauthenticated connect at `:423`. | Accept as an explicit Release-1 residual for this pass. Do not add a proxy-URL secret or change the child credential policy during M5 hardening. |

**Reparse policy:** retain rejection of **all reparse points** for this release. Do not blanket-allow non-name-surrogate tags. The name-surrogate bit identifies namespace redirection; its absence is not a general safety certification for arbitrary providers. Ordinary NTFS compression need not involve a reparse point, so the review’s compression claim is too broad. WOF/cloud/dedup compatibility remains unsupported where the actual entry is reparse-backed. Archive rejection remains strict under NFR-SEC-4. [Microsoft’s tag documentation](https://learn.microsoft.com/en-us/windows/win32/fileio/reparse-point-tags)

2. **Sequence and package boundaries**

Split H into three independently checked packages, plus a deferred Linux package:

**Finish/verify P → H1 filesystem → H2 execution/Git → H3 Windows recycle/close → WP4 → WP5 → merge E1 → E2 → E3 → I → R and documentation.**

Use one writer for each sequential package. Do not overlap H, WP4 or WP5 in `files.rs`, application node workflows, environment ports or desktop composition. Checkpoint each verified package.

H1 deliberately precedes WP4 because it changes the filesystem primitives and snapshot APIs that recovery must consume. H2 settles retention contracts before WP5 introduces another requirements consumer. H3 waits for P’s desktop merge.

Do not postpone both high findings until after WP4/WP5.

3. **H1 — bounded, pinned filesystem work**

**Files:**  
`crates/infrastructure/src/shared/model_paths.rs`;  
`crates/infrastructure/src/custom_nodes/{tree,paths,files,scan,provenance,tests}.rs`.

Implement these decisions:

- In `tree.rs`, define:
  - `MAX_NODE_PATH_COMPONENTS = 64`
  - `MAX_NODE_RELATIVE_PATH_BYTES = 1024`
  - `MAX_ARCHIVE_NAME_BYTES = 1024`
  - `MAX_ARCHIVE_PLANNED_NAME_BYTES = 8 * 1024 * 1024`
  - retain 255 UTF-8 bytes per component and existing archive entry/expansion limits.
- Apply archive limits during the **complete planning pass, before output creation**. Count slash separators in normalized relative length. Bound original decoded names too, so discarded `./` or repeated separators cannot evade the planning budget.
- Replace full-prefix keys with a trie represented by a map keyed by `(parent_id, folded_component)`. Store component spelling, node ID, kind and whether an explicit entry occurred. Reject file/directory conflicts, differently spelled case aliases and duplicate explicit entries. Permit exact reuse of an implicit directory.
- Replace recursive `remove_owned_tree` with iterative postorder traversal. At most 64 directory frames and bounded iterator handles may be open. On a legacy tree exceeding the bound, return refusal without descending further. Preserve the remainder; startup must report unresolved recovery rather than crash.
- Use one bounded pinned traversal for validation and digests. Enumerate names through a retained directory; open children through `child`/`open_entry`; read bytes through `open_read_pin`. Sort names per directory for deterministic traversal. Do not retain every directory in the tree simultaneously.
- Change internal snapshot helpers to accept pinned directories rather than reconstructing authority from `Path`:
  - `read_tree(directory: &Arc<PinnedDirectory>, git_node: bool)`
  - `validate_tree(directory: &Arc<PinnedDirectory>, git_node: bool)`
  - `tree_digest(directory: &Arc<PinnedDirectory>)`
  - leaf text/hash helpers take a pinned parent plus `SafeFileName`.
- Update scan/provenance readers accordingly. Git refs containing multiple components must traverse validated components from the pinned `.git` directory. Do not call `pin_absolute` on `/proc/.../fd/...` paths.
- Validate root `.git` itself before excluding its contents. Permit it only for Git trees. Reject nested `.git` and `.git` in non-Git trees.
- Reject any case-insensitive `.git` or `__pycache__` component in registry archives. Reject `__pycache__` in fresh staged Git checkouts, without deleting tracked files. Installed-tree cache exclusions examine **root-relative components only**; validation still checks cache entries for links/special files.
- Add `PinnedDirectory::create_new_child`. Existing entry → `AlreadyExists`; never adopt it. Use it for operation staging; keep `child(..., true)` for reusable layout directories.
- Record each successfully opened staging directory’s `platform_id`; failure cleanup uses the recorded identity. If creation/opening fails before ownership is established, do not delete that name. This fixes adoption; it does not make `mkdir` followed by open an atomic inode-creation primitive.
- Add `O_NONBLOCK` to Unix `PinnedDirectory::open`; retain the post-open regular-file check.
- Add `#[cfg(all(unix, not(target_os = "linux")))] compile_error!(...)` so unsupported Unix cannot report a successful no-op rename.

**Errors:** limits, links, forbidden metadata and ownership mismatch → `UnsafeSource` / `node_unsafe_source`; ordinary I/O → `Filesystem` / `node_filesystem`. Recovery preserves uncertain copies.

**Required tests and assertions:**

| Exact test name | Assertions |
|---|---|
| `registry_archive_depth_is_rejected_before_writing` | A 65-component entry returns `UnsafeSource`; staging contains no extracted entries. |
| `node_path_limits_accept_boundaries_and_reject_excess` | Exercise 64/65 components, 255/256-byte components and 1024/1025-byte paths. |
| `archive_planning_has_a_total_name_budget` | Many distinct valid names exceed the 8 MiB budget before writing. |
| `archive_component_trie_rejects_case_and_kind_collisions` | Cover implicit parent reuse, case aliases, duplicate explicit entries and file/parent conflicts. |
| `git_tree_depth_is_rejected_without_recursive_cleanup` | A too-deep checkout is refused; cleanup terminates without panic or stack overflow. |
| `owned_tree_cleanup_uses_a_bounded_stack` | Remove a 64-level tree on a small-stack thread; no external sentinel changes. |
| `legacy_overdeep_staging_is_preserved_without_startup_crash` | Cleanup refuses; recovery remains unresolved; repeated reconciliation terminates. |
| `registry_archives_reject_git_and_shipped_pycache` | Nested and case variants fail before extraction. |
| `fresh_git_checkouts_reject_shipped_pycache` | Refusal preserves checkout contents; no `git clean`/reset occurs. |
| `digest_exclusions_use_only_node_relative_components` | An install ancestor named `__pycache__` does not suppress node content; installed cache changes remain excluded. |
| `staging_create_new_refusal_preserves_foreign_directory` | Inject creation between existence check and create; return refusal and preserve its sentinel. |
| `tree_reads_use_pinned_nonfollowing_handles` | Substitute leaf/parent at controlled boundaries; no outside bytes are consumed. |
| `single_file_fifo_is_refused_without_blocking` | Linux-only: FIFO open returns promptly and does not retain the gate indefinitely. |

Preserve existing registry ownership and substitution tests.

4. **H2 — execution ownership, BOOT and Git safety**

**Files:**  
`crates/application/src/shared/{mod,gate}.rs`, new `shared/retained.rs`;  
`crates/application/src/environment/{ports,execute,packages}.rs`;  
`crates/application/src/custom_nodes/{ports,workflow,use_cases,tests}.rs`;  
`crates/infrastructure/src/shared/{gate,process}.rs`, `shared/process/{windows,tests}.rs`;  
`crates/infrastructure/src/environment/{exec,packages}.rs` and affected context-bearing adapter helpers;  
`crates/infrastructure/src/custom_nodes/{git,files,hook,tests}.rs`;  
exhaustive `RunError` consumers in runtime/HF adapters; constructor/fake tests.

**Retention contract**

The draft’s gate check is insufficient after `CleanupFailed`: the caller can return and drop its lease while the resource-retention task remains alive.

- Move the existing framework-free `Retained` type into application `shared`; re-export it from environment to limit import churn.
- Add `Retained::merge`; `CommandSpec::retain` must merge ownership rather than replace previously retained pins.
- Make `MaintenanceLease: Send + Sync` expose `fn retained(&self) -> Retained`.
- `TokioLease` owns an `Arc<OwnedMutexGuard<()>>`; its retention token clones that same ownership. The mutex releases only after the last owner drops.
- Add `StepContext.retained`. Populate it from the held lease in node preparation/commit and environment execution/package mutation.
- `run_step` merges context retention into the command before running it.
- Propagate retention into node `git_status`, `publish`, `recover` and `scan` port calls, which can launch Git without an existing step context. Update their callers and fakes.
- Give maintenance package-list/pin-check calls a retained context too; public nongated list calls may use none. Context-bearing adapter helpers using `run_quiet` must attach the same token.
- Update fake leases so retention delays release; do not implement fake retention as an empty token.

This is an ownership change, with no Tokio types crossing into application.

**Git policy**

- `scan::classify` preserves Registry/Manual marker provenance and marks contradictory Git presence as conflict. For a Git marker, observed revision may refresh only when repository identity agrees.
- Do **not** skip disabled clones merely because they are disabled: FR-NODE-5 includes them.
- Both marked and unmarked Git clones may obtain status only through a conservative configuration preflight.
- For Git commands, disable system/global configuration using an app-owned empty config and `GIT_CONFIG_NOSYSTEM=1`. Retain that file and the hooks directory.
- Before status/inspection/update, use the nonexecuting configuration query `git config --local --no-includes --null --list`. Refuse repositories containing:
  - `filter.*` configuration;
  - `include.path` or `includeIf.*.path`;
  - enabled worktree configuration or a redirected worktree.
- Keep existing hooks/fsmonitor/protocol/credential restrictions. Status explicitly ignores submodules. Read-only inspection does not inherit proxy variables or `SSH_AUTH_SOCK`.
- Unsafe configuration during automatic scanning leaves `dirty = None`; it must never imply clean. Explicit status/update returns fixed `GitRefused` explaining that repository configuration requires external inspection. Ordinary manual clones still receive remote, branch, commit and dirty state.
- Bind `GitNodes` command construction to retained resources covering `NodePaths`, the selected directory, `.git`, hooks/config and the lease. Nested reads use the same bound ownership.
- Replace stderr-derived `Failed` messages with fixed descriptions. Continue internal transient classification. `GitRefused.status` remains bounded, redacted, repository-relative status or a fixed explanation.

Git clean/process filters are executable configuration; disabling hooks and fsmonitor alone does not disable them. [Git configuration documentation](https://git-scm.com/docs/git-config)

**stdin and BOOT**

- Replace the detached stdin writer with a future owned by `run_inner`.
- Select writer completion alongside process exit, cancellation, timeout and output pumping.
- Add `RunError::InputFailed { tail }`. Failed/incomplete delivery cannot become success merely because the child exits zero.
- On input failure, terminate and drain the process tree through the existing cleanup path. `CleanupFailed` takes precedence and retains resources. Cancellation/timeouts retain their existing classifications.
- Map input failure to a fixed `ProvisionError::Failed("The command input could not be delivered.")`; hook callers map it to `DependenciesFailed`. Update exhaustive runtime/HF matches without exposing raw I/O text.

BOOT receives expected byte length and SHA-256 as arguments derived from the guarded bytes.

- Bootstrap imports/helpers live inside one function.
- Temporarily remove an initial empty `sys.path` entry before importing hashing/bootstrap modules, so a node’s `hashlib.py` cannot intercept verification.
- Read at most expected length + 1; require exact length and SHA-256 before compiling or executing.
- Compile bytes, preserving encoding-cookie/BOM behavior.
- Set `argv` to `['install.py']`, absolute `__file__`, `__package__ = None`, `__spec__ = None`, and a matching source-file loader.
- Remove the bootstrap function from `__main__` before execution.
- Restore the script-directory entry only when the original path entry was empty. Do not override `-P`/safe-path behavior.

**Windows DLL policy / F9**

**Correction (2026-10-20, D1):** The propagation premise below was false: `SetDllDirectoryW("")` applies to the manager process only; Windows does not pass it to children. D1 replaces it with this invariant: "Before exact-source consent, on Windows, no tool runs with newly acquired node content as its working directory while that content's top level holds an entry with a native-image extension; uv's interpreter query must not import Python modules from that folder." Installed and user-placed nodes are not re-checked. The implementation and any remaining gaps are recorded in `docs/handoff/evidence/s6-d1-implementation.md`.

Keep guarded requirements on stdin and keep the node directory as cwd on both platforms. This preserves current relative include/local-path semantics.

Add a once-only Windows initialization using `SetDllDirectoryW(L"")`, before worker/tool spawning. Call the same initializer defensively from Windows runner spawning for standalone tests. Failure prevents tool launch; do not temporarily toggle this process-wide setting around individual commands.

Add `Win32_System_LibraryLoader` to the existing `windows-sys` features. Microsoft documents both removal of cwd and propagation of this search policy to child processes. [SetDllDirectoryW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-setdlldirectoryw)

**Additional consent check**

The existing `--only-binary :all:` assumption needs a regression test: uv documents that editable requirements can still invoke build backends. Before consent, editable/source-directory requirements—including nested includes—must be refused without running their backend. Unknown/unhandled preflight syntax fails closed; never retry with source builds enabled. This is required by NFR-SEC-3, not an optional feature. [uv CLI documentation](https://docs.astral.sh/uv/reference/cli/)

**Required tests:**

| Exact test name | Assertions |
|---|---|
| `maintenance_lease_survives_cleanup_failure` | Drop caller/context after injected cleanup failure; gate remains busy until the process-empty signal. |
| `command_retention_merges_lease_and_filesystem_pins` | Neither resource is released early; both release after verified teardown. |
| `git_commands_retain_all_descriptor_owners` | Exercise resolve, clone, status, apply and recovery; each command carries required owners. |
| `registry_git_conflict_never_invokes_git` | Registry marker plus `.git` remains Registry/conflicted; Git runner receives zero calls. |
| `manual_and_disabled_git_nodes_receive_safe_status` | Ordinary clones receive accurate metadata and dirty state in both locations. |
| `git_scan_never_executes_repository_filters` | Real malicious clean/process filter writes a sentinel if invoked; sentinel remains absent for marked, manual and disabled cases. |
| `git_inspection_refuses_includes_and_worktree_redirection` | Fixed refusal; no status/fetch/apply call follows. |
| `git_failures_do_not_expose_stderr_or_absolute_paths` | Stderr containing profile paths and credentials never reaches error/IPC messages. |
| `stdin_delivery_failure_cannot_report_success` | Child closes stdin and exits zero; runner returns `InputFailed`, after cleanup. |
| `stdin_writer_obeys_cancellation_and_timeout` | Child never reads; runner terminates promptly and leaves no writer task. |
| `guarded_hook_rejects_truncated_or_changed_input_before_exec` | Truncated, appended and same-length changed bytes fail; script sentinel remains absent. |
| `guarded_hook_has_a_clean_main_namespace` | Bootstrap names absent; expected `__main__` attributes match direct invocation. |
| `guarded_hook_respects_safe_path_mode` | With safe-path mode, BOOT does not insert the node directory. |
| `windows_children_exclude_cwd_from_default_dll_search` | A uniquely named invalid DLL existing only in cwd is not selected by a helper child’s default lookup. |
| `node_preview_never_executes_editable_build_backend` | Root/nested editable and dynamic local-source fixtures create no backend sentinel and produce no environment mutation. |

**Test correction (2026-10-20, D1):** The DLL test above proves process-local initialization only. Rename it to `windows_dll_initializer_is_process_local`; its helper must observe error 193 before initialization and 126 after. The D1 native-image admission checks and the live uv interpreter-isolation test replace the inherited-child-policy acceptance premise.

Re-run `guarded_requirements_preserve_relative_includes_and_local_paths` with **pinned uv 0.13.1**, identified at [pins.rs:14](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/environment/pins.rs:14). Earlier system-uv 0.13.0 evidence is not evidence for that exact binary.

5. **H3 — recycle guarantees and close behavior**

**Files:**  
`crates/infrastructure/src/shared/trash.rs`, new `shared/trash/tests.rs`;  
root and infrastructure `Cargo.toml`, `Cargo.lock`;  
`crates/desktop/src/{app_lifecycle,state,lib}.rs`, affected close-command tests;  
node/model recycler integration tests.

**Windows recycle**

Implement `IFileOperationProgressSink` and pass it to `DeleteItem`.

- `PreDeleteItem`: missing `TSF_DELETE_RECYCLE_IF_POSSIBLE` → record refusal and return `E_ABORT`.
- Record per-item completion failure and whether a recycled destination was supplied.
- Success requires successful operation completion, no abort/refusal, positive per-item recycle completion, and source absence confirmed specifically as `NotFound`.
- Missing callbacks, inaccessible source status or mere disappearance are failures.
- Do not introduce OS permanent-delete confirmation as an alternative path. Permanent model deletion remains the separately reviewed application action.

A failing `PreDeleteItem` cancels the operation. However, the flag’s documented meaning is “if possible”; native tests are required before treating the combined implementation as proven. [PreDeleteItem](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ifileoperationprogresssink-predeleteitem), [transfer flags](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/ne-shobjidl_core-_transfer_source_flags)

**Dependencies are resolved:**

- Existing [Cargo.toml:54](D:/vjump/vjump-comfyui-portable-with-resource-manager/Cargo.toml:54) already enables `Win32_Foundation`, `Win32_System_Com`, `Win32_UI_Shell`.
- Cached `windows 0.62.2` contains the sink interface, implementation trait and transfer flag.
- There is no `implement` feature to enable.
- The implementation macro emits `::windows_core`; add direct, Windows-targeted `windows-core = "=0.62.2"` with `std`.
- That exact package and `windows-implement` already exist at [Cargo.lock:6385](D:/vjump/vjump-comfyui-portable-with-resource-manager/Cargo.lock:6385). Regenerate dependency edges without unrelated version updates.

**Failure behavior:** node backup remains and operation becomes ordinary `CleanupPending`; model deletion reports existing `TrashFailed`, retains the catalog entry, and offers the existing separately authorized permanent-delete plan.

**Close policy**

Use the existing dialog plugin for a native informational message; no new IPC command or capability is necessary.

- Closing while ComfyUI is stopped and maintenance is active **refuses that close attempt**, keeps the manager visible and says: “Maintenance is in progress. Wait for it to finish or cancel it before closing.”
- No automatic cancellation, force exit or queued exit.
- Acquire the gate before the final stopped-state check. Keep the acquired exit lease in `CloseCoordinator` until actual process exit, preventing another operation from starting between check and exit.
- For StopThenExit, allow `Ok` or `RuntimeError::NodeRescanFailed` to proceed to that final guarded stop check. Log the latter as a warning.
- Other stop failures keep the app open.
- Preserve the current keep-running-headless behavior for a genuinely running supervised ComfyUI.
- Recheck at action execution, including stale dialog responses and the headless-stop exit path; a decision made earlier is not authorization to bypass a newly active maintenance operation.
- Expose the existing shared gate in `Services`; do not construct a second gate.

**Required tests:**

| Exact test name | Assertions |
|---|---|
| `windows_recycle_sink_rejects_permanent_delete_flags` | Missing recycle flag returns failing HRESULT; no success state is recorded. |
| `windows_recycle_sink_refusal_preserves_the_item` | Run actual `IFileOperation` with a test-injected sink refusal; source and sentinel survive. |
| `windows_recycle_requires_positive_completion` | Aborted, failed or missing callback states cannot report success from disappearance. |
| `node_recycle_refusal_retains_cleanup_pending_backup` | Backup contents survive; verified publication stays intact; ordinary cleanup does not block Start. |
| `model_recycle_refusal_preserves_catalog_and_requires_separate_permanent_plan` | No catalog removal or permanent deletion occurs. |
| `stop_then_exit_accepts_only_rescan_failure_after_verified_stop` | Rescan failure permits guarded exit; termination/ownership failures do not. |
| `close_during_maintenance_keeps_the_manager_open` | Cover node preparation, hook, environment work and retained cleanup failure; no exit/window destruction. |
| `exit_lease_prevents_new_maintenance_before_process_exit` | A competing acquire remains blocked after exit is requested. |
| `stale_close_choice_cannot_bypass_maintenance` | A stale Stop/Keep-running response cannot destroy the final window during maintenance. |

Native recycle acceptance must additionally exercise a non-recyclable volume/location, disabled recycle configuration and an oversized item. Record unavailable scenarios as open evidence, not passing tests.

6. **Linux package and residual boundaries**

**Write now, explicitly uncompiled:** the single missing `O_NONBLOCK` flag, unsupported-Unix compile guard, and Linux test definitions accompanying the portable refactor.

**Defer until a native Linux environment exists:**

- Post-`renameat2` destination inspection using nonfollowing `O_PATH` opening and `(device, inode, type)` comparison.
- On mismatch: return refusal, preserve surviving entries and the nonterminal journal. **Do not blindly rename the destination back.**
- Update trash handling so a mismatched placement cannot count as a successful recycle or lose its recovery record.
- Any quarantine/unlink redesign.
- `PR_SET_PDEATHSIG`: do not write this uncompiled. It does not by itself supervise descendants, and its parent-thread semantics need testing with Tokio’s worker threads.

Native tests to add/run:

- `linux_rename_postcheck_detects_source_substitution`
- `linux_rename_mismatch_preserves_entries_and_blocks_recovery`
- `linux_trash_mismatch_never_reports_success`
- `linux_cleanup_never_follows_substituted_links`
- `linux_guarded_inputs_survive_parent_rename`
- `linux_cleanup_failure_retains_lease_and_pins`

Assert source/destination identities, external sentinels, returned errors and journal blocking—not merely that a syscall failed.

A postcheck detects some races; it does **not** create atomic by-handle rename/unlink semantics. Same-user mutation of tree contents/final names and Windows DOS remapping remain explicit boundaries. Neither platform should be advertised as a sandbox or universally “race-free.”

F16 remains documented: during an HF attempt, another local process/user can use the allow-listed relay, including the configured upstream proxy’s authority. It does not expose the destination TLS token directly. Revisit authentication separately with pinned HF/Xet compatibility evidence and an explicit secret-transport design.

7. **WP4/WP5 amendments and success gate**

Keep the approved WP4 recovery table and all named tests unchanged in intent. Its implementations must consume H1’s bounded pinned snapshots and H2’s retention contract. Preserve its distinction between ambiguous recovery and ordinary recycle failure. [Approved plan:216](D:/vjump/vjump-comfyui-portable-with-resource-manager/docs/handoff/plans/m5-backend-approved-plan.md:216)

For WP5:

- Choose `NodeRequirementSet.file: Option<GuardedRequirements>` carrying the protected bytes and resource token; do not return a bare path.
- Propagate the execution lease token through requirements-provider scans and dependency subprocesses.
- Merge file retention with step/lease retention.
- Keep fatal node-requirement failure in every replacement mode, per-environment/content outcomes, and no automatic hook replay.
- Remove `repair_requirements`; do not wire it temporarily.

For each H checkpoint, the implementing coder—not this advisor turn—runs:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -j 4 -- -D warnings
cargo test --workspace --locked -j 4 --no-fail-fast
node scripts/check-arch-deps.mjs
```

Run bindings export and frontend lint/typecheck/tests when contracts or generated bindings change. Use one heavy build at a time in this working copy.

Final acceptance requires:

- All named hardening tests actually execute on their applicable platform.
- Existing archive, recovery, model deletion, lifecycle and guarded-input tests remain passing.
- No source/configured command executes before the required consent.
- No cleanup path adopts foreign staging or silently permanently deletes.
- Guards **and leases** survive failed subprocess cleanup.
- WP4/WP5 tests pass on the integrated tree.
- R records native Linux, NVIDIA, DLL compatibility, recycle and packaging evidence accurately; README/documentation do not convert unverified scenarios into completed claims.

**No indispensable user-only questions.** All choices above are advisor decisions within the existing scope.
