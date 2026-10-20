ROLE: coder (write-capable). Mechanically implement the owner decision below. Do not redesign or expand scope. If it conflicts with the code, stop and report the gap precisely.

## Build environment and repository rules (mandatory)
- WORKING COPY: `D:\vjump\vjump-comfyui-portable-with-resource-manager` (branch `feat/m0-skeleton`, HEAD `e384d43`). Uncommitted working-copy changes = verified session-6 packages (Q, S0, F, G, X, G2b, G2c, D3+D5, D3b, U) + docs: preserve them; never revert files you did not change.
- Read `AGENTS.md`. TypeScript: 2-space, single quotes, no semicolons; match surrounding style. Vitest with `VITEST_MAX_WORKERS=2`. LF line endings. UI-only package: no Rust changes, no bindings changes.
- No git write commands. Do not touch `D:\vjc-s6\` or `D:\comfyui-test\`; do not kill `vjumpkung-comfyui-desktop` processes.

## Owner decision (2026-10-20 14:50)
"Make ComfyUI-Manager checked by default in setup but the Custom Nodes list not checked."

## Plan (main; trivial UI defaults, AI-decidable implementation of an owner decision)
`ui/src/features/setup/setup-wizard.tsx`:
1. Manager default: `useState(draft.installManager ?? (mode === 'first_run'))` (~:100-101) — ticked by default in first-run setup; other modes keep `false` (their Manager install still pauses for consent). Use the component's actual `mode` value at that point (if `mode` is not yet available there, derive the default where it is; keep a saved draft's explicit value).
2. Checkbox label (~:750): "Install ComfyUI-Manager (off by default)" → "Install ComfyUI-Manager" (no default claim in the label).
3. Curated default: initialise the selection to an EMPTY list instead of every available entry (~:169-172: `setCuratedSelection([])` once the catalog has loaded; keep the existing `null` = "not loaded yet" semantics and the explicit-empty-selection behaviour). Unavailable entries stay disabled.
4. The review-stage disclosure/`thirdPartyConsent` logic is unchanged (it already depends on the actual selection), so a default first run shows the disclosure for ComfyUI-Manager only.
5. Tests (adapt existing wizard tests that assumed Manager off / all curated preselected; add exact names): `first_run_ticks_comfyui_manager_and_no_curated_nodes_by_default` (first_run: Manager checkbox checked, every curated checkbox unchecked, payload `installManager: true`, `curatedNodes: []`, `thirdPartyConsent: true`, disclosure lists only the Manager source); `other_modes_leave_comfyui_manager_unticked` (e.g. change_torch: unchecked, `installManager: false`).
6. Docs (only these): `docs/REQUIREMENTS.md` FR-NODE-10: "Installing ComfyUI-Manager is optional; first-run setup ticks it by default (owner decision 2026-10-20)." FR-SETUP-9: the curated list is offered unticked; the user ticks the nodes they want (owner decision 2026-10-20). `docs/DOCUMENTATION.md` setup Options section (~:144 and the curated paragraph): Manager ticked by default in first-run setup; curated nodes unticked by default. `HANDOFF.md` §3 decision row "ComfyUI-Manager optional/off" → "optional; ticked by default in first-run setup; curated nodes unticked by default (owner, 2026-10-20)". `docs/handoff/plans/m6-approved-plan.md` near "Offer only for FirstRun; default all 22 selected": append "(superseded 2026-10-20: none selected by default)".

## Success criteria (report exact counts)
```
pnpm -C ui lint
pnpm -C ui typecheck
VITEST_MAX_WORKERS=2 pnpm -C ui test
pnpm -C ui build
```
OUTPUT: per-file summary with file:line, key diffs, command results with counts, deviations. Also write it to `docs/handoff/evidence/s6-w-implementation.md`.
