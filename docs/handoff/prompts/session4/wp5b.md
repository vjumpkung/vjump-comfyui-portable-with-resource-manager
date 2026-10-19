ROLE: coder (write-capable). Mechanically implement the advisor-approved decisions below. Do not redesign or expand scope. If something conflicts with the code, stop that item and report the gap precisely.

CONTEXT: M5 WP5 was implemented in the working copy (uncommitted; keep all of it; checkpoint docs/handoff/patches/s4-04-wp5-two-gaps-open.patch; 609 tests pass). The WP5 coder stopped two items; the advisor (gpt-6.1-sol xhigh) decided them. Implement exactly the advisor's decision quoted below (the "ADVISOR DECISION" text is authoritative, including its file references and its test table).

ADVISOR DECISION:
1. **Gap 1 — GO for rejecting invalid requirements; NO-GO for changing only `pin_location`.**

   Only `NotFound` should mean absence. Oversized or non-UTF-8 root requirements must return `NodeAppError::UnsafeSource`; empty UTF-8 files remain valid.

   There is another bypass: [read_tree](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/custom_nodes/files.rs:177) also converts invalid requirements into absent metadata. Then [preview_packages](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/application/src/custom_nodes/workflow.rs:340) can return early, or preview registry extras without acquiring the guard. A strict `pin_location` alone therefore **does not guarantee refusal before consent**.

   Corrected implementation:

   - In [files.rs](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/custom_nodes/files.rs:917), explicitly distinguish absent, oversized, invalid UTF-8, and valid bytes. Preserve valid bytes and their pinned handle.
   - In [workflow.rs](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/application/src/custom_nodes/workflow.rs:340), validate the guard for every directory Install/Update/RetryDependencies preview **before** the “no requirements” early return. Reuse that guard during package preview. Preserve single-file handling.
   - Clarify the guard contract in [ports.rs](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/application/src/custom_nodes/ports.rs:89): `None` means absent; unsupported input is an error.
   - Keep the existing provider error translation: `ProvisionError::Failed("Custom node requirements could not be read or recorded.")`. Neither provider method may convert this error into an empty set or `false`.

   This satisfies FR-NODE-4 and WP5’s exact requirement selection. It preserves NFR-SEC-3 consent boundaries and the existing NFR-SEC-4 archive safeguards. The existing `node_unsafe_source` DTO mapping and frontend refusal handling suffice; **no UI copy, DTO, bindings, or migration change is required**.

2. **Gap 2 — GO for unconditional `NotRun` when identity cannot support projection.**

   In [project_dependencies](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/application/src/custom_nodes/use_cases.rs:30), return `NotRun` without looking up an outcome when:

   ```rust
   node.conflict || node.tree_digest.is_empty()
   ```

   Put this rule in the shared projection so both `scan_held()` and subsequent `list()` calls enforce it. Preserve source, origin, node ID, conflict display, and historical dependency outcomes.

   One factual correction: the current real [scanner](D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/custom_nodes/scan.rs:210) supplies an empty digest, and `merge_observed` does **not** copy the stored digest. The reported stale-digest trace is therefore inaccurate for that path. Nevertheless, projection currently lacks the explicit conflict/unknown-content exclusion, so the proposed invariant is appropriate.

   Non-conflict refresh failures already propagate through `?`; preserve that behavior. Do not introduce a successful partial scan or new adapter fallback. Any deliberately returned unrefreshed node must carry an empty digest and project `NotRun`.

The remaining mini-plan is to add these focused regressions, then run the approved checks. Names below are exact proposed names unless marked **extend**.

| Test file | Test | Required assertions |
|---|---|---|
| `crates/infrastructure/src/custom_nodes/tests.rs` | **New** `node_execution_refuses_oversized_and_non_utf8_requirements` | Oversized and invalid UTF-8 inputs return `UnsafeSource` through `pin_location`; cover live, disabled, and staged directory locations. |
| Same | **New** `node_execution_distinguishes_absent_empty_and_bounded_requirements` | Absent → `None`; empty → `Some(empty)` with a requirements digest; valid UTF-8 exactly at `REQUIREMENTS_LIMIT` preserves its bytes. |
| `crates/application/src/custom_nodes/tests.rs` | **New** `node_dependency_previews_validate_guard_before_omitting_requirements` | For directory Install/Update/RetryDependencies with absent snapshot metadata, injected guard refusal prevents a review, including with registry-only extras. No package preview/install, hook, publication, or environment mutation occurs. |
| Same | **Extend** `node_requirement_provider_errors_use_fixed_provision_descriptions` | Inject `UnsafeSource` from `pin_location`; both `requirement_sets` and `needs_repair` return the exact fixed provider error, never empty success or `false`. |
| Same | **New** `conflicted_nodes_never_project_successful_dependency_outcomes` | Seed matching historical `Ok` outcomes and a nonempty matching digest. Registry and Manual conflicts return `NotRun` through rescan **and list**; provenance/conflict remain intact; stored outcomes remain unchanged; no conflicted snapshot is attempted. |
| Same | **New** `node_dependency_projection_requires_a_content_digest` | An empty digest projects `NotRun` even if a matching `Ok` row exists. |
| Same | **Extend** `node_requirements_refuse_blocking_operations_and_provenance_conflicts` | Add an observed `conflict = true` with unchanged source identity; both provider methods fail. Existing source-mismatch refusal remains covered. |
| `crates/application/src/environment/tests/setup.rs` | **New** `node_requirement_provider_failure_never_reports_repaired_or_activates` | Inject provider failure for Verified Repair preflight, RepairRequired replacement, ChangeTorch, ChangePython, and custom-venv replacement. Never return `NothingToRepair` or activate; preserve the old active environment; no node `Ok` outcome or hook execution. |

Retain the positive assertions in `node_dependency_state_follows_active_environment_and_content`: fresh, matching content can still project `Ok`.

Retain the positive assertions in `node_dependency_state_follows_active_environment_and_content`: fresh, matching content can still project `Ok`.

SUCCESS CRITERIA (run at the end, report each with counts):
- cargo fmt --all -- --check
- cargo clippy --workspace --all-targets --locked -j 4 -- -D warnings
- cargo test --workspace --locked -j 4 --no-fail-fast (before this follow-up: 609 passed, 0 failed, 9 ignored)
- node scripts/check-arch-deps.mjs
- If any IPC DTO changes (it should not): regenerate bindings via the export test and run pnpm -C ui typecheck/lint/test.

OUTPUT: per-file summary (file:line), tests added/extended (names) with results, exact command results, gaps.

## Build environment and repository rules (mandatory)
- WORKING COPY: D:\vjump\vjump-comfyui-portable-with-resource-manager (branch feat/m0-skeleton, HEAD a879fc0). Build with the default MSVC toolchain from rust-toolchain.toml into the default target/ dir; do not set RUSTUP_TOOLCHAIN or CARGO_TARGET_DIR. Pass -j 4 to cargo; never run two cargo commands at once. Git tests must really execute.
- Rust: rustfmt, clippy -D warnings, no serde/tokio/reqwest/rusqlite/tauri in domain or application, ports via async-trait, match surrounding style and comment density. Normalize edited files to LF.
- Do not commit, push, stash, reset, checkout, apply or touch git history (git inside test temp dirs is fine). Touch only the files named in the decision and their tests.
