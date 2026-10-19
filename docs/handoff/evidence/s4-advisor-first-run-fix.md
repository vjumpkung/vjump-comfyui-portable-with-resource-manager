**No-go as written; go for the in-place merge after the corrections below.** It addresses the pinned-directory constraint, but the proposed rollback is not sufficient for interruption recovery, and pathname checks alone do not satisfy containment requirements.

| Question | Decision |
|---|---|
| **Q1** | Use tolerate-and-merge for this fix. Defer lazy startup initialization. Preserve every existing scaffold directory, including an empty destination; use whole-directory rename only when the destination is absent. |
| **Q2** | Accept any bounded tree containing only real directories. Reject files anywhere, links, reparse points, special entries, traversal errors, and exceeded limits. Existing `.git` metadata must follow checkout validation, never scaffold classification. A downloaded model in a non-Git destination remains a refusal. |
| **Q3** | Keep reverse rollback for handled failures, but add durable recovery information. An in-memory moved-entry list is insufficient. Roll back only entries still verified as unchanged, app-owned clone content; preserve ambiguous state. |
| **Q4** | The intended behavior preserves adoption, KeepCurrent, and protection of user files. The original plan has gaps against FR-SETUP-6 retryability, §6.2 reliability, and NFR-SEC-5 containment. The corrections below close those gaps without changing the accepted architecture. |
| **Q5** | An infrastructure regression using the real creators is sufficient. No desktop composition change or full desktop startup test is required. Recovery must be reachable before the generic `NotGit` refusal. |
| **Q6** | Use the wrapper fix for both viewers, preserving `h-72` for `LogView` and **`h-80` for `StreamLogView`**. Add real layout verification; mocked class assertions cannot establish that overflow is fixed. |

I would implement the corrected plan in this order:

1. **Make scaffold inspection conservative and read-only.**  
   Use explicit non-following metadata/open results, including for the destination itself. Only `NotFound` means missing: `exists()` hides errors and follows links, including dangling links. Retain the proposed 64-component/4,096-entry bounds and reject every Windows reparse point.

   Check for pre-existing `.git` metadata before accepting a directory-only scaffold. Otherwise, an empty or broken `.git` directory becomes cloneable, weakening the existing refusal contract tested in [source/tests.rs](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/environment/source/tests.rs:326).

2. **Plan publication completely, then use pinned operations.**  
   Preserve existing destination directories and plan all overlapping paths before moving anything. Validate staged names and entry types as well as destination entries. Refuse collisions before mutation, including a staged file colliding with an existing directory.

   Use the existing [pinned-directory and no-replace rename primitives](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/shared/model_paths.rs:566), rather than pathname `exists()` checks followed by ordinary `fs::rename`. Pins protect directory identity; no-replace operations protect against a destination appearing after planning. This follows [NFR-SEC-5](D:/vjump/vjump-comfyui-portable-with-resource-manager/docs/REQUIREMENTS.md:475).

   Keep `.vjump-no-hooks` **empty**, rather than merely preserving its directory. Refuse a staged checkout that would populate it.

3. **Make interrupted publication safely retryable.**  
   Persist a bounded ownership/publication record before the first move, binding the stage and destination to the planned entries and the identities/content needed for safe recovery. Continue moving the top-level `.git` last.

   For handled failures or cancellation before that commit point, reverse only verified, unchanged clone entries. If rollback cannot finish safely, retain the stage and recovery record and report incomplete publication accurately.

   On Retry, recognize a validated pending publication before ordinary `NotGit` handling. Inspection can remain read-only; the clone path performs recovery, restoring the scaffold before attempting a fresh clone. Arbitrary partial trees must remain refusals.

   **Remove unconditional stage deletion from this recovery path.** Currently, [clone_checkout](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/environment/source.rs:248) deletes the stage at entry. Keeping it after rollback failure accomplishes nothing if the next attempt deletes it.

   Treat successful `.git` publication as the commit point. A subsequent failure to remove the empty stage skeleton must not roll back a completed checkout or claim the destination was “left unchanged.”

   Keep these details inside infrastructure. No new domain abstraction or application port is needed for this approach. Preserve HANDOFF’s accepted Linux residuals; ordinary verified rollback does not authorize blindly renaming an identity-mismatched destination back.

4. **Apply the terminal wrapper fix without changing log behavior.**  
   Put `setup-log`, the existing height, border, rounding, padding, and `overflow-hidden` on the outer frame. Pass a padding-free, border-free `h-full min-w-0` child to `term.open()`. Keep each current accessible label on the terminal host and continue observing that host for resizing.

   Preserve the distinct heights visible in [log-view.tsx](D:/vjump/vjump-comfyui-portable-with-resource-manager/ui/src/shared/ui/log-view.tsx:124). Leave sanitization, streaming, and terminal lifecycle behavior unchanged.

The material risks are publication interruption and concurrent filesystem changes. A kill after moving ordinary files leaves a non-scaffold destination without `.git`; `.git`-last alone prevents false checkout recognition but does **not** make Retry work. Also, pinned directories do not freeze their contents. Rollback and cleanup must never delete files another operation added or modified. The existing maintenance-gate design should remain unchanged.

Verification should establish the following:

- **Real startup scaffold:** create DownloadStack/LibraryRoots first, then NodePaths—the latter opens an existing `ComfyUI` directory. Keep pins alive throughout publication and verify directory identities afterward. Use actual creators, not a handwritten equivalent. Place the test where private NodePaths access is available without widening production visibility.
- **Real overlapping checkout:** extend the local Git fixture with tracked files beneath both `models/` and `custom_nodes/`. The current README/requirements fixture would not exercise recursive merging. Verify the requested HEAD, clean porcelain status, preserved directory identities, empty hooks directory, and successful cleanup.
- **Refusal and containment:** cover user files, empty/broken `.git`, root and nested links/junctions, bounds, read failures, and directory/file conflicts. Verify unchanged user content and external sentinels.
- **Failure recovery:** inject a failure after at least one move, cancellation, rollback failure, and interruption before/after `.git` publication. Retry must recover without deleting retained evidence or user changes.
- **UI:** test both `open()` hosts and resize wiring, then inspect real xterm geometry in setup, ComfyUI, and custom-node logs after resize and font loading. Confirm the dark band is gone and terminal scrolling remains usable.

Keep the proposed checks, using `cargo fmt --all -- --check`, one heavy Cargo command at a time with `-j 4`, and `VITEST_MAX_WORKERS=2`. Passing these regressions proves the clone blocker is fixed; it does not establish the full NVIDIA clean-setup acceptance scenario.

**Indispensable user-only questions: none.** This review changed no files and ran no builds or tests.