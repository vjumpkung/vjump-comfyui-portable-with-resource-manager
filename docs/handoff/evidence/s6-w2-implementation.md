# S6 W2 implementation: setup wizard "Latest commit (master)"

Owner decision 2026-10-20 15:20: add "Latest commit (master)" to the setup wizard's ComfyUI source choice.
Scope: UI and docs only. No Rust file touched, no cargo run, no bindings change (`ComfyRefDto` already has `{ kind: 'branch'; value }`, `ui/src/shared/ipc/bindings.ts:227`), no git write command.

## Per-file summary

### `ui/src/features/setup/model.ts`
- `:97` new `export type ComfySource = 'latest' | 'master' | 'tag'`.
- `:103` `WizardDraft.useTag: boolean` replaced by `comfySource: ComfySource`.
- `:124-133` `readDraft` reads a valid `comfySource` string; otherwise maps a legacy boolean `useTag` (true -> `'tag'`, false -> `'latest'`); an absent or invalid value leaves the field unset (wizard default `'latest'`).

### `ui/src/features/setup/setup-wizard.tsx`
- `:42` imports `type ComfySource`.
- `:100-102` state `useTag` replaced by `comfySource` (default `draft.comfySource ?? 'latest'`; Latest release stays the default).
- `:125` the saved draft JSON now carries `comfySource` instead of `useTag`.
- `:207` validation: the tag must be non-empty only when `comfySource === 'tag'`.
- `:697-722` Stage 4 toggle group, controlled by `comfySource`, items in order "Latest release" (`latest`), "Latest commit (master)" (`master`), "Specific tag" (`tag`). `onValueChange` narrows to the three known values (ignores the empty deselect). The `FieldDescription` shows the spec's development note for `master`, and the existing sentence otherwise.
- `:723` the tag input renders only for `tag`.
- `:889-895` review summary: `tag` -> the tag, `master` -> "Latest commit on master", else "Latest published release".
- `:1016-1023` payload: non-first-run -> `{ kind: 'keepCurrent' }` (unchanged); `tag` -> `{ kind: 'tag', value: tag.trim() }`; `master` -> `{ kind: 'branch', value: 'master' }`; `latest` -> `{ kind: 'latest' }`.

Key diff (payload):
```
- : useTag
-   ? { kind: 'tag', value: tag.trim() }
-   : { kind: 'latest' },
+ : comfySource === 'tag'
+   ? { kind: 'tag', value: tag.trim() }
+   : comfySource === 'master'
+     ? { kind: 'branch', value: 'master' }
+     : { kind: 'latest' },
```

### `ui/src/features/setup/setup-wizard.test.tsx`
- `:197` `latest_commit_choice_sends_the_master_branch`: Latest release is checked by default and the development note is absent; clicking "Latest commit (master)" shows the development note and the `origin/master` sentence and hides the tag input; the review shows "Latest commit on master"; Start setup sends `comfyuiRef: { kind: 'branch', value: 'master' }` in the full typed request.
- `:238` `legacy_draft_use_tag_still_selects_specific_tag`: the wizard is mounted with `status.draft = {"version":1,"useTag":true,"tag":"v0.8.0"}`; at stage 4 "Specific tag" is checked and the tag input holds `v0.8.0`. Drafts are restored through `readDraft(status.draft)`, so this is meaningful at wizard level.
- The existing latest-release payload tests (three `comfyuiRef: { kind: 'latest' }` cases) and the specific-tag test (`comfyuiRef: { kind: 'tag', value: 'v0.9.0' }`) are unchanged and pass.

### `ui/src/features/setup/model.test.ts`
- `:61` `reads_the_comfy_source_and_maps_the_legacy_use_tag_flag` (added, not in the spec list): `comfySource: 'master'` is read as-is; `comfySource` wins over a conflicting `useTag`; legacy `useTag: true` -> `'tag'`; `useTag: false` -> `'latest'`; an invalid `comfySource` -> `{}`. The existing `readDraft` test is unchanged and passes.

### `docs/DOCUMENTATION.md`
- `:143` the "ComfyUI version" option now describes **Latest release** (default), **Latest commit (master)** (newest `master` commit, development code, may be less stable, tracks `origin/master`, updated by `git pull` or the app's **Fast-forward**) and **Specific tag** (exact tag; tag browsing is not offered).

### `docs/REQUIREMENTS.md`
- `:125` (section 2.4, ComfyUI row) appended "(the wizard offers Latest release, Latest commit (master) and a specific tag — owner decision 2026-10-20)" to the Alternatives cell.

## Command results (run from the repository root)

| Command | Result |
|---|---|
| `pnpm -C ui lint` | exit 0, no output from eslint |
| `pnpm -C ui typecheck` | exit 0 (`tsc -b`) |
| `VITEST_MAX_WORKERS=2 pnpm -C ui test` | exit 0; Test Files 30 passed (30); Tests 200 passed (200) |
| `pnpm -C ui build` | exit 0; built in 596 ms (only the existing >500 kB chunk-size warning) |
| `node scripts/check-arch-deps.mjs` | `Architecture dependencies: OK` |

Targeted run of `src/features/setup` (verbose): 3 files, 27 tests passed, including the three new tests listed above.

## Deviations and notes
- Added one extra unit test in `model.test.ts` (`reads_the_comfy_source_and_maps_the_legacy_use_tag_flag`) beyond the two named in the spec; it covers the new `readDraft` mapping directly.
- The saved draft no longer writes `useTag` (only `comfySource`). Drafts written by earlier builds are still read (legacy mapping). A draft written by this build and read by an older build would ignore the source choice and default to Latest release; this is the intended one-way compatibility.
- No Rust, bindings, or backend change was needed; the backend support noted in the spec (`ComfyRefDto::Branch` -> `ComfyRef::Branch`) was not re-verified here since Rust files were out of scope.
- No cargo or git write command was run.
