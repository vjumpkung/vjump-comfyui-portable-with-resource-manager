# HANDOFF — vjumpkung-rust-comfyui-desktop

Last updated: 2026-10-16. **Implementation paused by the user. M0–M4 have earlier Windows checks; M5 is partially integrated and has unresolved backend audit findings; M6 incomplete; README.md absent.** Branch: `feat/m0-skeleton`. Current HEAD verified locally: `f0b35e9` (`chore: disable ci (temp)`). The worktree has uncommitted implementation changes listed below. No commit or push was performed in this session. All delegated agents stopped; no known commands or test sessions remain running.

## 1. What this project is
A Rust desktop app (Tauri 2 + system webview, not Electron) that:
- installs, runs and maintains a local ComfyUI;
- downloads and manages models from CivitAI, Hugging Face, Google Drive and direct URLs;
- manages custom nodes through the Comfy Registry or git;
- manages the Python, venv and torch builds, with a first-run wizard and re-setup.

## 2. Read these first (in order)
1. `docs/REQUIREMENTS.md`: the normative spec (FR/NFR IDs, priorities, architecture §3.3, acceptance scenarios §8, open items §9).
2. `docs/adr/0001-ui-technology.md`: why Vite + React (not native Rust UI or Leptos/Dioxus).
3. `docs/adr/0002-clean-architecture-ddd.md`: the Rust adaptation of Clean Architecture + DDD.
4. `docs/DESIGN.md`: the UI design system, adapted from vjump.dev.

Reference material, read-only and outside this repo:

| Path | Use |
|---|---|
| `D:\vjump\vjumpkung-sd-ui-manager-backend` | Python/FastAPI reference for downloads (CivitAI file selection, HF URL parsing, `hf download` preference) and custom-node install flow |
| `D:\vjump\vjump-resource-manager-frontend` | Next.js reference UI. Port its logic and structure, and restyle it per DESIGN.md. |
| `d:\vjump\pixellatent-happy-meals-scripts\comfyui-python-3-12-happy-meals\portable_version\install_comfyui_3_12_uv_nvidia_torch_2100_portable.bat` (+ `update_comfyui_portable.bat`) | Setup steps, env isolation vars, accelerator wheels, verification, ff-only update safety |
| `d:\vjump\vjump-dev-new-website\DESIGN.md` | Upstream design system (fingerprint sha256 `36a7e467c0c6ba12…`) |

## 3. Decisions already made — do not re-litigate
| Decision | Who | Where |
|---|---|---|
| Release 1 = **Windows x64 + Linux x64, NVIDIA only** | User | §1.4 |
| Moved install → **guided rebuild** of the venv (keep user data); the install is not portable as-is | User | FR-SETUP-12 |
| **Tauri 2.12.x** (not v3 alpha, not raw wry/tao) | Advisor | §2.1 |
| **Vite + React + TS** frontend (Options A and B rejected) | Advisor, user asked | §2.2, ADR-0001 |
| **tauri-specta 2.0.0-rc.25** pinned for IPC bindings; fallback ts-rs 12 | Advisor | §2.1 |
| **Clean Architecture, Rust adaptation:** concrete use-case structs with `execute()`; traits only for DIP/streaming ports | User | §3.3, ADR-0002 |
| Crates per layer: `domain` ← `application` ← `infrastructure` ← `desktop`, plus `ui/`. Contexts are modules inside each crate. | Advisor | §3.3.2 |
| Contexts: `environment`, `comfy_runtime`, `models{acquisition,library}`, `custom_nodes`, `settings`, `shared` | Advisor | §3.3.1 |
| No serde in domain/application. `async-trait` for dyn ports. Errors per layer (`thiserror`). | Advisor | §3.3.2 |
| One **maintenance gate**, no event bus. Model rename/delete allowed while ComfyUI runs. | Advisor | §3.3.3–3.3.4 |
| Failed in-place pip / `install.py` → env **`repair_required`**, no venv snapshot/rollback | User | §6.2, FR-NODE-4 |
| Python **3.13** + torch **2.14.1 cu130** default; 3.12/torch 2.10 and cu126 profiles | Advisor | §2.4, §5.4 |
| uv-managed Python (not the embeddable zip). MinGit subprocess on Windows, system git on Linux. | Advisor | §2.3 |
| Tokens in the OS keyring only; never in URLs, logs or the DB | Advisor | FR-SET-1, NFR-SEC-1/2 |
| Delete → Recycle Bin/trash; never a silent permanent delete | Advisor | FR-MODEL-3 |
| ComfyUI-Manager optional, off by default | Advisor | FR-NODE-10 |
| **HF downloads default to the `hf` CLI** (2.2.0, isolated hash-locked tool venv). **Pause/resume restarts from 0** for that engine. Native engine is a setting, and a fallback only on tool launch failure. | User | FR-DL-3/3a/3b |
| Design system = **vjump.dev**, adapted (`radix-vega`, lucide, Inter/Geist Mono + Noto Sans Thai, bundled) | User | DESIGN.md, NFR-UX |
| UI copy **English only** for release 1 (Thai P2) | User | NFR-UX-2 |

## 4. Open items (spec §9 — need runtime validation)
- Accelerator wheels and their SHA-256 for py3.13 / torch 2.14.1 on both OSes: SageAttention, FlashAttention 2/3, onnxruntime-gpu, llama-cpp-python.
- Native HF engine vs Xet-backed files.
- Whether a newer `hf` restores cross-run resume (if so, revisit FR-DL-3a).
- WebKitGTK performance for the ComfyUI canvas; Thai font rendering on Linux.
- tauri-specta rc.25 compatibility with pinned Tauri 2.12.4 was validated at M0; retain the binding drift check.
- Windows code signing and updater key custody.

## 5. Implementation status and next steps

The full objective remains to finish every milestone through M6, then write `README.md` documenting the program. The goal is currently **paused**, verified through the goal tool. The latest user request was only to update this handoff for a future session. Resume implementation when the user resumes the work. Do not treat earlier green checks as proof that later edits pass.

| Milestone | Status / evidence |
|---|---|
| M0 Skeleton | Workspace, settings, SQLite, credentials, redacted logging, typed IPC, capability isolation and CI implemented. |
| M1 Setup | Setup backend, wizard and Environment page implemented. Earlier session reported a real RTX 3060 setup with Python 3.13.16 / torch 2.14.1 cu130, successful CUDA verification and healthy Repair. UI handoff: `ui/M1-UI-HANDOFF.md`. |
| M2 ComfyUI runtime | Runtime supervisor, launch/update/close flows, desktop commands and UI implemented. Earlier Windows checks passed; Linux and full native close/navigation acceptance remain unproven. |
| M3 Downloads | Provider adapters, hf/native engines, queue, durable finalization, commands and UI implemented. Late overwrite decisions now echo the observed file identity and reject stale decisions. Real provider acceptance remains outstanding. |
| M4 Library | Scanning/watching, model operations, identification, commands and UI implemented. Last main-agent check reported 369 Rust tests and 73 UI tests passing, clean fmt/clippy/architecture/lint/typecheck and UI build. These checks predate M5. |
| M5 Custom nodes | Existing domain/application/infrastructure modules, migration 006, desktop node DTOs/commands, state composition, permissions and valid-body ACL coverage were inspected. Partial frontend files and gateway wrappers now exist, plus a new repository browser command. The Custom Nodes page is still absent/unwired. Backend audit found repair, recovery, Git, registry warning and runtime gate gaps below. M5 is not complete. |
| M6 Release 1 | Advisor plan approved with corrections. CI/release workflow files exist, but their presence is not completion evidence. Accelerators, curated nodes, moved-install rebuild, updater, global bandwidth and diagnostics need implementation/audit. |
| README | Does not exist. Write after the implementation audit, documenting actual behavior and remaining limitations. |

### Resume here
1. Read current `git status` and the actual diff. Preserve the existing uncommitted changes; the historical “Now state.rs modifications” stopping point is obsolete. `NodeServices`, recovery composition, all original 15 `node_*` command registrations and their valid-body ACL tests were already present at session start.
2. Finish the repository opener validation and regenerate bindings through the explicit ignored export test. Wire the opener through the gateway. Finish the Custom Nodes page and app-shell route using the partial files below. Do not assume these frontend files typecheck or correctly implement all consent states yet.
3. Fix the M5 backend findings below, with regression tests for the actual failure/recovery boundaries. Repair and launch gating are correctness work, not optional polish.
4. Run current M5 checks, then implement M6 against its saved plan plus advisor corrections and the gap map below. Audit all P0/P1 requirements and acceptance evidence before writing README and claiming completion.
5. Preserve fixed decisions in section 3. Do not claim Linux, real-provider, native recycling, scale/performance or release acceptance without evidence from those environments.

### Changes actually made in this session

No domain/application/infrastructure fixes were made. The two agents investigating those layers stopped before editing. The main agent added partial UI integration; a third agent implemented the repository browser command.

| Files | Current change / unfinished integration |
|---|---|
| `ui/src/shared/ipc/gateway.ts` | Added wrappers for the existing node search/list/rescan/prepare/commit/enable/uninstall/update-all/cancel/status/subscription commands. `nodeOpenRepository` is not added yet. |
| `ui/src/features/custom-nodes/model.ts` (new) | Source labels, initial unchecked consent, eligibility and exact-plan commit payload helpers. Needs tests and audit against backend trust/consent semantics. |
| `ui/src/features/custom-nodes/use-nodes.ts` (new) | React Query list, subscription retry/cleanup, epoch/sequence/gap invalidation, bounded live logs and 5-second list polling. Needs tests for reconnects, gaps, stale events, teardown and operation-state refresh. |
| `ui/src/features/custom-nodes/review-dialog.tsx` (new) | Partial exact-source review with hashes, requirements, warnings, protected changes and explicit execution/script/trust controls. No typecheck or rendering check was run. Audit consent reset when plans/bindings change and remembered script permission when controls are hidden/unchecked. |
| `crates/desktop/src/ipc/node_commands.rs` | Added `node_open_repository(repository: String)`, normalized HTTPS validation through infrastructure `validate_remote`, and two URL-validation unit tests. Uses `state.shell()?.open_browser(url)`. Existing automatic webview link handling is disabled in `lib.rs`, so explicit IPC is needed. |
| `crates/desktop/src/ipc/bindings.rs`, `crates/desktop/build.rs`, `crates/desktop/capabilities/manager.json` | Registered the new opener and manager-only permission. |
| `crates/desktop/tests/acl.rs`, `crates/desktop/tests/support/mod.rs` | Added opener valid-body ACL case and a counting fake `ShellActions` implementation so tests do not open the OS browser. |
| `crates/desktop/permissions/autogenerated/node_open_repository.toml` (new, generated by build) | Generated opener permission; retain and review with the rest of the command integration. |
| `AGENTS.md` (untracked at session start) | User/repository instructions, not created by this implementation. Preserve it. |

`ui/src/shared/ipc/bindings.ts` was exported before the opener was added and produced no diff. It is now **out of date with the new command collection**; ordinary tests must not overwrite it. Regenerate explicitly after finishing the command:

```powershell
cargo test -p desktop --test bindings --locked -- --ignored export_bindings
```

The opener validator still needs explicit rejection of an **empty raw authority**, plus a regression test for malformed spellings such as `https:////@github.com/example/node`. Its raw delimiter check currently rejects `@` but permits an empty authority before URL normalization. Do not take the two passing unit tests as complete coverage of this case.

Still missing from frontend: `custom-nodes-page.tsx`, feature `index.ts`, and app-shell route (sidebar item already exists). Build registry browse/search/pagination/manual cache refresh with metadata and unknown/stale states; registry release/version vs Git install; installed-node filtering and keyboard-aware virtualization above 200 rows; exact-target update/retry review; sequential Update all with per-node results; enable/disable/uninstall confirmation; errors/logs/cancellation/recovery; Stop-and-retry and Restart after successful verification; Repair routed to existing setup facade. Keep feature composition in `ui/src/app/` and all Tauri access in the gateway. Reloading/unmounting UI must not cancel backend work.

### M5 backend audit findings — not fixed

The following are code-review findings, not claims that every issue has been reproduced at runtime. Re-read current code before implementing; line numbers are navigation hints from this audit.

1. **Repair must restore node dependencies and preserve failure state.**
   - `crates/infrastructure/src/environment/source.rs` around 459–485 only collects root `requirements.txt` files. Registry-only pip dependencies are persisted in `node_operations.requirements_json` but are omitted during side-by-side rebuild. `NodeService::repair_requirements()` in application `custom_nodes/use_cases.rs` around 398–415 is unused and also file-only.
   - `crates/application/src/environment/execute.rs::repair_check` around 342–375 can clear `RepairRequired` from core verification/pins alone despite failed node dependencies. `comfy_requirements` around 726–768 turns node requirement failures into warnings and can permit replacement activation.
   - `node_env_deps` is written in infrastructure `custom_nodes/sqlite.rs` but not read. List/scan state retains a global dependency flag across environment activation. Outcomes must be selected by environment ID plus exact node source revision, including disabled nodes.
   - Proposed integration, **not implemented and not yet reviewed as a final design**: add `ComfySource` repair-requirement/outcome methods in application `environment/ports.rs`; inject `NodeRepository` into infrastructure `GitSource` using the existing SQLite connection in desktop `state.rs`; select latest consented install/update/retry operation matching current installed node and exact source; add outcome lookup to custom-node port/SQLite adapter; install persisted registry dependencies with existing constraints; force replacement for genuine node dependency repair and refuse Repair activation on failure. Never re-resolve a different registry release, import Python, or rerun `install.py` without consent. Add adjacent source/setup/custom-node regression tests.
2. **Recovery must prove identity before moving or recycling copies.** `crates/infrastructure/src/custom_nodes/files.rs` around 793–875 trusts existence for enable/disable/uninstall/restored backups and recycles backups in `Verified`/`CleanupPending` without checking the published identity. Validate recorded source/destination/backup identities; preserve changed/ambiguous copies and unresolved state, especially the only recoverable old copy. Add foreign-destination, changed-published-tree, modified-backup and crash-boundary tests.
3. **Git tree revalidation is incomplete.** `files.rs` around 154–160 skips the full tree check for Git snapshots; update around 607–622 lacks post-apply tree validation. Add clean-worktree and link/reparse checks at staged snapshot/revalidation and after frozen-target apply; reject staged file changes beyond root requirements/hook. Preserve dirty/diverged refusal and immutable consent binding.
4. **Final digest after hooks needs deliberate handling.** A consented `install.py` may modify node files. Before persisting `Verified`/`CleanupPending`, refresh/persist the verified final digest so later recovery distinguishes the successful final tree. Coordinate this application workflow change with stricter infrastructure recovery checks rather than making recovery reject every legitimate hook mutation.
5. **Registry warnings/refusals are lost.** `files.rs` around 252 ignores metadata GET errors; around 316–319 node-level flagged/deprecated state is not merged into release warnings. Propagate service refusals and bind all applicable warnings to the exact review.
6. **Launch checks must run under the maintenance lease.** `crates/desktop/src/ipc/comfy_commands.rs` around 54/74 calls `nodes.ensure_startable()` before Start/Restart acquires the gate. An intervening node failure can create unresolved publication before launch. Put the required journal check in held application orchestration. Restart currently calls runtime restart directly and bypasses awaited stop/rescan. `NodeServices::after_stop` in `state.rs` around 488–493 swallows Busy/recovery failures. Resolve stop/reconcile/start ordering without reacquiring the same gate.
7. **Filesystem parent containment remains weak.** `files.rs::ensure_layout` around 89–97 checks `custom_nodes/.disabled` but follows a `ComfyUI` parent symlink; pinned-parent protection described in the port contract is not implemented. Audit operation-time parent substitution/reparse races, not only lexical paths.

### M6 audit gap map — implementation not started this session

| Area | Evidence and next implementation work |
|---|---|
| Accelerators (FR-SETUP-8, NFR-SEC-7) | All seven profile JSONs have `accelerators: []`; profile tests explicitly defer them to M6. `environment/execute.rs` forwards a loose requirement without using `sha256`; queries report no accelerators. Implement exact HTTPS wheel URL/hash/filename/provenance/module/no-deps records, controlled download/hash/local-wheel install with pin guard, selections and actual results in request/manifest/UI. Default py313/314 stays empty until hashed wheels pass real import verification; do not invent hashes. Reference compatibility-wheel URLs are in saved `ev-installer.md` around 61–67. |
| Curated nodes (FR-SETUP-9) | Setup unconditionally skips this step; no bundled curated list/selections exist. Saved `ev-installer.md` around 118–139 contains 22 ordered URLs. Add default-on selection and exact-commit review through M5 consent/publishing/dependencies. **Integration prerequisite:** `NodeService::prepare_held` and `commit_held` currently select active Verified env; first-run setup target is not active yet. Introduce explicitly validated/bound setup-target support under the existing lease, not a call that silently uses the old active env. Individual warnings may continue, but dependency damage must remain reflected in env health. |
| Moved-install rebuild (FR-SETUP-12, acceptance scenario 4) | Status does not compare recorded root or probe interpreter; no guided rebuild/open-existing-install flow or root-path migration exists. Add root mismatch, managed `pyvenv.cfg.home` containment and bounded interpreter probe; side-by-side rebuild with recorded profile/components and all node requirements including disabled, no automatic hooks; recreate hf tool venv and migrate old-root persisted paths. Inspect startup missing-root behavior before creating directories. Main files: application `environment/{queries,model,setup,execute,activation}.rs`, infrastructure `{manifest,python,sqlite,switcher}.rs`, shared paths, desktop state/env commands, setup wizard/Settings/App. |
| Updater (FR-SET-4) | No dependency/plugin/commands/UI. The updater config overlay only enables bundle artifacts. Add pinned plugin per plan (`=2.14.0`), optional build-time public key/endpoint, Rust-only check/download/install/status, maintenance/runtime checks/progress/restart, and check-on-launch default false. Extend IPC/ACL/bindings/gateway. Remain inert and hide update action without complete configuration. |
| Global bandwidth (FR-DL-14) | Setting persists but is unused; Settings cap field disabled; native stream and hf are unthrottled. Add one shared live-configurable bucket to acquisition stack/native transfers and allow-listed loopback CONNECT relay for hf. Preserve upstream proxy chaining, opaque TLS, CONNECT host/443 validation, child-only HTTP(S)_PROXY and cleared NO_PROXY. Combined native/hf and Xet proxy honoring require actual runtime evidence. |
| Diagnostics (§6.7) | Existing ComfyUI diagnostics has runtime details plus 200 runtime log lines, not full app diagnostics. Add app/uv/hf versions, CUDA, manifest/profile and last 200 app-log lines; redact the entire output. Add collection command and Rust clipboard integration (`=2.5.0` per plan), with textarea fallback; extend IPC/ACL/UI. |
| Packaging/release (§6.3) | Windows/Linux overlays and placeholder icons exist. Release workflow is entirely commented out, consistent with currently disabled CI; presence is not release evidence. Audit launcher overlay discovery, build NSIS/MSI and Linux AppImage/deb, dependencies/fonts, install/launch/single-instance/ACL/uninstall. Linux and native acceptance remain unproven. Keep signing/key custody/license/logo decisions as documented below. |
| README | Still absent. Write after implementation audit, documenting actual setup/rebuild, node consent/repair, model/provider/queue behavior, resource_list format, security, prerequisites, troubleshooting, development/checks/binding regeneration, release/updater setup, third-party notices and license status. Explicitly distinguish verified behavior from remaining OS/provider/runtime limitations. |

Recommended order after M5: setup-target support; accelerator selection/persistence plus curated setup/rebuild; bandwidth; updater/diagnostics; packaging and complete requirement audit; README. This is an implementation sequence, not a reduction of release scope.

### Verification observed in this session

| Check | Result and scope |
|---|---|
| `cargo check --workspace --locked` | Passed before the new opener changes. Establishes compilation of the existing M5 Rust implementation, not final-worktree verification. |
| `cargo test -p application custom_nodes --locked` | 19 passed, 0 failed (87 filtered). Does not cover the newly identified recovery/repair gaps. |
| `cargo test -p desktop --test acl --locked` | Original 15-node-command coverage passed; after adding opener/fake shell, all 6 integration tests passed again. Covers command completeness, local manager routing and ComfyUI/remote-origin denial with valid bodies. |
| Repository opener unit tests | Agent reported 2 passed, exit 0 (`node_commands` targeted tests). Empty raw authority case is not covered yet. |
| Targeted rustfmt | Agent formatted only the changed Rust command/build/bindings/ACL/support files; no full-workspace fmt check was run. |
| Explicit bindings export | Passed before opener addition and produced no change. Export must be repeated after the new registration; current checked-in bindings are stale. |
| `pnpm -C ui exec shadcn info --json` | Confirmed existing Vite/Radix/radix-vega project and installed components. This is discovery, not frontend validation. |

**Not run on this changed worktree:** UI lint/typecheck/tests/build, full workspace tests/clippy/fmt/architecture, binding drift after opener, coverage, production desktop build, native/manual/provider/Linux acceptance. No final completion claim is justified. Test handles `79709` (ACL) and `18021` (opener unit tests) were polled terminal exit 0; no background work should be assumed live next session.

### Saved plans and advisor corrections
The previous session saved plans outside the repo at:

`C:/Users/vjump/AppData/Local/Temp/claude/D--vjump-vjump-comfyui-portable-with-resource-manager/d0238eb2-6d30-4e01-a5ad-f9ad8b7880d0/scratchpad/`

Read each plan together with its advisor corrections: `M1-PLAN.md` / `M1-ADVISOR.md`, `M2-PLAN.md` / `M2-ADVISOR.md`, `M3-PLAN.md` / `M3-ADVISOR.md`, `M4-M5-PLAN.md` / `M45-ADVISOR.md`, and `M6-PLAN.md` / `M6-ADVISOR.md`. These temporary files may disappear; normative repo docs remain authoritative.

M6 corrections include hashed exact accelerator wheels; one shared bandwidth bucket for native transfers and an allow-listed loopback CONNECT proxy for hf; side-by-side moved-install rebuilding; curated nodes through M5 consent; optional build-time updater configuration; redacted diagnostics; packaging and release checks. Updater key custody, Authenticode and project license remain owner decisions. Keep updater inactive without configuration, placeholder icons, and no invented license grant.

### Verification commands
Build the real UI before desktop production embedding. Ordinary tests must not rewrite checked-in bindings.

```powershell
pnpm -C ui install --frozen-lockfile
pnpm -C ui lint
pnpm -C ui typecheck
pnpm -C ui test
pnpm -C ui build
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
node --test scripts/check-arch-deps.test.mjs
node scripts/check-arch-deps.mjs
cargo test -p desktop --test bindings --locked
cargo build -p desktop --locked --features tauri/custom-protocol
```

Also retain coverage targets (domain 90%, application 80%), mutation checks for architecture/ACL/binding drift, and the manual acceptance scenarios in REQUIREMENTS and advisor reviews. Last reported coverage was measured before later milestones and must be remeasured.

## 6. Working agreement and historical tooling
- Follow current AGENTS.md and runtime tools. The prior CLI workflow was: explorer (Codex `gpt-6-luna` xhigh, read-only) → plan → advisor (Codex `gpt-6.1-sol` xhigh, read-only) → coder (Codex `gpt-6.1-sol` xhigh, write) → verify.
- Historical Claude CLI routing (use only if available in the current runtime): call Codex via `/codex:rescue` with explicit `--model`, `--effort`, `--fresh`, scope and success criteria. Fallbacks: Haiku 5.5 (explore), Sonnet 5.5 (code).
- Ask the user only about decisions the advisor classifies as user-only.
- Background Codex jobs: check with `node C:/Users/vjump/.claude/plugins/cache/openai-codex/codex/1.0.6/scripts/codex-companion.mjs status`, then `result <task-id>`.
- Local clean-architecture plugin: `C:\Users\vjump\.claude\plugins\cache\anthropic-plugin-directory\clean-architecture\1.0.0-5a68049e6455\CLAUDE.md`. Its per-use-case port rule is intentionally adapted (ADR-0002).
- Commit only when the user asks. Branch off `main` first.
