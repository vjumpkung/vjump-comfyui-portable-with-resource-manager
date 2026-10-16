ROLE: coder (write-capable). Mechanically implement the approved plan below. Do not redesign or expand scope. If underspecified or conflicting with the code, stop and report the gap.

WORKING COPY: C:\Users\vjump\Documents\vjump-comfyui-portable-with-resource-manager (main repo). Another coder is concurrently editing Rust files under crates/ in this same working copy: do NOT edit, format or revert anything under crates/, and do not run cargo. You own only files under ui/ (listed below). Do not commit.

MANDATORY: read C:\Users\vjump\AppData\Local\Temp\claude\C--Users-vjump-Documents-vjump-comfyui-portable-with-resource-manager\ce429ed4-f6f3-49eb-9e87-13e9ba4d84af\scratchpad\coder-env.md (repo rules; TypeScript conventions) and docs/DESIGN.md (tokens, status chips never color-only, keyboard, 200% zoom). Use the shadcn components already in ui/src/shared/ui; add none from the network.

GOAL: finish the M5 Custom Nodes UI.

APPROVED SPEC (authoritative): C:\Users\vjump\AppData\Local\Temp\claude\C--Users-vjump-Documents-vjump-comfyui-portable-with-resource-manager\ce429ed4-f6f3-49eb-9e87-13e9ba4d84af\scratchpad\adv-m5-frontend.out.md sections "WP2 — Correct pure consent and eligibility rules", "WP3 — Finish list, operation and Registry hooks", "WP4 — Build Registry installation and exact review", "WP5 — Build installed-node controls and batch updates", "WP6 — Compose page, errors, operations and runtime actions", plus all frontend tests in its section 5 table (model.test.ts, review-dialog.test.tsx, use-nodes.test.tsx, use-registry.test.tsx, installed-table.test.tsx, custom-nodes-page.test.tsx, update-all-dialog.test.tsx, ui/src/app/shell/app-shell.test.tsx). The IPC contract (WP1) is ALREADY implemented: use the generated ui/src/shared/ipc/bindings.ts and gateway.ts (nodeOpenRepository, nodeGitStatus, NodeListDto.pendingReviews/recent, OperationDto.nodeId/blocksStart, RegistryNodeDto raw metadata). Read them first; if a needed DTO field is missing, report it instead of inventing it.

BACKEND BEHAVIOUR NOTES (accepted backend plan, being implemented concurrently; code the UI for them):
- Node plans never contain protected-package changes anymore (backend refuses them with node_resolution_failed); keep the protected-changes UI defensive (render if present, require confirmation) but expect an empty list.
- New IPC error codes: node_recovery_required (start/restart blocked by interrupted node work), node_rescan_failed (ComfyUI stopped but node rescan failed; offer Rescan), node_git_refused messages now include bounded git status text.
- Start/Restart run node checks under the maintenance lease; Stop waits for the gate then rescans.

FILES YOU MAY TOUCH: ui/src/features/custom-nodes/** (new + existing partial files), ui/src/features/comfy-runtime/index.ts (export useRuntime and useRuntimeActions only — create useRuntimeActions as a thin public hook wrapping existing mutations if it does not exist, inside comfy-runtime), ui/src/app/shell/app-shell.tsx (+ app-shell.test.tsx), ui/src/app/App.tsx only if needed to pass onSetup('repair').

SUCCESS CRITERIA: pnpm -C ui lint, pnpm -C ui typecheck, pnpm -C ui test, pnpm -C ui build all pass. All named tests exist and pass.

OUTPUT: per-file summary, exported components/hooks and props, tests added (names), exact command results, and any spec gaps found.
