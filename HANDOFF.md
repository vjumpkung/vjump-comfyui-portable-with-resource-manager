# HANDOFF — vjumpkung-rust-comfyui-desktop

Last updated: 2026-10-17 02:10 (+07:00) — **local builds now use MSVC; GNU is fallback only** (§5.1). **Implementation paused by the user mid-M5.** All subagents and Codex jobs were stopped on request; no build, test or agent process is running. Branch `feat/m0-skeleton`, HEAD `8310f12` (`chore: update HANDOFF.md`). Nothing from this session is committed. The main working copy holds **unverified partial M5 work**. Two extra git worktrees hold partial packages (see §5.4). README.md is still absent.

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
4. **`docs/handoff/`** (new this session, untracked): the approved implementation plans, evidence, coder prompts, worktree patches and local build tooling referenced below. These replace the lost temp-folder plans of earlier sessions.

Reference material:

| What | Where now |
|---|---|
| Reference portable installer (setup steps, 22 curated nodes, accelerator wheels) | **Not on this machine** at the old `D:\vjump\...` path. Fetched read-only from the owner's private GitHub repo `vjumpkung/pixellatent-happy-meals-scripts`, file `comfyui-python-3-12-happy-meals/portable_version/install_comfyui_3_12_uv_nvidia_torch_2100_portable.bat` (`gh api repos/.../contents/<path> -H "Accept: application/vnd.github.raw"`). Everything M6 needs from it is extracted into `docs/handoff/plans/m6-reference-data.md`. |
| Python backend / Next.js reference projects | `C:\Users\vjump\Documents\vjumpkung-sd-ui-manager-backend` exists; the frontend reference was not located. Not needed for the remaining work. |

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

### 3.1 Decisions made this session (advisor, AI-decidable — accepted; details in `docs/handoff/plans/*approved-plan.md`)
- **Custom-node containment:** real pinned-parent directory operations for all node mutations (NFR-SEC-5 check/use races), incl. pinned recycle with no permanent-delete fallback.
- **Git node identity = content tree digest** (not `git:<commit>`); Git updates stage a full checkout of the frozen target; post-apply validation before deps/hooks; `GitRefused { status }` keeps bounded `git status`.
- **Node operations can never authorize protected-package (torch/triton) changes**; constrained preview only; FR-ENV-3 keeps its own confirmation flow.
- **Final tree digest** must equal the reviewed digest unless a consented hook ran (then refresh); ambiguous recovery in any non-terminal phase **blocks launch** (`NodeOperation::blocks_start`).
- **Repair/replacement:** node requirement sets (root `requirements.txt` + persisted registry deps of the latest consented op, enabled *and* disabled nodes) come from a new `NodeRequirementSource` port implemented in application `custom_nodes/requirements.rs`; any node requirement failure is **fatal in every replacement mode**; `RepairRequired` always rebuilds; dependency state is per `(node, env, dependency_identity)` from `node_env_deps`, missing = `not_run`. No migration 007 for this.
- **Launch lifecycle:** `NodeLifecycle` runtime port; Start = lease → node check → spawn; Restart = one lease: stop → reconcile/rescan → check → start; Stop = ungated stop, then waiting acquire, re-verify, awaited rescan; errors `node_recovery_required`, `node_rescan_failed`.
- **Frontend contract additions:** raw registry metadata in `RegistryNodeDto`; read-only `node_git_status`; `NodeListDto.pendingReviews/recent`; `OperationDto.nodeId/blocksStart`; Restart offered via `comfyRestart`.
- **M6:** curated nodes run **before activation against an explicitly bound setup candidate** (`NodeTarget::Setup(RunId)`, `SetupNodes` port injected into `ExecuteSetup`), with exact M5 review (main's call: present the selected curated plans as **one batch** in the multi-plan review dialog), never running `install.py`; accelerators = hashed wheels + package component, py312 wheels shipped **experimental and unchecked**, py313/314 lists stay empty; relocation detected before any startup reconciliation, separate `setup_open_install` / `setup_new_install`, rebuild into a new app-owned env with the exact recorded profile revision; HF relay used for **every** HF attempt so live cap changes apply; updater env names **`VJUMP_UPDATER_PUBKEY` / `VJUMP_UPDATER_ENDPOINT`**, binds install to the reviewed offer, **no check-on-launch setting**; diagnostics redact the whole report and use the clipboard plugin from Rust with a textarea fallback.

## 4. Open items (spec §9 — need runtime validation)
- Accelerator wheel compatibility (hashes ≠ ABI/CUDA proof); FA3 import module name unverified.
- Native HF engine vs Xet files; whether newer `hf` restores cross-run resume.
- WebKitGTK performance and Thai fonts on Linux; Windows code signing and updater key custody.
- **This machine cannot produce NVIDIA or Linux evidence** (see §5.1). All CUDA/setup/accelerator/Linux acceptance remains open; Windows packaging is only partly covered (unsigned NSIS/MSI bundles build, installers not run — §5.5).

## 5. Implementation status and next steps

### 5.1 Environment facts discovered this session (important)
- Machine: Windows 11 x64, **AMD Radeon GPU only (no NVIDIA)**, 12 cores, 15 GB RAM (often only 3–5 GB free → run at most two Rust builds at once, use `-j 4`).
- **Build toolchain = MSVC (default).** The VS 2022 Community instance (17.14) now has the C++ toolset (MSVC v143 **14.44.35207**, component `Microsoft.VisualStudio.Component.VC.Tools.x86.x64`) and the **Windows 11 SDK 10.0.22621** (`Microsoft.VisualStudio.Component.Windows11SDK.22621`), added 2026-10-17. Cargo uses the toolchain pinned by `rust-toolchain.toml` (`1.99.0-x86_64-pc-windows-msvc`) and the default `target/` dir: no env script, no Developer Prompt; works from Git Bash and PowerShell. Desktop test exes get the Common Controls v6 manifest from `crates/desktop/build.rs`. Results: §5.5.
  - If the components go missing, linking fails with `LNK1181 kernel32.lib` (rustc falls back to the old VS 14.0 `link.exe`). An `install --add …` bootstrapper run on the existing instance only *updates* VS and silently skips the `--add` components (happened 2026-10-16). Add them with an elevated `"C:\Program Files (x86)\Microsoft Visual Studio\Installer\setup.exe" modify --installPath "C:\Program Files\Microsoft Visual Studio\2022\Community" --add Microsoft.VisualStudio.Component.VC.Tools.x86.x64 --add Microsoft.VisualStudio.Component.Windows11SDK.22621 --passive --norestart`.
- **GNU toolchain = fallback only** (use when MSVC is unavailable or broken): `1.99.0-x86_64-pc-windows-gnu` (installed) plus MSYS2 UCRT64 gcc/windres. In Git Bash, `source docs/handoff/tooling/gnu-env.sh` before cargo (sets `RUSTUP_TOOLCHAIN`, `CARGO_TARGET_DIR=target-gnu`, a test runner that places the 64-bit `WebView2Loader.dll` next to test exes, a Common Controls v6 manifest object linked into every artifact, and line-tables-only debug info). Without this, GNU desktop test executables fail with `0xc0000139`/`0xc000007b`. The exports last for the whole shell, so use a separate shell for GNU. `/target-gnu/` was added to `.gitignore` (uncommitted).
- **Codex:** the plugin companion route (`/codex:rescue`, `codex-companion.mjs task`) forces a read-only/workspace-write sandbox whose shell commands are all rejected on this machine. Working route: `codex exec --skip-git-repo-check -m <model> -c model_reasoning_effort=xhigh -o <out> - < <prompt>` (helper: `docs/handoff/tooling/run-codex.sh`, set `CODEX_SCRATCH`); read-only roles must be enforced in the prompt. The account hit its **Codex usage limit at ~00:43 on 2026-10-17** (reset stated as 04:29); Claude Sonnet 5.5 fallback coders were then used.
- Generated permission TOMLs under `crates/desktop/permissions/autogenerated/` show as modified after builds; it is line-ending noise only (`git diff` is empty).

### 5.2 Milestone status
| Milestone | Status |
|---|---|
| M0–M4 | Implemented in earlier sessions. Baseline this session on committed HEAD: UI lint/typecheck pass, **73 UI tests pass**; Rust `application` **106 tests pass** on GNU. A full clean workspace run on HEAD was not completed (desktop exe loader issue, since solved by the tooling). |
| M5 Custom nodes | **In progress.** Contract (C0) and UI largely written but unverified; WP6 partially done in a worktree; backend WP1–WP5 not started. Details §5.3. |
| M6 Release 1 | Approved plan exists (`docs/handoff/plans/m6-approved-plan.md`). Bandwidth (B) partially written in a worktree; everything else not started. |
| README | Absent. Write last, from verified behavior (plan section R). |

### 5.3 Work packages — exact state
Approved specs: M5 backend = `docs/handoff/plans/m5-backend-approved-plan.md` (WP1…WP7); M5 frontend = `docs/handoff/plans/m5-frontend-approved-plan.md` (WP1 = contract, WP2…WP6 = UI); M6 = `docs/handoff/plans/m6-approved-plan.md` (E1, E2, E3, B, P, I, R) + data `m6-reference-data.md`. Ready-to-use coder prompts: `docs/handoff/prompts/*.md` (they reference this session's temp scratchpad path — substitute `docs/handoff/plans|evidence|tooling/`; their GNU build-env instructions, incl. the "no MSVC" line in `code-p.md`, are superseded: build with MSVC per `tooling/coder-env.md`, GNU `gnu-env.sh` only as fallback).

| Package | Location | State | What remains |
|---|---|---|---|
| **C0** — M5 IPC contract (FE plan WP1 + BE WP7): opener empty-authority fix, raw registry metadata, `node_git_status` + `NodeAppError::GitUnavailable`, `pending_reviews`/`recent_operations`, `OperationDto.nodeId/blocksStart`, `NodeOperation::blocks_start()`, bindings regenerated, gateway `nodeOpenRepository`/`nodeGitStatus` | **main working copy** (26 Rust/UI files modified, bindings.ts regenerated) | Codex implemented most, Sonnet fallback was verifying. Last report: `cargo test -p infrastructure custom_nodes` **42 passed incl. real-git `git_status` tests**; desktop lib/ACL/bindings tests, clippy, fmt check and UI checks **not yet confirmed** (the 2026-10-17 MSVC run on the whole working copy passed desktop lib/ACL/bindings, clippy and `pnpm -C ui build` — §5.5; fmt check and UI lint/test still open). | Run the full C0 success criteria in `prompts/code-c0.md`; fix failures. Review the diff — it also touched `domain/custom_nodes/{compatibility,consent}.rs`, `infrastructure/custom_nodes/{compat,provenance,scan,stack,tree}.rs` and `workflow.rs` (partly rustfmt of previously unformatted files; confirm no unintended behavior change). |
| **FE** — M5 UI WP2–WP6 (Custom Nodes page, registry browser, install/review/update/update-all/confirm dialogs, installed table with virtualization, operations panel, hooks, app-shell route, comfy-runtime public `useRuntime`/`useRuntimeActions`) | **main working copy**, `ui/` only (~25 new files under `ui/src/features/custom-nodes/`, `app-shell.tsx`, `app-shell.test.tsx`, `comfy-runtime/index.ts`) | Sonnet coder stopped mid-way while writing tests. **No lint/test run yet**; `pnpm -C ui build` (incl. `tsc -b`) passed on 2026-10-17 (§5.5). | Finish missing tests named in the frontend plan §5, then `pnpm -C ui lint && pnpm -C ui typecheck && pnpm -C ui test && pnpm -C ui build`. Prompt: `prompts/code-fe.md`. |
| **WP6** — launch lifecycle (`NodeLifecycle` port, `custom_nodes/runtime.rs`, Start/Restart/Stop ordering, `node_recovery_required`/`node_rescan_failed`) | worktree `C:\Users\vjump\vjc-wt\runtime` (detached at HEAD) + saved **`docs/handoff/patches/wt-runtime-wp6.patch`** | Application tests reported passing with all 7 spec tests present; infrastructure/desktop/ACL/clippy **not verified**. | Apply into main **after C0 passes**: take only the WP6 hunks (application `comfy_runtime/*`, `custom_nodes/{runtime,mod,use_cases,tests}.rs`, desktop `app_lifecycle.rs`, `ipc/{comfy_commands,error}.rs`, `state.rs`); the patch's `domain/` and `infrastructure/custom_nodes/*` hunks are mostly rustfmt reflow of HEAD — discard them in favour of main's versions. Then run the WP6 success criteria (`prompts/code-wp6.md`). Remove the worktree afterwards (`git worktree remove`). |
| **WP1** pinned filesystem containment | main | Not started | Prompt `prompts/code-wp1.md`. |
| **WP2+WP3** Git content identity/full staged update/status on refusal; protected-package refusal + registry warning merge | main | Not started | Specs in backend plan. |
| **WP4** final digest + identity-based recovery + `blocks_start` extension | main | Not started | Spec in backend plan. |
| **WP5** `NodeRequirementSource`, fatal node deps in replacement, `repair_check`, per-env dependency projection, `NodeAdapters` split | main | Not started | Spec in backend plan. Prerequisite for M6 E2/E3. |
| **M6 B** bandwidth limiter + HF CONNECT relay + live settings + Settings field + `DownloadErrorCode` variant `bandwidth_proxy_unavailable` | worktree `C:\Users\vjump\vjc-wt\bandwidth` + saved **`docs/handoff/patches/wt-bandwidth-b.patch`** | Codex wrote `bandwidth.rs`, `hf_proxy.rs` (+ tests dirs), engine/stack/codec/domain/settings-form changes, then hit the usage limit. **Never compiled or tested.** | Finish + verify per `prompts/code-b.md` (includes the domain-enum amendment), then patch-merge into main. |
| **M6 P** updater + diagnostics | — | Not started | Prompt ready: `prompts/code-p.md` (planned worktree `C:\Users\vjump\vjc-wt\platform`). |
| **M6 E1 → E2 → E3** accelerators → curated nodes (setup candidate) → relocation/rebuild | main | Not started | Sequential, after WP5 (they share `environment/{model,ports,setup,execute,queries}.rs`). Data in `m6-reference-data.md` (22 curated commits resolved 2026-10-16T17:11Z; py312 wheel URLs + publisher SHA-256). |
| **M6 I** integration (composition, 8 new commands, ACL, bindings, UI) and **R** release audit, `docs/RELEASE_ACCEPTANCE.md`, docs refresh, README | main | Not started | Per M6 plan. Windows bundles now build locally on MSVC (`pnpm -C ui tauri build`, unsigned, no updater config — §5.5); Linux bundles need a Linux host. |

### 5.4 Resume here (recommended order)
1. `git status`; preserve all uncommitted work. Cargo runs directly on MSVC (no env setup); only for the GNU fallback, `source docs/handoff/tooling/gnu-env.sh` in Git Bash first.
2. Finish/verify **C0** in main (full criteria incl. `cargo fmt --all -- --check`, clippy `-D warnings`, `cargo test -p desktop --lib/--test acl/--test bindings`, UI checks). Bindings regeneration only via `cargo test -p desktop --test bindings --locked -- --ignored export_bindings`.
3. Finish/verify **FE** (ui only) — can run in parallel with step 4.
4. Merge **WP6** from the runtime worktree/patch (WP6 hunks only), verify.
5. Backend **WP1 → WP2+WP3 → WP4 → WP5** sequentially in main, verifying each.
6. In parallel worktrees (≤1 extra Rust build at a time): finish **B**, then **P**; patch-merge each into main and re-verify.
7. **E1 → E2 → E3**, then **I** (register commands, ACL, bindings, UI), then **R** (packaging audit, acceptance ledger, IMPLEMENTATION_STATUS/REQUIREMENTS §9 refresh, README).
8. Before claiming completion, run the full check list (§5.6) on the merged tree and record which acceptance scenarios remain unverifiable on this machine.

### 5.5 Verification actually observed this session
| Check | Result |
|---|---|
| Committed HEAD UI: `pnpm -C ui install --frozen-lockfile`, lint, typecheck, test | pass; 17 files / **73 tests** |
| Committed HEAD Rust (GNU): `cargo test -p application` | **106 passed** |
| `cargo fmt --all -- --check` on HEAD | **fails** on pre-existing files (`application/custom_nodes/{tests,use_cases}.rs`, `desktop/src/state.rs`) |
| C0 (in progress) | `cargo test -p infrastructure custom_nodes`: 42 passed (Sonnet fallback report) |
| WP6 (worktree, in progress) | `cargo test -p application`: pass incl. 7 new WP6 tests (Sonnet fallback report) |
| **MSVC**, 2026-10-17, main working copy (incl. uncommitted partial M5): `cargo build --workspace --all-targets --locked -j 4` | pass (7 m 44 s cold) |
| same: `cargo test --workspace --locked -j 4 --no-fail-fast` | pass — domain 44, application 109, infrastructure 267 (+5 ignored), infrastructure `workflow` 8, `real_setup` 0 (+1 ignored), desktop lib 7, `acl` 8, `bindings` 1 (+1 ignored export), `navigation` 2; no loader failures |
| same: `cargo clippy --workspace --all-targets --locked -j 4 -- -D warnings` | pass |
| same: `pnpm -C ui build`, then `cargo build -p desktop --locked --features tauri/custom-protocol` | pass (without `ui/dist` the desktop build fails: "`frontendDist` … `../../ui/dist` … doesn't exist") |
| same: `pnpm -C ui tauri build` (release) | pass (~12.5 min); unsigned `target/release/bundle/nsis/vjumpkung-comfyui-desktop_0.1.0_x64-setup.exe` (7.9 MiB) and `target/release/bundle/msi/vjumpkung-comfyui-desktop_0.1.0_x64_en-US.msi` (11.2 MiB); tauri-cli downloaded NSIS 3.13 + WiX 3.14 on first run. Installers **not run**. |
| Everything else in this session's diff | **not verified** |

### 5.6 Full check list (run on the merged tree)
```bash
# MSVC (default): no env setup. GNU fallback only: source docs/handoff/tooling/gnu-env.sh
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
- Codex roles: call `codex exec` directly (see §5.1); the companion route's sandbox is broken here. Fallbacks: Haiku 5.5 (explore), Sonnet 5.5 (code), Opus thinker for review.
- Run coders for main-repo Rust and `ui/` concurrently only when their file sets are disjoint; use detached worktrees under `C:\Users\vjump\vjc-wt\` for independent Rust packages and merge by patch (no commits).
- Commit only when the user asks.
