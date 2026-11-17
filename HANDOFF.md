# HANDOFF — vjumpkung-rust-comfyui-desktop

Last updated: **2026-11-17 (+07:00)**. The session history (sessions 3–13, 2026-10-19 to 2026-11-01) was removed; read it with `git show 881aa90:HANDOFF.md`. Files removed after `8dc99b1` (prompts, patches, most plans and evidence): `git show 8dc99b1:<path>`.

## 0. Current state

- `main` @ `881aa90`, version 0.1.5; 96 IPC commands, migrations 001–011, 9 bundled environment profiles (+2 archived), 14 `#[ignore]` tests.
- v0.1.5 was published 2026-11-02 from `48f3f60`; published releases: v0.1.2, v0.1.3, v0.1.5. Commits after the last session record (session 13): `5503168`, `48f3f60`, `3dce71d`, `5905813`, `881aa90`. `3dce71d` and later are not in a release; they have automated evidence only (OPEN-NATIVE-001 in the ledger).
- Hosted CI is green on `881aa90` (run 43076757035: Windows 1,111 / 0 / 14, Ubuntu 1,086 / 0 / 11, Vitest 44 files / 321 tests, coverage domain 95.73 %, application 85.83 %).
- Release verdict **NO-GO**: gates, residuals and evidence are in `docs/RELEASE_ACCEPTANCE.md`; milestones are in `docs/IMPLEMENTATION_STATUS.md`.
- The tutorial recording profile scripts (`scripts/swap-to-tutorial-profile.cmd`, `scripts/restore-default-profile.cmd`; `881aa90`) are an app-state swap, not an environment profile (README "Tutorial recording profile").
- **Never touch the owner's working install**: never read, list, launch, modify or delete it.
  - Native tests use debug builds with `VJUMPKUNG_COMFYUI_HOME` set to a disposable root; the override is honoured only in debug builds (`crates/infrastructure/src/shared/paths.rs`).
  - On the owner's machine, do not run a release build, a debug build without the override, `scripts/dev-reset.cmd` (it offers the install root recorded in `bootstrap.json` for deletion) or the profile-swap scripts without asking: all of them use the owner's app data, which may point at that install.
  - The single-instance identifier is shared with production, so a launch while the owner's app runs is handed to it.

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
4. `AGENTS.md` (check list "Before finishing a feature").
5. `docs/RELEASE_ACCEPTANCE.md` and `docs/IMPLEMENTATION_STATUS.md`.
6. `docs/handoff/`: `plans/m6-reference-data.md`, `evidence/` (rulings, native registers, residuals, Linux/packaging evidence cited by the docs), `tooling/` (GNU fallback, Linux container check, Codex runner). Older material: `git show 8dc99b1:<path>`.
7. `docs/handoff/evidence/s3-residuals.md` (source of R01–R21).

Reference material:

| What | Where now |
|---|---|
| Reference portable installer (setup steps, 22 curated nodes, accelerator wheels) | Owner's private GitHub repo `vjumpkung/pixellatent-happy-meals-scripts`, file `comfyui-python-3-12-happy-meals/portable_version/install_comfyui_3_12_uv_nvidia_torch_2100_portable.bat` (`gh api repos/.../contents/<path> -H "Accept: application/vnd.github.raw"`). Everything M6 needs is extracted into `docs/handoff/plans/m6-reference-data.md`. |

## 3. Decisions already made — do not re-litigate
| Decision | Who | Where |
|---|---|---|
| Release 1 = **Windows x64 + Linux x64, NVIDIA only** | User | §1.4 |
| Moved install → **guided rebuild** of the venv (keep user data) | User | FR-SETUP-12 |
| **Tauri 2.12.x**; **Vite + React + TS** frontend; **tauri-specta 2.0.0-rc.25** pinned | Advisor/user | §2.1–2.2, ADR-0001 |
| **Clean Architecture, Rust adaptation**; crates `domain` ← `application` ← `infrastructure` ← `desktop` + `ui/`; contexts as modules; no serde in domain/application; `async-trait` dyn ports; errors per layer | User/advisor | §3.3, ADR-0002 |
| One **maintenance gate**, no event bus; model rename/delete allowed while ComfyUI runs | Advisor | §3.3.3–3.3.4 |
| Failed in-place pip / `install.py` → env **`repair_required`**, no venv snapshot | User | §6.2, FR-NODE-4 |
| Linux default Python **3.13** + torch **2.14.1 cu130**; **Windows default is now Python 3.12 + torch 2.10 cu130 (`win-py312-torch2100-cu130`, owner 2026-10-20; the py313/torch 2.14.1 and cu126 profiles stay available)**; uv-managed Python; MinGit on Windows, system git on Linux | Advisor; owner (Windows default) | §2.3–2.4, §5.4 |
| First-run setup installs Manager + curated nodes without consent pauses (the review step discloses the third-party code and **Start setup** is the confirmation; Repair and the Custom Nodes page keep their prompts); curated = latest default-branch commit, no commit lock (owner, 2026-10-20) | Owner | NFR-SEC-3, FR-SETUP-9 |
| Tokens only in the OS keyring; delete → Recycle Bin/trash; ComfyUI-Manager optional; ticked by default in first-run setup; curated nodes unticked by default (owner, 2026-10-20) | Advisor | FR-SET-1, FR-MODEL-3, FR-NODE-10 |
| **HF downloads default to the `hf` CLI**; pause/resume restarts from 0 for that engine | User | FR-DL-3/3a/3b |
| Design system = vjump.dev adapted; UI copy **English + Thai** (single `translations.json`, default English, chosen on wizard stage 0 or in Settings; supersedes "English only", owner 2026-10-22) | User | DESIGN.md §1.6, NFR-UX-2 |
| **New global frontend state uses zustand** (pinned exact); existing `useSyncExternalStore` stores stay as they are | User | DOCUMENTATION conventions |
| CI workflows **re-enabled** by the owner (2026-10-21, `9a12c05`); hosted CI green on Windows and Ubuntu; `release.yml` builds draft bundles when the version in `crates/desktop/tauri.conf.json` changes on `main`, and the owner publishes them; still run the checks locally | User | `.github/workflows/*`, AGENTS.md |
| Updater key custody, Authenticode, project license, final icons = **owner decisions**; updater stays inert without build-time config; no license added | Owner | IMPLEMENTATION_STATUS |
| **Commit only when the user asks** | User | working agreement |
| CPU testing profiles are experimental, for testing without an NVIDIA GPU, never recommended or preselected, and launch ComfyUI with `--cpu` | Owner (2026-10-23) | FR-SETUP-2, §1.4, §5.4 |
| App Git commands check out LF (`-c core.autocrlf=input -c core.eol=lf`) and the portable MinGit config gets a matching block; line-ending-only differences are not local changes | Owner (2026-11-01) | §6.2 |
| "Dangerously skip security confirmations" is off by default and only pre-ticks UI confirmations; protected-package changes, source builds and remembered trust are unchanged | User | FR-SET-3, NFR-SEC-3 |

### 3.1 Advisor decisions (AI-decidable — accepted; file names below are under `docs/handoff/` and were removed after `8dc99b1`: `git show 8dc99b1:docs/handoff/<path>`)
- **Custom-node containment:** real pinned-parent directory operations for all node mutations (NFR-SEC-5 check/use races), incl. pinned recycle with no permanent-delete fallback.
- **WP1 gap decision (`plans/m5-wp1-gap-decision.md`):** registry archive downloads into a create-new pinned file reused for extraction; a typed **`NodeLocationGuard`** keeps pins and pinned inputs alive through preview, dependency install, hook execution and cleanup (incl. `CleanupFailed`).
- **Git node identity = content tree digest** (not `git:<commit>`); Git updates stage a full checkout of the frozen target; post-apply validation before deps/hooks; `GitRefused { status }` keeps bounded `git status`. **Implemented (WP2).**
- **Node operations can never authorize protected-package (torch/triton) changes**; constrained preview only; FR-ENV-3 keeps its own confirmation flow. **Implemented (WP3).**
- **Registry review warnings (new, 2026-10-19):** compatibility (OS/accelerator/ComfyUI) comes **exclusively from the exact release** being installed (missing dimensions stay "unknown"); flagged/deprecated = exact release OR the **top-level node's** own flags; flags present only on an unrelated `latest_version` never propagate; one `warnings()` call, naturally deduplicated. `RegistryNode` gained `node_flagged`/`node_deprecated`; registry cache schema bumped to 2. **Implemented.**
- **Final tree digest** must equal the reviewed digest unless a consented hook ran (then refresh); ambiguous recovery in any non-terminal phase **blocks launch** (`NodeOperation::blocks_start`). **Implemented (WP4).**
- **Repair/replacement:** node requirement sets from a `NodeRequirementSource` port in application `custom_nodes/requirements.rs`; any node requirement failure is **fatal in every replacement mode**; `RepairRequired` always rebuilds; dependency state per `(node, env, dependency_identity)`; missing = `not_run`; no migration. **Amended (H plan §7):** `NodeRequirementSet.file: Option<GuardedRequirements>` carrying protected bytes + resource token; propagate the lease retention; remove `repair_requirements`. **Implemented (WP5)**; advisor gap ruling: only NotFound = no requirements file (oversized/non-UTF-8 → `UnsafeSource`, validated before the preview's early return); conflicted / empty-digest nodes project `NotRun`.
- **Session 4 decisions (accepted; details `evidence/s4-advisor-first-run-fix.md`, `evidence/s4-reviews.md`):** first-run scaffold = bounded directory-only tree treated as Absent, clone merged in place with a durable publication record (never replace pinned dirs); verbatim `\\?\` paths converted for the Windows shell recycle API (other verbatim forms refused); Skip refused while any curated entry is Failed; E3 restricted composition only for a moved root, current-root runs reconciled in restricted mode, relocation rebases from every recorded root and stores non-verbatim paths, quiesce only on a successful restart; runtime tests use an injectable process snapshot (`procscan::SnapshotFn`).
- **Launch lifecycle:** `NodeLifecycle` runtime port; Start/Restart/Stop ordering under one lease. **Implemented (WP6); thinker verified the ordering.**
- **M6:** curated nodes run before activation against an explicitly bound setup candidate (`NodeTarget::Setup(RunId)`, `SetupNodes` port injected into `ExecuteSetup`), one batch in the multi-plan review; accelerators = hashed wheels + package component, py312 wheels experimental/unchecked, py313/314 lists empty; relocation detected before any startup reconciliation; separate `setup_open_install` / `setup_new_install`; rebuild into a new app-owned env with the exact recorded profile revision; HF relay for every HF attempt; updater env names `VJUMP_UPDATER_PUBKEY` / `VJUMP_UPDATER_ENDPOINT`, install bound to the reviewed offer, no check-on-launch setting; diagnostics redact the whole report, clipboard from Rust with a textarea fallback.
- **M5 hardening (new, 2026-10-19, `plans/m5-h-hardening-approved-plan.md`):** split into **H1 → H2 → H3, all before WP4**; caps 64 path components / 1024-byte relative paths / 255-byte components / 8 MiB archive name budget; registry archives refuse `.git` and `__pycache__`; Git commands run with an app-owned empty global config + `GIT_CONFIG_NOSYSTEM=1` and a non-executing `git config --local --no-includes` preflight refusing `filter.*`, includes and worktree redirection; keep stdin + node-dir cwd for guarded requirements; BOOT verifies length + SHA-256; **retain rejection of all reparse points** (no allowance for WOF/cloud/dedup tags); **F16 relay authentication = accepted Release-1 residual**; Linux post-`renameat2` identity checks and `PR_SET_PDEATHSIG` deferred to a native Linux package (never "rename back").
- **Main's integration decisions:** coders may make the minimal desktop wiring needed to make their package testable (commands, ACL cases, bindings regeneration, compile-fix TS); package I builds the UI and remaining composition.

## 4. Open follow-ups and next steps
- Residuals R01–R43 and gates G01–G10 live in `docs/RELEASE_ACCEPTANCE.md`.
- Next steps: G01 Linux x64 NVIDIA desktop; G02 installers on a disposable VM; G03 provider tokens; G04 signing (owner); G05 Windows native scenarios incl. OPEN-NATIVE-001; G07 performance/UX audit; G08 R19/R21/R28; the tests listed in OPEN-TEST-001.
- Under `pnpm -C ui tauri dev` a self-restart kills the Vite dev server (restart `pnpm -C ui dev`).
- The owner's PostToolUse rustfmt hook orders imports differently from the project config: run `cargo fmt --all` after Rust edits.
- The vjump.dev tutorial (other repo) can drop its Disable known-issue paragraph (fixed in session 11).
- Owner follow-up (separate package): app downloads into the extra-model base folder / `is_default: true`.
- WM_CLOSE right after Stop may be refused while the post-stop rescan holds the gate: recheck after `3dce71d`.
- README "Project status" and REQUIREMENTS §9 still describe 2026-10-20.

## 5. Environment and tooling
- Machine: Windows 11 x64, NVIDIA GeForce RTX 3060 12 GiB (driver 610.88), repo on `D:\`; MSVC toolchain (VS 2022 Build Tools), Rust 1.99.0 pinned by `rust-toolchain.toml`; GNU fallback `docs/handoff/tooling/gnu-env.sh` (+ `gnu-runner.sh`, `cc6.rc`, `test.manifest`). Tools as of 2026-11-17: Node 24.22.1, pnpm 10.24.0, git 2.55.0.windows.3, cargo-llvm-cov 0.9.1 + `llvm-tools-preview`. Run one heavy cargo command at a time.
- A second, GPU-less Windows machine was used in sessions 11–13; use the CPU testing profile there.
- Checks: AGENTS.md "Before finishing a feature". Extras: `VITEST_MAX_WORKERS=2` under load; `CARGO_INCREMENTAL=0` if rustc hits an ICE; the ignored native smoke test `cargo test -p desktop --test startup_smoke --features tauri/custom-protocol fresh_root_starts_and_closes_cleanly -- --ignored --exact --nocapture --test-threads=1` (opens a window; fails if another instance runs); `pnpm -C ui tauri build` for local bundles (~12 min); re-run and sum test counts yourself.
- Native driving: start the debug exe with `VJUMPKUNG_COMFYUI_HOME` (disposable root) and `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`; drive it with `node docs/handoff/tooling/cdp.mjs text|click|type|js|shot|wait`. CDP `Emulation.setDeviceMetricsOverride` resets when the connection closes (measure in the same connection).
  - To hide `~/.gitconfig` override only `HOMEDRIVE`/`HOMEPATH` (overriding `USERPROFILE` breaks the app data folder and the DevTools port).
  - A running app locks `target\debug\*.exe`; the single-instance plugin hands a second launch to the running app; close with WM_CLOSE (force-killing leaves msedgewebview2 alive and an immediate relaunch panics with WebView2 0x800700AA).
  - UI Automation: prefer actionable elements over same-named text; the first synthetic click on an unfocused WebView2 window only focuses it; React inputs accept `ValuePattern.SetValue`. Read a live `app.log` with `FileShare.ReadWrite`.
  - Delete disposable roots and the debug WebView2 cache `%LOCALAPPDATA%\dev.vjump.comfyui-desktop` afterwards.
- Linux (Docker Desktop, WSL2): `docs/handoff/tooling/linux-check.sh`; start Docker Desktop, then `docker run --rm -v <repo>:/src:ro -v <dir with linux-check.sh>:/out -v vjc-linux-target:/target -v vjc-linux-cargo:/usr/local/cargo/registry rust:1.99.0-trixie bash /out/linux-check.sh` (the named volumes keep the build cache). `linux-bundle.sh` builds the deb + AppImage; `linux-gui/` (Dockerfile + `start.sh` + `wd.py`) runs the GUI with `--gpus all`. Git Bash rewrites `/paths` in `docker exec`: wrap commands in `bash -c '…'`.
- Agents: roles per AGENTS.md and the user's global CLAUDE.md. Optional Codex runner `docs/handoff/tooling/run-codex.sh <model> <effort> <name>` with `CODEX_SCRATCH=<dir holding <name>.prompt.md>`; `CODEX_SANDBOX=danger-full-access` for writers, `read-only` for reviewers. Inside the Codex sandbox Vitest/`check-arch-deps` hit `spawn EPERM`, there is no npm registry access and ~55 Recycle-Bin/Win32 tests fail: always re-run checks outside. Check for an orphaned `codex.exe` before relaunching; never overwrite `run-codex.sh` while a run uses it; Codex on Windows may write CRLF: normalize edited files to LF.
- Repo facts: generated permission TOMLs under `crates/desktop/permissions/autogenerated/` stay in the tree; write prompt/script files with the Write tool rather than shell heredocs, and keep backslash paths out of inline PowerShell/Python strings.

## 6. Working agreement and tooling
- Follow `AGENTS.md` and the user's global CLAUDE.md (main orchestrates; explorer → plan → advisor → coder → main verifies; ask the user only for advisor-classified user-only decisions).
- One writer per file set; packages run sequentially in the main working copy; main re-runs the checks after every coder.
- Coders must not run git write commands (add/reset/checkout/restore/stash/commit/apply/clean).
- Commit only when the user asks.
