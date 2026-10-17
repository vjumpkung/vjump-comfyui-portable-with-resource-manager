# HANDOFF — vjumpkung-rust-comfyui-desktop

Last updated: 2026-10-17 15:30 (+07:00). **Paused by the user mid-M5 (WP2+WP3 in progress).** All subagents and Codex jobs were stopped on request; no build, test or agent process is running. Branch `feat/m0-skeleton`, HEAD `0ffa136` (the owner committed the previous session's partial M5 work + `docs/handoff/` as `38ea8ae`/`0ffa136` "feat: implement M6" — the M6 code itself is **not** in HEAD). Nothing from this session is committed. The main working copy holds **verified** WP6 + WP1 + B work, draft docs, and **unverified partial WP2+WP3 edits** (§5.3). README.md and docs/DOCUMENTATION.md exist as drafts with `<!-- VERIFY -->` markers.

## 1. What this project is
A Rust desktop app (Tauri 2 + system webview, not Electron) that:
- installs, runs and maintains a local ComfyUI;
- downloads and manages models from CivitAI, Hugging Face, Google Drive and direct URLs;
- manages custom nodes through the Comfy Registry or git;
- manages the Python, venv and torch builds, with a first-run wizard and re-setup.

## 2. Read these first (in order)
1. `docs/REQUIREMENTS.md`: the normative spec (FR/NFR IDs, priorities, architecture §3.3, acceptance scenarios §8, open items §9).
2. `docs/adr/0001-ui-technology.md`, `docs/adr/0002-clean-architecture-ddd.md`.
3. `docs/DESIGN.md`: UI design system.
4. `docs/handoff/`: approved plans (`plans/`), evidence (`evidence/`), coder prompts (`prompts/`, current set in `prompts/session2/`), saved patches (`patches/`) and local build tooling (`tooling/`).

Reference material:

| What | Where now |
|---|---|
| Reference portable installer (setup steps, 22 curated nodes, accelerator wheels) | Owner's private GitHub repo `vjumpkung/pixellatent-happy-meals-scripts`, file `comfyui-python-3-12-happy-meals/portable_version/install_comfyui_3_12_uv_nvidia_torch_2100_portable.bat` (`gh api repos/.../contents/<path> -H "Accept: application/vnd.github.raw"`). Everything M6 needs is extracted into `docs/handoff/plans/m6-reference-data.md`. |
| Python backend reference | `C:\Users\vjump\Documents\vjumpkung-sd-ui-manager-backend` (not needed for remaining work). |
| Code inventory for documentation | `docs/handoff/evidence/docfacts.md` (explorer, 2026-10-17, file:line evidence of the committed app). |

## 3. Decisions already made — do not re-litigate
| Decision | Who | Where |
|---|---|---|
| Release 1 = **Windows x64 + Linux x64, NVIDIA only** | User | §1.4 |
| Moved install → **guided rebuild** of the venv (keep user data) | User | FR-SETUP-12 |
| **Tauri 2.12.x**; **Vite + React + TS** frontend; **tauri-specta 2.0.0-rc.25** pinned | Advisor/user | §2.1–2.2, ADR-0001 |
| **Clean Architecture, Rust adaptation**; crates `domain` ← `application` ← `infrastructure` ← `desktop` + `ui/`; contexts as modules; no serde in domain/application; `async-trait` dyn ports; errors per layer | User/advisor | §3.3, ADR-0002 |
| One **maintenance gate**, no event bus; model rename/delete allowed while ComfyUI runs | Advisor | §3.3.3–3.3.4 |
| Failed in-place pip / `install.py` → env **`repair_required`**, no venv snapshot | User | §6.2, FR-NODE-4 |
| Python **3.13** + torch **2.14.1 cu130** default; 3.12/torch 2.10 and cu126 profiles; uv-managed Python; MinGit on Windows, system git on Linux | Advisor | §2.3–2.4, §5.4 |
| Tokens only in the OS keyring; delete → Recycle Bin/trash; ComfyUI-Manager optional/off | Advisor | FR-SET-1, FR-MODEL-3, FR-NODE-10 |
| **HF downloads default to the `hf` CLI**; pause/resume restarts from 0 for that engine | User | FR-DL-3/3a/3b |
| Design system = vjump.dev adapted; UI copy English only | User | DESIGN.md, NFR-UX-2 |
| CI workflows stay **disabled** (owner commit `chore: disable ci (temp)`); run checks locally | User | `.github/workflows/*` |
| Updater key custody, Authenticode, project license, final icons = **owner decisions**; updater stays inert without build-time config; no license added | Owner | IMPLEMENTATION_STATUS |

### 3.1 Advisor decisions (AI-decidable — accepted; details in `docs/handoff/plans/*`)
- **Custom-node containment:** real pinned-parent directory operations for all node mutations (NFR-SEC-5 check/use races), incl. pinned recycle with no permanent-delete fallback.
- **WP1 gap decision (new, 2026-10-17, `plans/m5-wp1-gap-decision.md`):** registry archive downloads into a create-new pinned file reused for extraction; a typed **`NodeLocationGuard`** port keeps pins and pinned inputs alive through preview, dependency install, hook execution and cleanup (incl. `CleanupFailed`); documenting the subprocess race as a limitation was rejected; **WP5 amendment:** `NodeRequirementSet.file` must keep its guard (or a protected snapshot) alive through provisioning.
- **Git node identity = content tree digest** (not `git:<commit>`); Git updates stage a full checkout of the frozen target; post-apply validation before deps/hooks; `GitRefused { status }` keeps bounded `git status`.
- **Node operations can never authorize protected-package (torch/triton) changes**; constrained preview only; FR-ENV-3 keeps its own confirmation flow.
- **Final tree digest** must equal the reviewed digest unless a consented hook ran (then refresh); ambiguous recovery in any non-terminal phase **blocks launch** (`NodeOperation::blocks_start`).
- **Repair/replacement:** node requirement sets come from a `NodeRequirementSource` port implemented in application `custom_nodes/requirements.rs`; any node requirement failure is **fatal in every replacement mode**; `RepairRequired` always rebuilds; dependency state is per `(node, env, dependency_identity)`; missing = `not_run`. No migration for this.
- **Launch lifecycle:** `NodeLifecycle` runtime port; Start = lease → node check → spawn; Restart = one lease: stop → reconcile/rescan → check → start; Stop = ungated stop, then waiting acquire, re-verify, awaited rescan; errors `node_recovery_required`, `node_rescan_failed`. **Implemented (WP6).**
- **Frontend contract additions:** raw registry metadata in `RegistryNodeDto`; read-only `node_git_status`; `NodeListDto.pendingReviews/recent`; `OperationDto.nodeId/blocksStart`; Restart via `comfyRestart`. **Implemented (C0, committed).**
- **M6:** curated nodes run **before activation against an explicitly bound setup candidate** (`NodeTarget::Setup(RunId)`, `SetupNodes` port injected into `ExecuteSetup`), with exact M5 review (selected curated plans as **one batch** in the multi-plan review dialog), never running `install.py`; accelerators = hashed wheels + package component, py312 wheels **experimental and unchecked**, py313/314 lists empty; relocation detected before any startup reconciliation, separate `setup_open_install` / `setup_new_install`, rebuild into a new app-owned env with the exact recorded profile revision; HF relay for **every** HF attempt; updater env names **`VJUMP_UPDATER_PUBKEY` / `VJUMP_UPDATER_ENDPOINT`**, binds install to the reviewed offer, **no check-on-launch setting**; diagnostics redact the whole report and use the clipboard plugin from Rust with a textarea fallback.
- **Main's integration decisions (this session):** E2 and E3 coders may make the minimal desktop wiring needed to make their packages testable (commands, ACL cases, bindings regeneration, compile-fix TS); package I then only builds the UI and remaining composition.

## 4. Open items (spec §9 — need runtime validation)
- Accelerator wheel compatibility (hashes ≠ ABI/CUDA proof); FA3 import module name unverified (E1 records it as unverified, no import check).
- Native HF engine vs Xet files; whether newer `hf` restores cross-run resume; the opt-in relay test `real_hf_xet_obeys_relay_with_direct_egress_denied` has not been run.
- WebKitGTK performance and Thai fonts on Linux; Windows code signing and updater key custody.
- **Linux-only code paths of WP1 (pinned read pins, `/proc/<pid>/fd/<n>/.` anchored cwd for uv/Python, Unix substitution tests) have never been compiled or run** — this machine has no Linux target.
- **This machine cannot produce NVIDIA or Linux evidence.** All CUDA/setup/accelerator/Linux acceptance remains open; Windows packaging only partly covered (unsigned NSIS/MSI bundles built on 2026-10-17 from an older tree, installers not run).

## 5. Implementation status and next steps

### 5.1 Environment facts (important)
- Machine: Windows 11 x64, **AMD Radeon GPU only (no NVIDIA)**, 12 cores, **15 GB RAM, usually 2–5 GB free** (the owner's VS Code + rust-analyzer hold ~2–2.7 GB). Two concurrent cargo builds at `-j 3` pushed commit charge to ~22/22 GB and Claude Code's **low-memory reaper killed background shell wrappers** (the `codex exec` children survived as orphans and finished; re-attach with PowerShell `Wait-Process -Id <pid>` in a background shell). **Run at most one heavy Rust build plus one `-j 2` build at a time.**
- **Build toolchain = MSVC (default)**: VS 2022 Community 17.14 with MSVC v143 14.44.35207 + Windows 11 SDK 10.0.22621. Cargo uses `rust-toolchain.toml` (`1.99.0-x86_64-pc-windows-msvc`) and default `target/`; works from Git Bash and PowerShell. If components go missing (`LNK1181 kernel32.lib`), add them with an elevated `"C:\Program Files (x86)\Microsoft Visual Studio\Installer\setup.exe" modify --installPath "C:\Program Files\Microsoft Visual Studio\2022\Community" --add Microsoft.VisualStudio.Component.VC.Tools.x86.x64 --add Microsoft.VisualStudio.Component.Windows11SDK.22621 --passive --norestart`.
- **GNU toolchain = fallback only:** `source docs/handoff/tooling/gnu-env.sh` in a separate Git Bash shell (target dir `target-gnu`).
- **Codex:** use `codex exec` directly (`docs/handoff/tooling/run-codex.sh`, set `CODEX_SCRATCH` to a folder with `<name>.prompt.md`); the plugin companion route's sandbox is broken here. **Usage limit hit again at ~14:34 on 2026-10-17 (reset stated 18:14)** after ~1 h of three parallel xhigh jobs. **Fallback that worked well:** Agent tool, `general-purpose`, `model: sonnet`, `effort: xhigh`, pointing it at a prompt file plus the isolated partial delta of the interrupted run.
- Generated permission TOMLs under `crates/desktop/permissions/autogenerated/` show as modified after builds; line-ending noise only.
- Vitest is memory-hungry with 25+ isolated workers; `VITEST_MAX_WORKERS=2` keeps it stable.

### 5.2 Milestone status
| Milestone | Status |
|---|---|
| M0–M4 | Implemented in earlier sessions. |
| M5 Custom nodes | C0 contract + UI committed (`0ffa136`; 145 UI tests). **WP6 done, WP1 done (incl. gap fix), WP2+WP3 partial (unverified), WP4/WP5 not started.** |
| M6 Release 1 | **B done and merged into main. E1 done (verified patch, not merged). P done (verified in worktree, not merged).** E2, E3, I, R not started. |
| Docs | README.md + docs/DOCUMENTATION.md drafted (25 `<!-- VERIFY -->` markers, 1 in README); `docs/RELEASE_ACCEPTANCE.md` not written; IMPLEMENTATION_STATUS not refreshed. |

### 5.3 Work packages — exact state
Specs: M5 backend = `docs/handoff/plans/m5-backend-approved-plan.md` (+ `m5-wp1-gap-decision.md`); M5 frontend = `m5-frontend-approved-plan.md`; M6 = `m6-approved-plan.md` + `m6-reference-data.md`. **Current coder prompts: `docs/handoff/prompts/session2/<pkg>.md`** — append `session2/common.md` (build/memory/repo rules) when using them; worktree prompts (`e1.md`, `p.md`) contain a `__WT__` placeholder for the worktree path.

| Package | Location | State | Verification observed (2026-10-17, MSVC) |
|---|---|---|---|
| **C0 + FE** (M5 contract + Custom Nodes UI) | committed in `0ffa136` | Done except one named gateway test `node_subscription_keeps_tauri_channel_inside_gateway` (missing → fold into I). | lint, typecheck pass; 25 files / 145 UI tests pass; fmt clean. |
| **WP6** launch lifecycle | main working copy | **Done.** `NodeLifecycle` port, `custom_nodes/runtime.rs`, Start/Restart/Stop/update ordering, `node_recovery_required` / `node_rescan_failed`, startup reconcile+scan. 7 named tests at `crates/application/src/comfy_runtime/tests.rs:256-399`. | fmt, clippy `-D warnings`, `cargo test --workspace` (457 passed), arch check: all pass. |
| **WP1** pinned containment + gap fix | main working copy | **Done on Windows.** `PinnedDirectory::{open_entry, remove_owned_tree, open_read_pin}`, pinned `recycle_entry`, `custom_nodes/paths.rs` (`NodePaths`, `PinnedLocation`), `Recycler` type, registry download into an opened create-new pinned file reused for extraction, `NodeLocationGuard` + `NodeFiles::pin_location`, `NodeHook::run(.., &dyn NodeLocationGuard, ..)`, `InstallSpec.guarded` (`GuardedRequirements`), `CommandSpec.{stdin, retained}` + `retain_until_empty` after `CleanupFailed`. **Deviation (accepted by main, pending thinker review):** the guard hands out pinned *bytes*; uv gets `-r -` on stdin with cwd = guarded node dir; `install.py` runs as `python -s -c BOOT` with the script bytes on stdin (emulates argv/`__file__`/`sys.path[0]`). Behaviour change: local-path requirements resolve against the node directory. Limits: script ≤16 MiB, requirements ≤256 KiB UTF-8. Windows pins can make a hook that rewrites its own `requirements.txt`/`install.py` fail with a sharing violation. | fmt, clippy, `cargo test --workspace` (application 120, infrastructure 291+8, desktop 9/8/1/2), arch check: pass. All 14 named tests ran on Windows (junctions; file symlinks skipped for lack of privilege; requirements test used real uv 0.13.0 `--dry-run`; hook test used real Python 3.12). Linux branches **never compiled**. |
| **B** bandwidth limiter + HF relay | **merged into main** | **Done.** `models/acquisition/{bandwidth,hf_proxy}.rs`, shared `Arc<BandwidthLimiter>` in `DownloadStack`, relay on every HF attempt, upstream proxy chaining, `DownloadErrorCode::BandwidthProxyUnavailable` (`bandwidth_proxy_unavailable`), settings field enabled (`settings-form.tsx` + test). Direct deps `tokio-rustls`, `rustls-native-certs`, `base64` (already in lock). | Worktree: all checks pass (infra 278+8, UI 146). After merge into main: clippy, infrastructure (280+8) and desktop tests pass. |
| **WP2+WP3** Git content identity / staged update / refusal status; protected-package refusal + registry warnings | main working copy | **Partial, unverified, may not compile.** Sonnet coder stopped while editing tests. Delta vs the verified state: `docs/handoff/patches/main-05-wp23-partial-unverified.patch` (7 files: application `custom_nodes/{model,tests,workflow}.rs`, desktop `ipc/node_dto.rs`, infrastructure `custom_nodes/{files,git,tests}.rs`). | none |
| **WP4** final digest + identity-based recovery | — | Not started. Prompt `session2/wp4.md`. | — |
| **WP5** `NodeRequirementSource`, fatal node deps in replacement, `repair_check`, dependency projection, `NodeAdapters` split | — | Not started. Prompt `session2/wp5.md` (includes the guard-lifetime amendment). Prerequisite for E2/E3. | — |
| **E1** hashed accelerators, selections, component outcomes, pin safety | **patch** `docs/handoff/patches/e1-accelerators-verified.patch` (based on `0ffa136`; 4 new files incl. migration `007_environment_components.sql`) | **Done, not merged.** Merge into main **after WP5** (3-way; conflicts expected in `application/src/environment/{ports,deps,execute,setup}.rs`, `tests/{fakes,setup}.rs`, desktop `state.rs`/`tests/support/mod.rs`, `ipc/error.rs`). Note: `SetupActionDto` has no accelerators field yet (desktop-originated setups select none) → I adds it. | Worktree: fmt, clippy, `cargo test --workspace` (application 118, infra 275+8, desktop 7/8/1/2), arch check pass. |
| **P** signed updater + diagnostics | worktree `C:\Users\vjump\vjc-wt\bandwidth` (uncommitted, detached at `0ffa136`) + **patch** `docs/handoff/patches/p-updater-diagnostics-verified.patch` (21 new files incl. 5 permission TOMLs) | **Done, not merged.** Commands `app_update_status`, `app_update_check`, `app_update_install(updateId, onEvent)`, `app_diagnostics`, `app_copy_diagnostics`; plugins `tauri-plugin-updater =2.14.0`, `tauri-plugin-clipboard-manager =2.5.0` (+42 lock packages, none changed); updater registered only when both env vars are set (https endpoint); no plugin permission granted to any webview; UI `updates-panel`, `diagnostics-panel`, comfy-page copy fallback. **Known limitations:** model-library scan loop is not quiesced before install; the log guard is not flushed before `install()` (last buffered lines may be lost on Windows); after a failed post-quiesce install, downloads stay stopped until restart; `requireSignedVersion` left at plugin default; accelerator outcomes section shows an availability warning until E1 is merged. Merge any time between main packages (3-way; expect conflicts in desktop `state.rs`, `lib.rs`, `ipc/bindings.rs`, `build.rs`, `capabilities/manager.json`, `tests/{acl,support}`, gateway files); **regenerate bindings after merging**. | Worktree: fmt, clippy, application 118, infra 271, desktop lib 18/acl 10/bindings 1/navigation 2, UI 157 tests, typecheck, lint, arch check: pass. |
| **E2** curated nodes (setup candidate) | — | Not started. Prompt `session2/e2.md`. After WP5 + E1 merge. | — |
| **E3** relocation / guided rebuild | — | Not started. Prompt `session2/e3.md`. After E2. | — |
| **I** UI + remaining composition | — | Not started (prompt not yet written). Scope: m6 plan §I UI items (accelerator selection reset on profile change, curated selection/batch review via public custom-nodes exports, damaged optional step not skippable, missing-root Locate/New, rebuild summary, stored accelerator results on Environment, remove obsolete "M6/deferred" and "Updating ComfyUI is a later milestone" copy), `SetupActionDto.accelerators`, the missing gateway test, "Tree identity" label check, all required UI tests listed in m6 plan §I. | — |
| **R** release audit + docs | — | Not started. `docs/RELEASE_ACCEPTANCE.md` (every P0/P1 + 17 scenarios → evidence or open blocker), IMPLEMENTATION_STATUS/REQUIREMENTS §9 refresh, resolve every `<!-- VERIFY -->` marker in README.md and docs/DOCUMENTATION.md against verified behaviour, packaging audit (`pnpm -C ui tauri build`). | — |
| Thinker security review of WP1/WP6 | — | Started, **stopped by the user before reporting**. Re-run (read-only) on `main-04-verified-wp6-wp1-b-docs.patch` before closing M5. | — |

Worktrees: `C:\Users\vjump\vjc-wt\bandwidth` (holds P, warm MSVC `target/`; reuse for further isolated packages after `git reset -q && git checkout -- . && git clean` of P's files, P is saved as a patch); `C:\Users\vjump\vjc-wt\runtime` is **obsolete** (WP6 merged) — remove with `git worktree remove --force C:\Users\vjump\vjc-wt\runtime`.

### 5.4 Resume here (recommended order)
1. `git status`; preserve all uncommitted work. Decide WP2+WP3 start point:
   - **Continue** the partial edits (give the coder `session2/wp23.md` + `common.md` and say the partial delta is `docs/handoff/patches/main-05-wp23-partial-unverified.patch`), or
   - **Restart clean**: `git checkout HEAD -- <the 7 files>` then `git apply --3way --include=<file> docs/handoff/patches/main-04-verified-wp6-wp1-b-docs.patch` for each of them, which restores the verified WP6+WP1+B state.
2. Finish/verify **WP2+WP3** → **WP4** → **WP5** sequentially in main (full workspace checks after each; save a checkpoint patch after each).
3. Merge **P** (any time between packages) and **E1** (after WP5) by `git apply --3way` (for files already modified in main use `--exclude` + a plain context apply, as `--3way` refuses files that differ from the index); regenerate bindings; run the full check list.
4. **E2 → E3** (prompts ready), then write and run **I**, then **R** (acceptance ledger, IMPLEMENTATION_STATUS, VERIFY-marker pass on README/DOCUMENTATION, packaging).
5. Re-run the thinker security review on the final M5 backend diff.
6. Before claiming completion, run §5.6 on the merged tree and record which acceptance scenarios remain unverifiable on this machine.

### 5.5 Verification history
| Check | Result |
|---|---|
| HEAD `0ffa136` UI: lint, typecheck, test; `cargo fmt --check` | pass; 25 files / 145 tests; fmt clean |
| Main + WP6 (MSVC): fmt, clippy, `cargo test --workspace --locked -j 3`, arch check | pass (457 tests, 7 ignored) |
| Main + WP6 + WP1 + gap fix + B: fmt, clippy, `cargo test --workspace`, arch check | pass (see §5.3) |
| Worktree E1 / P | pass (see §5.3) |
| Earlier tree (2026-10-17 morning): `pnpm -C ui tauri build` | unsigned NSIS + MSI built; installers not run |
| Everything else | not verified |

### 5.6 Full check list (run on the merged tree)
```bash
pnpm -C ui install --frozen-lockfile && pnpm -C ui lint && pnpm -C ui typecheck && pnpm -C ui test && pnpm -C ui build
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -j 4 -- -D warnings
cargo test --workspace --locked -j 4 --no-fail-fast
node --test scripts/check-arch-deps.test.mjs && node scripts/check-arch-deps.mjs
cargo test -p desktop --test bindings --locked
cargo build -p desktop --locked --features tauri/custom-protocol
pnpm -C ui tauri build   # Windows NSIS + MSI bundles (MSVC only; release build, ~12 min)
```
Also: coverage (domain ≥90 %, application ≥80 %, `cargo llvm-cov`), and the acceptance scenarios in REQUIREMENTS §8 / plan section R (`docs/RELEASE_ACCEPTANCE.md` must map every P0/P1 item and all 17 scenarios to evidence or an explicit open blocker).

## 6. Working agreement and tooling
- Follow `AGENTS.md` and the user's global CLAUDE.md (main orchestrates; explorer → plan → advisor → coder → verify; ask the user only for advisor-classified user-only decisions).
- Codex roles: `codex exec` directly (§5.1). Fallbacks: Haiku 5.5 (explore), Sonnet 5.5 xhigh (code), Opus thinker for review.
- One writer per file set; isolated Rust packages go to the worktree and are merged by patch (no commits). Save a checkpoint patch (`git add -N <new files> && git diff --binary HEAD`) after every verified package so an interrupted coder can be continued from an isolated delta.
- Commit only when the user asks.
