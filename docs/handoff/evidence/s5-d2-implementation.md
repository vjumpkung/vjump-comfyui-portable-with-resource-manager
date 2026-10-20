# S5 package D2 implementation: curated node "unavailable" marker

Role: coder. Plan: advisor stand-in ruling D2, GO with O2-narrowed (O1 rejected, O3 deferred). Working copy `D:\vjump\vjump-comfyui-portable-with-resource-manager`, branch `feat/m0-skeleton`, no git write command was run. Packages P0 and I (uncommitted) were preserved; only the files below were changed for D2.

## Change summary (per file)

| File | Change |
| --- | --- |
| `crates/infrastructure/src/environment/data/curated_nodes.json:104` | PlagueKind entry gets `"unavailable"` with the exact approved reason. Order and pinned commit unchanged. |
| `crates/infrastructure/src/environment/curated.rs:13,23,51,73-79,114-121,138` | `MAX_UNAVAILABLE_BYTES = 300`; `EntryDoc.unavailable: Option<String>` (`#[serde(default)]`); `CuratedError::Unavailable` ("a curated node has an invalid unavailability reason"); `valid_unavailable` (non-empty, already trimmed, <= 300 bytes, no control chars); mapped into `CuratedNodeSpec`. |
| `crates/application/src/environment/model.rs:483` | `CuratedNodeSpec.unavailable: Option<String>`. |
| `crates/application/src/environment/setup.rs:292-312` | `curated_progress` refuses an id whose catalog entry has `unavailable` set with the existing `CuratedSelectionInvalid` (`curated_selection_invalid`); doc comment updated. |
| `crates/application/src/environment/tests/fakes.rs:87` | `curated_spec` fixture sets `unavailable: None` (the only constructor besides the infrastructure one). |
| `crates/desktop/src/ipc/env_dto.rs:348,360` | `CuratedNodeDto.unavailable: Option<String>` and the `From<&CuratedNodeSpec>` mapping. |
| `ui/src/shared/ipc/bindings.ts` | Regenerated only (`unavailable: string | null` in `CuratedNodeDto`, line ~292). Not hand-edited. |
| `ui/src/features/setup/setup-wizard.tsx:164-167,805-807,822-824` | Initial default selection excludes unavailable entries; unavailable checkbox is `disabled` and never `checked`; shows "Unavailable: {reason}". No unavailable id can enter `curatedSelection`, so none is sent. |
| `ui/src/features/setup/setup-wizard.test.tsx:256,275` | Existing catalog fixture gets `unavailable: null`; new Vitest test added. |
| `ui/src/app/App.test.tsx:185` | Catalog mock gets `unavailable: null` (typecheck requires it). |
| `docs/DOCUMENTATION.md:151` | "All are ticked by default." replaced by "21 of the 22 are ticked by default; ComfyUI-PlagueKind-Nodes is listed as unavailable at its pinned commit (it ships `__pycache__`)." |
| `docs/handoff/evidence/s3-residuals.md:22` (row 16) | Appended the approved sentence (see deviations). |

`ui/src/shared/testing/environment.ts` needed no change: it has no curated catalog fixture (only `curated: []` run progress). `ui/src/shared/ipc/gateway.test.ts` also needed none (its catalog mock is untyped).

## Tests added (exact names)

- `crates/infrastructure/src/environment/curated/tests.rs:228` `bundled_catalog_marks_only_plaguekind_unavailable` (exactly one unavailable entry, id `comfyui-plaguekind-nodes`, pinned commit, reason mentions `__pycache__`, catalog index 19 kept, 21 available).
- `crates/infrastructure/src/environment/curated/tests.rs:257` `catalog_rejects_blank_or_control_unavailable_reason` (accepts a valid reason, 300 bytes and `null`; rejects empty, blank, padded, newline, bell, ESC, NUL and 301 bytes with `CuratedError::Unavailable`).
- `crates/application/src/environment/tests/setup.rs:1745` `first_run_rejects_unavailable_curated_selection` (unavailable alone or beside an available id gives `CuratedSelectionInvalid` and stores no run; the available id alone still registers).
- `ui/src/features/setup/setup-wizard.test.tsx:275` `unavailable_curated_nodes_start_unticked_disabled_show_reason_and_are_not_sent`.

## Verification (all run at the end, sequentially, `-j 6`, `VITEST_MAX_WORKERS=2`)

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | exit 0 |
| `cargo clippy --workspace --all-targets --locked -j 6 -- -D warnings` | exit 0, no warnings |
| `cargo test --workspace --locked -j 6 --no-fail-fast` | 718 passed / 0 failed / 10 ignored (baseline 715 / 0 / 10, +3 as expected) |
| `cargo test -p desktop --test bindings --locked -j 6` (after regeneration with `-- --ignored export_bindings`) | `bindings_up_to_date` ok; 1 passed, 1 ignored (the export test) |
| `cargo test -p desktop --test acl --locked -j 6` | 11 passed / 0 failed |
| `pnpm -C ui lint` | exit 0 |
| `pnpm -C ui typecheck` | exit 0 |
| `pnpm -C ui test` | 29 files passed / 184 tests passed (baseline 29 / 183, +1) |
| `pnpm -C ui build` | built, exit 0 (existing chunk-size warning only) |
| `node scripts/check-arch-deps.mjs` | "Architecture dependencies: OK" |

All edited files are LF (checked byte-wise; `git ls-files --eol` reports `w/lf`).

## Deviations and interpretations

1. `s3-residuals.md` row 16: the plan says "row 16 -> <sentence>". Replacing the whole description would drop the existing `TargetExists` and cancelled-first-run facts, so the approved sentence ("Skip optional step skips every curated node not yet installed; to keep the others, abort and start setup without the failing node.") was appended to the existing description. If a full replacement was intended, trim the first two sentences of that cell.
2. "Trimmed" in the reason validation is enforced as "must already be trimmed" (leading or trailing whitespace is rejected, like blank or control content), rather than silently trimming it.
3. No other deviations; nothing outside the stated scope was edited, and `D:\vjc-s5\` and running `vjumpkung-comfyui-desktop` processes were not touched.

## Remaining notes

- Runs saved before this change that already selected PlagueKind are unchanged (out of scope per the plan); Skip still skips every not-installed curated node (O3 deferred).
