# M5 WP1 scope-gap decision (advisor gpt-6.1-sol, 2026-10-17, read-only review)

Accepted by main. Extends WP1 of `m5-backend-approved-plan.md` (item 5: downloads and protection through subprocess completion) and amends WP5 (`NodeRequirementSet.file` must keep its guard or a protected snapshot alive through provisioning).

**Go for a sequential “finish WP1 → WP2+WP3” coder run. No-go for accepting WP1 as complete, or carrying its current path boundary into WP4/WP5.** Both gaps fall within approved WP1 item 5; extending its file list requires no new user decision. The approved plan explicitly includes downloads and protection through subprocess completion. [Approved plan:84](../../../docs/handoff/plans/m5-backend-approved-plan.md:84)

All three questions are **AI-decidable**. No indispensable user questions.

1. **Q1 — Extend WP1 to `registry.rs`; pass an already-open file.**

   The registry does **not** call `Downloader::fetch`. It uses `Downloader::client()` and implements its own streaming loop, including hashing, size limits, cancellation, timeout, truncation detection and `sync_all`. Only its destination opening needs to change. [registry.rs:164](../../../crates/infrastructure/src/custom_nodes/registry.rs:164)

   Minimal signature:

   ```rust
   pub async fn download(
       &self,
       url: &str,
       destination: &mut std::fs::File,
       cancel: &CancelToken,
   ) -> Result<Artifact, NodeAppError>;
   ```

   In `prepare_registry`, create the archive atomically through:

   ```rust
   zip.0.open(&zip.1, true, true)
   ```

   That API already provides create-new, non-following opening. A new `PinnedDirectory::create_new` wrapper or writer callback adds no necessary capability. [model_paths.rs:204](../../../crates/infrastructure/src/shared/model_paths.rs:204)

   **Keep the same file handle through extraction.** Moving it into the blocking extraction closure avoids replacing the write race with a download-to-extraction reopen race: the current code reopens the archive by name. Retain `NodePaths` through cleanup, as it already does. [files.rs:279](../../../crates/infrastructure/src/custom_nodes/files.rs:279)

   Preserve all existing transfer behavior and leave shared `Downloader::fetch` unchanged. On create-new refusal, preserve the existing entry. Review the outer unconditional `discard_staging` error path so it cannot delete a foreign entry that this attempt never created. [files.rs:674](../../../crates/infrastructure/src/custom_nodes/files.rs:674)

2. **Q2 — Prefer a typed port guard, with stable execution inputs. A guard holding ordinary paths is insufficient.**

   Choose **(b), using a `NodeLocationGuard` trait**, rather than `Box<dyn Any + Send>`. Application-defined resource-lifetime ports fit ADR-0002; concrete handles remain in infrastructure. `Any` provides no useful containment contract. [ADR-0002:25](../../../docs/adr/0002-clean-architecture-ddd.md:25)

   Suggested boundary:

   ```rust
   pub trait NodeLocationGuard: Send + Sync {
       fn directory(&self) -> &Path;
       fn requirements_file(&self) -> Option<&Path>;
       fn install_script(&self) -> Option<&Path>;
       fn snapshot(&self) -> &TreeSnapshot;
   }

   // Added to NodeFiles:
   async fn pin_location(
       &self,
       root: &Path,
       node: &InstalledNode,
       staged: Option<&NodeId>,
   ) -> Result<Box<dyn NodeLocationGuard>, NodeAppError>;
   ```

   Change the hook boundary to consume the guarded location:

   ```rust
   async fn run(
       &self,
       root: &Path,
       env: &PythonEnvironment,
       location: &dyn NodeLocationGuard,
       ctx: &StepContext,
   ) -> Result<(), NodeAppError>;
   ```

   The concrete guard retains the full parent chain, the node directory itself, and the selected regular-file inputs. Its snapshot binds those inputs to the reviewed requirement/hook digests. Workflow holds it across preview, installation, hook execution and their awaited cleanup. Acquire the live guard **after publication**, so Windows directory pins do not obstruct publication.

   Cover **preview as well as commit**: both currently pass ordinary requirements paths. The hook additionally performs a check followed by pathname execution. [workflow.rs:307](../../../crates/application/src/custom_nodes/workflow.rs:307), [workflow.rs:687](../../../crates/application/src/custom_nodes/workflow.rs:687), [hook.rs:33](../../../crates/infrastructure/src/custom_nodes/hook.rs:33)

   Platform requirements:

   - **Windows:** retain directory handles without `FILE_SHARE_DELETE`. Selected requirements/script read handles must also prevent replacement while permitting uv/Python readers. Existing `PinnedDirectory::open` requests DELETE access and permits delete sharing; do not assume it supplies this read-consumer contract.
   - **Linux:** retain handles **and actually use descriptor-anchored inputs**. The existing `anchored_path()` uses `/proc/<parent-pid>/fd/<fd>/.`, which avoids re-resolving renamed ancestors while the parent retains the descriptor. A retained directory plus its original pathname provides no such guarantee. [model_paths.rs:145](../../../crates/infrastructure/src/shared/model_paths.rs:145)
   - **Final files:** anchoring only the directory does not prevent replacement of `requirements.txt` or `install.py`. Consume the selected opened file, or a protected snapshot made from it. Passing `directory.join("install.py")` after validation leaves another check/use gap.

   **Evaluate (a) as a valid mechanism, but insufficient as stated.** Infrastructure hook-local pins can work, and requirements copied from an opened handle can work. However, independently reopening loses continuity with the reviewed location, and copying only root `requirements.txt` can change relative `-r`, `-c` and local-path semantics. Descriptor-backed script execution must likewise preserve hook cwd, imports and filename behavior. These need compatibility tests against the pinned uv/Python; neither shortcut is proven by current tests.

   **Reject (c).** NFR-SEC-5 contains no exception allowing an accepted subprocess-path race. Documenting that limitation cannot establish compliance. The separate statement that a venv is not a sandbox remains applicable to third-party code. [REQUIREMENTS.md:473](../../../docs/REQUIREMENTS.md:473)

   Proposed `NodeFiles` documentation:

   > `path()` returns an informational location and grants no filesystem authority. Execution consumers must obtain and retain a guarded location through subprocess teardown and cleanup. Windows directory pins prevent rename/replacement through delete-sharing restrictions. Linux pins retain directory identity but do not freeze pathnames or contents; consumers must use descriptor-anchored inputs or protected snapshots without reopening original paths. These guarantees cover adapter-selected locations and inputs, not arbitrary filesystem access by third-party code. Unsafe or uncertain locations are refused and recovery copies are preserved.

   One lifetime caveat needs explicit handling: the process runner can return `CleanupFailed` when descendants remain. Merely retaining a guard until that return does **not** prove protection through child completion. Preserve guard ownership with surviving process cleanup, rather than treating this error as completed teardown. [process.rs:508](../../../crates/infrastructure/src/shared/process.rs:508)

3. **Q3 — Fold the fix into the next coder assignment as its first, separately verified step.**

   No separate user approval or commit is necessary. Use one writer for the overlapping files and preserve the intended order:

   **WP1 completion and verification → WP2+WP3 → WP4 → WP5.**

   That follows HANDOFF’s sequencing and avoids concurrent edits to `files.rs` and `workflow.rs`. [HANDOFF.md:97](../../../HANDOFF.md:97)

   Amend WP5’s planned contract now: `NodeRequirementSet.file: Option<PathBuf>` must retain the corresponding guard—or carry a protected snapshot—through provisioning. Returning a path after dropping the guard recreates this gap. [Approved plan:322](../../../docs/handoff/plans/m5-backend-approved-plan.md:322)

**Corrected file scope and verification plan**

| Files | Required change |
|---|---|
| `crates/infrastructure/src/custom_nodes/registry.rs` | Open-file download signature; preserve transfer loop. |
| `crates/infrastructure/src/custom_nodes/files.rs` | Atomic archive creation; same-handle extraction; ownership-aware cleanup; implement guarded location acquisition. |
| `crates/infrastructure/src/custom_nodes/paths.rs` | Concrete retained location/input resources and stable execution paths. |
| `crates/infrastructure/src/shared/model_paths.rs` | Add a dedicated non-following regular-file read-pin helper with suitable Windows sharing. Preserve existing mutation-opening behavior. |
| `crates/application/src/custom_nodes/ports.rs` | Guard trait, `pin_location`, hook signature, precise guarantees. |
| `crates/application/src/custom_nodes/workflow.rs` | Guard preview and execution; compare guarded input identities; remove execution uses of `path()`. |
| `crates/infrastructure/src/custom_nodes/hook.rs` | Execute the guarded script with guarded cwd; retain resources through teardown. |
| `crates/infrastructure/src/environment/packages.rs` and `packages/tests.rs` | Verify/adapt uv input handling while preserving requirements semantics. |
| `crates/infrastructure/src/shared/process.rs`, `environment/exec.rs` | Carry resource ownership through surviving-process cleanup where `CleanupFailed` would otherwise release it. |
| Application/infrastructure `custom_nodes/tests.rs` | Update the only existing `NodeFiles`/`NodeHook` fakes and add boundary tests. |
| Approved backend plan, `code-wp1.md`, `HANDOFF.md` | Record expanded scope, WP5 guard lifetime, observed checks and remaining platform evidence. |

Add these named tests:

- `registry_download_writes_the_opened_staging_file`
- `registry_archive_replacement_is_not_extracted`
- `registry_create_new_refusal_preserves_existing_entries`
- `registry_download_cancellation_cleans_only_owned_entries`
- `node_execution_guard_spans_preview_install_hook_and_cleanup`
- `node_subprocess_inputs_resist_parent_and_leaf_substitution`
- `node_execution_refuses_linked_requirements_and_hooks`
- `guarded_requirements_preserve_relative_includes_and_local_paths`
- `guarded_hook_preserves_cwd_imports_and_script_filename`
- `cleanup_failure_retains_execution_resources_until_children_exit`

Use deterministic substitution boundaries and external sentinels. On Linux, prove substitution actually occurred; on Windows, distinguish sharing protection from unavailable link privileges. The current substitution test covers publication, restore, cleanup and recycling—not these execution boundaries. [tests.rs:2319](../../../crates/infrastructure/src/custom_nodes/tests.rs:2319)

Success requires the new tests, existing registry/archive/recovery tests, application custom-node tests, shared model-library regressions, formatting, architecture checks and clippy. Native Linux evidence must demonstrate actual uv/Python behavior with guarded inputs; Windows-only passes cannot establish it. Verify the merged M6 B code separately before attributing any workspace failure to this fix.

Two remaining risks prevent an unconditional “race-free” claim:

- Linux descriptor paths must remain usable by the child; tool canonicalization must not silently restore ordinary pathname resolution.
- Existing Linux rename/unlink helpers check identity and subsequently act by name. They protect parent addressing but do not make that identity check atomic with the mutation. Keep the documented guarantee precise and assess final-entry substitution separately. [model_paths.rs:358](../../../crates/infrastructure/src/shared/model_paths.rs:358), [model_paths.rs:462](../../../crates/infrastructure/src/shared/model_paths.rs:462)

This was a read-only snapshot review. No files were changed, and no builds, tests or installs were run.