# Session 6 package W - wizard defaults (implementation report)

Owner decision (2026-10-20 14:50): ComfyUI-Manager is ticked by default in setup; the Custom Nodes list is not.
Scope: UI-only. No Rust, bindings or git write changes. Other session-6 working-copy changes preserved.

## Per-file summary

### `ui/src/features/setup/setup-wizard.tsx`
- `:101` Manager default: `draft.installManager ?? mode === 'first_run'` (was `?? false`). A saved draft's explicit value still wins; other modes (`change_torch`, `repair`, `switch_venv`) stay `false`.
- `:169-170` Curated default: `if (curated.data && curatedSelection === null) setCuratedSelection([])` (was: every `unavailable === null` entry id). `null` still means "not loaded yet" (`optionsOK` unchanged); an empty array is the explicit empty selection; unavailable entries stay disabled/unticked. Comment added: "The curated list is offered unticked: the user ticks the nodes they want."
- `:748` Label: `Install ComfyUI-Manager` (was `Install ComfyUI-Manager (off by default)`).
- Review-stage disclosure and `thirdPartyConsent` (`:227-237`) unchanged; they already derive from the actual selection, so a default first run shows the disclosure for the Manager source only.

### `ui/src/features/setup/setup-wizard.test.tsx`
New tests (exact names):
- `:432` `first_run_ticks_comfyui_manager_and_no_curated_nodes_by_default`: Manager checkbox `aria-checked=true`; every curated checkbox (including the unavailable one) `false`; review disclosure lists only `https://github.com/Comfy-Org/ComfyUI-Manager`; payload `installManager: true`, `curatedNodes: []`, `thirdPartyConsent: true`.
- `:462` `other_modes_leave_comfyui_manager_unticked` (change_torch): Manager `aria-checked=false`, no third-party list, payload `installManager: false`, `curatedNodes: []`, `thirdPartyConsent: false`.

Adapted existing tests:
- `:161` typed-tag test renamed "...and the default Manager choice"; payload now `installManager: true`, `thirdPartyConsent: true`.
- `:251` `empty_accelerator_selection_remains_empty`: payload `installManager: true`, `thirdPartyConsent: true`.
- `:276` `catalog_failure_blocks_start_and_refetch_preserves_unchecked_nodes`: node starts unchecked; tick survives a refetch; untick survives a refetch; review shows two "None selected".
- `:299` `unavailable_curated_nodes_start_unticked_disabled_show_reason_and_are_not_sent`: the available node starts unchecked and is ticked by the test; payload `installManager: true`, `curatedNodes: ['node']`, `thirdPartyConsent: true`; blocked node unchanged.
- `:334` `review_page_discloses_third_party_code_and_sends_consent`: cases reworked for the new defaults: Manager + both nodes ticked (sources Manager, node, other); Manager alone (default); Manager + one node; node only with Manager unticked (new case); nothing (Manager unticked) = no disclosure, no consent. The `change_torch` / `repair` cases are unchanged and still pass.

### `ui/src/app/App.test.tsx`
- `:147-148` `startFirstRun()` now asserts "Curated Pack" starts unchecked (`false`) and ticks it before continuing.
- `:210` `curated_selection_does_not_authorize_execution` expected request: `installManager: true` (was `false`); `curatedNodes: ['curated-pack']`, `thirdPartyConsent: true` unchanged.

### Docs
- `docs/REQUIREMENTS.md:385` FR-NODE-10: "Installing ComfyUI-Manager is optional; first-run setup ticks it by default (owner decision 2026-10-20)."
- `docs/REQUIREMENTS.md:285` FR-SETUP-9: "The user can tick or untick each one." replaced by "The curated list is offered unticked; the user ticks the nodes they want (owner decision 2026-10-20)."
- `docs/DOCUMENTATION.md:144` Options, ComfyUI-Manager: "Optional. It is ticked by default in first-run setup (other setup modes leave it unticked)."
- `docs/DOCUMENTATION.md:151` Options, curated paragraph: "21 of the 22 are ticked by default; ..." replaced by "None is ticked by default; you tick the nodes you want. ComfyUI-PlagueKind-Nodes is listed as unavailable and cannot be ticked (...)."
- `HANDOFF.md:109` section 3 decision row: "ComfyUI-Manager optional; ticked by default in first-run setup; curated nodes unticked by default (owner, 2026-10-20)".
- `docs/handoff/plans/m6-approved-plan.md:325`: appended "(superseded 2026-10-20: none selected by default)".

## Command results (repo root, `VITEST_MAX_WORKERS=2` for tests)
| Command | Result |
|---|---|
| `pnpm -C ui lint` | exit 0, no findings |
| `pnpm -C ui typecheck` | exit 0 (`tsc -b`, no errors) |
| `VITEST_MAX_WORKERS=2 pnpm -C ui test` | exit 0: 30 test files passed (30), 197 tests passed (197) |
| `pnpm -C ui build` | exit 0, built in 467 ms (only the pre-existing >500 kB chunk-size warning) |

Targeted run before the full suite: `src/features/setup` + `src/app/App.test.tsx`: 4 files, 34 tests passed.
LF endings verified (0 CR characters) in every edited file.

## Deviations
- `ui/src/app/App.test.tsx` was not named in the prompt, but it asserted the old defaults (curated pack preselected, `installManager: false`), so it needed the adaptation above (covered by "adapt existing wizard tests").
- Docs: the prompt named the `DOCUMENTATION.md` "~:144" Manager line and "the curated paragraph" (:151); both edited. `docs/handoff/evidence/*`, `docfacts.md` and the earlier session prompts still describe the previous defaults; left untouched per "Docs (only these)".
- `ui/src/features/setup/rebuild-setup.tsx:40` (`installManager: false`, rebuild mode) intentionally unchanged: rebuild is not the first-run wizard.
- No saved-draft test was added (not requested); the draft precedence (`draft.installManager ?? ...`) is preserved by construction.
