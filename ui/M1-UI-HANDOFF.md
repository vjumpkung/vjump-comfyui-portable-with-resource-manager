# M1 frontend handoff

The UI write scope is released to main. No Rust, generated bindings, staging, or commits were performed by the frontend coder.

## Behavior

- First-run shell gate with theme, transport-error and installation-recovery controls.
- Install-root picker and draft handoff. **Save install root** requires closing and reopening the desktop app; no native restart command is claimed.
- Hardware observations, all GPU details, and backend profile eligibility reasons, including unknown/unavailable observations.
- Managed/custom Python, validation checklist, invalidation after path edits, explicit adopted-path consent, mode-specific profile filtering.
- Latest published release or exact entered ComfyUI tag. Re-setup retains the current checkout.
- Manager off by default; exact resolved source/commit consent before retrying its dependency step. Source builds disabled.
- Review/start, persisted step snapshots, byte progress, bounded sanitized xterm logs, exact same-run retry, optional-failure skip, cancellation awaiting backend cleanup, status polling/reconnect.
- Selected environment summary, protected pins and verification report, package search with 100-row pages.
- Install/upgrade/uninstall preview, exact protected-change confirmation bound to the returned plan, one apply attempt per preview, metadata refresh after every apply outcome, RepairRequired messaging.
- Inactive-owned delete by exact environment ID with typed confirmation; backend remains authoritative for ownership, active selection, and in-use protection.
- Settings and Environment re-setup entry points. Features communicate through app callbacks and shared gateway types.

## Checks (2026-10-16)

All commands exited 0 on final source:

```text
pnpm -C ui lint
pnpm -C ui typecheck
pnpm -C ui test
  Test Files  8 passed (8)
       Tests  29 passed (29)
pnpm -C ui build
  2005 modules transformed; production build completed
```

The build emits a nonfatal size warning: the main JS chunk is approximately 509 kB. The xterm viewer is loaded separately (approximately 333 kB).

Confirmed that the only production Tauri API import and generated-binding imports are in `src/shared/ipc/gateway.ts`. `cn` was removed from dependencies and all generated imports use the existing local utility. Dependency versions remain exact.

## Gaps for main review

- The current API does not list retained environments. Deletion uses a known recorded environment ID rather than presenting an invented inventory.
- Reconnect restores persisted steps and sanitized failure tails; no API exists to reattach/replay historical log lines. The UI explains that live output is session-local and full per-run logs remain on disk.
- No ComfyUI tag-list command exists: an exact tag can be entered, or Latest selected.
- M2 lifecycle controls, M6 accelerators/curated nodes, source build hooks and automatic moved-install rebuild are explicitly deferred in UI copy.
- Desktop WebView2/WebKitGTK smoke, actual picker interaction, visual themes, 200% zoom, and full GPU installation were not performed by this coder. Main owns independent desktop/GPU acceptance.

## Changed files (all relative to ui/)

Modified existing files:

- `package.json`, `pnpm-lock.yaml`
- `src/app/App.tsx`, `src/app/globals.css`
- `src/app/shell/app-shell.tsx`, `src/app/shell/top-bar.tsx`
- `src/features/settings/settings-page.tsx`
- `src/shared/ipc/gateway.ts`, `src/shared/ipc/gateway.test.ts`

Added feature files:

- `src/features/setup/index.ts`
- `src/features/setup/model.ts`, `src/features/setup/model.test.ts`
- `src/features/setup/setup-wizard.tsx`, `src/features/setup/setup-wizard.test.tsx`
- `src/features/setup/run-progress.tsx`, `src/features/setup/run-progress.test.tsx`
- `src/features/setup/use-setup-run.ts`
- `src/features/environment/index.ts`
- `src/features/environment/environment-page.tsx`, `src/features/environment/environment-page.test.tsx`
- `src/features/environment/package-changes.tsx`, `src/features/environment/package-changes.test.tsx`

Added shared files:

- `src/shared/lib/display.ts`, `src/shared/lib/environment-queries.ts`
- `src/shared/testing/environment.ts`
- `src/shared/ui/feedback.tsx`, `src/shared/ui/status-badge.tsx`
- `src/shared/ui/log-view.tsx`, `src/shared/ui/log-view.test.tsx`
- shadcn components: `src/shared/ui/{alert,checkbox,dialog,field,progress,radio-group,skeleton,table,toggle,toggle-group}.tsx`
- This handoff file.
