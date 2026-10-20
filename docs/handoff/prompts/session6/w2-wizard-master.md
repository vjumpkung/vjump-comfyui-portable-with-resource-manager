ROLE: coder (write-capable). Mechanically implement the owner decision below. Do not redesign or expand scope. If it conflicts with the code, stop and report the gap precisely.

## Build environment and repository rules (mandatory)
- WORKING COPY: `D:\vjump\vjump-comfyui-portable-with-resource-manager` (branch `feat/m0-skeleton`, HEAD `e384d43`). Uncommitted working-copy changes = verified session-6 packages (Q, S0, F, G, X, G2b, G2c, D3+D5, D3b, U, W) + docs; another coder (package D4) is concurrently editing Rust files under `crates/infrastructure/src/models/acquisition/` — do not touch any Rust file and do not run cargo. Preserve all other changes; never revert files you did not change.
- Read `AGENTS.md`. TypeScript: 2-space, single quotes, no semicolons; match surrounding style. Vitest with `VITEST_MAX_WORKERS=2`. LF line endings. UI + docs only; no bindings change (the wire type already has `{ kind: 'branch', value }`).
- No git write commands. Do not touch `D:\vjc-s6\` or `D:\comfyui-test\`; do not kill `vjumpkung-comfyui-desktop` processes.

## Owner decision (2026-10-20 15:20)
"Add Latest commit (master) option in setup wizard too." (Package U already added "Update to latest commit (git pull)" to the ComfyUI page; the open owner question from the G2b ruling is now answered.) The backend already supports it: `ComfyRefDto::Branch { value }` (`crates/desktop/src/ipc/env_dto.rs:891-900`) → `ComfyRef::Branch` → `source.rs` resolves `ls-remote refs/heads/<b>` and clones with `checkout -B <b> --track origin/<b>` (test `a_branch_clone_tracks_the_branch`); activation records `comfyui_reference = <b>`.

## Plan (main; trivial UI, AI-decidable)
`ui/src/features/setup/setup-wizard.tsx`:
1. Replace the boolean source state (`useTag`, ~:99, persisted in the draft ~:122) with a three-way choice `comfySource: 'latest' | 'master' | 'tag'`; read a legacy draft's `useTag` (true → 'tag', false/undefined → 'latest') so saved drafts keep working; default `'latest'` (Latest release stays the default).
2. Stage 4 toggle group (~:690-705): items in this order — "Latest release" (value `latest`), "Latest commit (master)" (value `master`), "Specific tag" (value `tag`). Description: keep the existing sentence for latest/tag; when `master` is selected show "Installs the newest commit on ComfyUI's master branch (development code, may be less stable than a release). The checkout tracks origin/master, so `git pull` and the app's Fast-forward update it." The tag input shows only for `tag`.
3. Validation (~:204): the tag must be non-empty only for `tag`.
4. Payload (~:1002-1007): `latest` → `{ kind: 'latest' }`, `master` → `{ kind: 'branch', value: 'master' }`, `tag` → `{ kind: 'tag', value: tag.trim() }`; non-first-run modes unchanged (`keepCurrent`).
5. Review summary (~:880): show "Latest commit on master" for `master`.
6. Tests (`setup-wizard.test.tsx`, exact names): `latest_commit_choice_sends_the_master_branch` (selecting "Latest commit (master)" sends `comfyuiRef: { kind: 'branch', value: 'master' }`, shows the development note, review shows "Latest commit on master"); keep/adapt the existing latest/tag tests; `legacy_draft_use_tag_still_selects_specific_tag` only if drafts are restored in tests already (otherwise skip and say so).
7. Docs (only these): `docs/DOCUMENTATION.md` setup section (ComfyUI source choice: Latest release default, Latest commit (master) = development code tracking origin/master, Specific tag); `docs/REQUIREMENTS.md` §2.4 ComfyUI row: append "(the wizard offers Latest release, Latest commit (master) and a specific tag — owner decision 2026-10-20)".

## Success criteria (report exact counts)
```
pnpm -C ui lint
pnpm -C ui typecheck
VITEST_MAX_WORKERS=2 pnpm -C ui test
```
(Do not run `pnpm -C ui build` while D4 runs cargo? — `pnpm -C ui build` is safe; run it too.)
```
pnpm -C ui build
```
OUTPUT: per-file summary with file:line, key diffs, command results with counts, deviations. Also write it to `docs/handoff/evidence/s6-w2-implementation.md`.
