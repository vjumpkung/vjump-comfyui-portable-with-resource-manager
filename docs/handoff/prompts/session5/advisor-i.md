ROLE: advisor (read-only). Do NOT edit any file and do NOT run git write commands. Read files and run read-only commands only (rg, cat, git log/show/diff). Do not run cargo/pnpm builds or tests.

REPO: D:\vjump\vjump-comfyui-portable-with-resource-manager (Rust/Tauri 2 + React/TS). Read AGENTS.md, HANDOFF.md (§0a, §3, §5.3 row "I"), docs/handoff/plans/m6-approved-plan.md section "I — Integrate IPC, recovery composition and UI", docs/handoff/evidence/exp-ui-i.md, docs/handoff/evidence/s4-reviews.md ("Notes for package I", E3 open points), docs/DESIGN.md.

## Task
Review the coder prompt drafted by main for M6 package I: docs/handoff/prompts/session4/i-DRAFT-needs-advisor-review.md. Check every claim in it against the CURRENT code at HEAD a4175d1 (the owner committed all session-4 work: E1 accelerators, E2 curated nodes, E3 relocation/rebuild are in HEAD). In particular verify against: ui/src/shared/ipc/bindings.ts (generated), crates/desktop/src/ipc/env_dto.rs, env_commands.rs, crates/desktop/src/state.rs (RecoveryInfo, compose restricted mode), crates/application/src/environment/{setup.rs,model.rs,relocation.rs}, crates/domain/src/environment/profile.rs (accelerator/component specs), ui/src/features/setup/*, ui/src/app/App.tsx, ui/src/app/shell/app-shell.tsx, ui/src/features/environment/environment-page.tsx, ui/src/features/custom-nodes/index.ts, ui/src/shared/ipc/gateway.ts(+test), crates/desktop/tests/acl.rs.

New facts since the draft (main verified 2026-10-20):
- The committed app panics at startup (crates/desktop/src/lib.rs:131 `app.state::<AppState>()` before setup's `manage`). A separate fix package (P0) runs BEFORE I; it only touches crates/desktop/src/lib.rs (+ a regression test). Assume it is fixed when I runs.
- With that line disabled, the UI runs; the Options step still shows the stale "M1 scope — Optional accelerators and curated custom node packs are deferred … Runtime Start / Stop controls arrive in M2" info box (ui/src/features/setup). Confirm the draft removes ALL stale milestone copy (grep for "deferred", "later milestone", "M1 scope", "M2", "M6", "Coming in").
- The machine now has an NVIDIA GeForce RTX 3060 (driver 610.88 / CUDA 13.3). Not relevant to I's code, but R (next package) can now run native NVIDIA acceptance.

## Decisions needed (classify AI-decidable vs user-only; decide the AI-decidable ones with evidence)
D1. E3 open point D8: `open_existing` also refuses the recorded OLD root itself, so a user in the moved/restricted state cannot "Locate" the original install from the UI. Should I (UI) offer "Locate" in the rebuild_required/moved state at all, and should the backend allow re-pointing to the recorded old root (it is the user's original install, e.g. drive letter came back)? If a backend change is needed, specify it exactly (file/function/test) or defer it as a documented residual.
D2. Exact shape of the three DTO additions (ProfileDto.accelerators, SetupRequestDto.accelerators, EnvironmentInfoDto.components) given the real domain/application types — field names, which experimental flag source, status enum values — with file:line.
D3. How setup must send a Rebuild request (which SetupRequestDto fields are required/ignored for mode 'rebuild') per crates/application/src/environment/setup.rs.
D4. Whether the curated batch review can reuse the M5 ReviewDialog via the custom-nodes public barrel without breaking the ESLint boundary rules (check eslint.config.js), and the exact injection mechanism from ui/src/app.
D5. Anything in the draft that is wrong, missing, or over-scoped vs the m6 plan section I (required integration + required UI tests). Keep scope to plan section I.

## Required output
go/no-go; a CORRECTED, complete coder prompt (ready to paste, same structure as the draft, with exact files, DTO shapes, test names and success criteria); decisions + assumptions with file:line evidence; risks; success checks; only indispensable user questions.
