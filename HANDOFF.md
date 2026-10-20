# HANDOFF — vjumpkung-rust-comfyui-desktop

Last updated: **2026-10-20 ~11:30 (+07:00)**. **Session 5 stopped at the owner's request after package D2 (a new session will continue).** No build, test, agent, Codex job or acceptance app of this session is running. Branch `feat/m0-skeleton`, HEAD **`a4175d1`** (owner commit `feat: temp` = all session-4 work through E3). **Nothing from session 5 is committed.** The working copy holds verified work through **P0 + I + D2**; authoritative checkpoint **`docs/handoff/patches/s5-03-through-d2-verified.patch`** (SHA-256 `046a5637…f64f`; = `git diff HEAD → working copy`, binary, excludes `docs/handoff/patches`). README.md and docs/DOCUMENTATION.md are still the session-2 drafts with `<!-- VERIFY -->` markers (D2 edited one line of DOCUMENTATION). **Read §0b first.**

## 0b. Session 5 summary (2026-10-20 09:35–11:30) — read this first

**Goal given by the owner:** read HANDOFF, debug the "segmentation fault" first, continue every phase, then generate README.md + documentation.

**Latest verification (main, ~11:25, working copy = s5-03):** `cargo fmt --check` clean; `cargo clippy --workspace --all-targets --locked -j 6 -- -D warnings` clean; `cargo test --workspace --locked -j 6 --no-fail-fast` **718 passed / 0 failed / 10 ignored**; bindings drift passes; ACL 11 pass (verified after I); `pnpm -C ui lint` + `typecheck` pass; Vitest **29 files / 184 tests**; `pnpm -C ui build` + `cargo build -p desktop --features tauri/custom-protocol` pass (after I); `node scripts/check-arch-deps.mjs` OK (+10 self-tests); native smoke test `cargo test -p desktop --test startup_smoke --features tauri/custom-protocol fresh_root_starts_and_closes_cleanly -- --ignored --exact --nocapture --test-threads=1` passes. Test-count history: 709 (HEAD) → 712 (P0) → 715 (I) → 718 (D2).

**Done and verified this session** (main re-ran the checks after every coder):

| # | Package | What | Checkpoint |
|---|---|---|---|
| 1 | **P0 startup crash = the owner's "segfault"** | Every launch of the committed app panicked (exit 101, `state() called before manage()`), because `crates/desktop/src/lib.rs:131` read managed `AppState` before `run_return` — Tauri 2.12 runs `.setup` (which calls `manage`) inside `run_return` on Ready. No test ran `desktop::run()`. Fix (advisor-approved): the close coordinator is published into an outer `OnceLock` from setup and retained to `process::exit`; `run_with_runtime<R>` + `on_run_event<R>` extracted; pre-setup guards in `restore_manager` (single-instance callback), `on_manager_close_requested`, `keep_core_alive`; log markers "application setup finished"/"application shutdown finished". Tests: `startup_tests::run_return_publishes_state_and_retains_close` (MockRuntime), 2 app_lifecycle tests, **ignored native smoke `tests/startup_smoke.rs::fresh_root_starts_and_closes_cleanly` (mandatory in the check list; opens a window; fails if another instance runs)**. Native F4 checks passed: fresh root, half-set-up copy of the owner's root, recovery mode, second instance during init/after Ready, WM_CLOSE during init/after Ready. No main-thread stack overflow observed. | `s5-01-startup-panic-fix-verified.patch` |
| 2 | **I (UI integration)** | Advisor-corrected prompt (`prompts/session5/i.md`). DTOs `ProfileDto.accelerators`, `SetupRequestDto.accelerators`, `EnvironmentInfoDto.components` (InstalledComponentsDto); gateway `setupListCuratedNodes/OpenInstall/NewInstall` + missing subscription test; wizard accelerator + curated selection (selection ≠ consent); one batch curated review reusing `ReviewDialog` via an app-injected render prop (trust memory hidden in setup); Retry resends exact decisions; Skip only via `failure.canSkip`; missing/inaccessible-root Locate/New screens; dedicated restricted-safe rebuild screen; Environment page shows stored accelerator results; all stale milestone copy removed. D8 decided: no Locate in the moved state (residual). Report `evidence/s5-package-i-implementation.md`. | `s5-02-through-i-verified.patch` |
| 3 | **D2 curated "unavailable"** | Native finding: the default first run (all 22 curated) can never succeed — `ComfyUI-PlagueKind-Nodes@d58d006` (= upstream HEAD) tracks `bundle_nodes/__pycache__/*.pyc`, refused by `validate_fresh_git`. Catalog entry now carries `unavailable` (reason shown, unselectable, never sent, backend refuses with `curated_selection_invalid`). Report `evidence/s5-d2-implementation.md`. | **`s5-03-through-d2-verified.patch`** |

**Environment facts corrected:** this machine HAS an **NVIDIA GeForce RTX 3060 12 GiB (driver 610.88, CUDA 13.3)** — native NVIDIA acceptance is possible (the "no NVIDIA" statements below are outdated). Codex hit its usage limit at ~11:00 (reset stated **14:36**); the Opus advisor stand-in / thinker and the Sonnet xhigh coder fallback were used afterwards.

**Native acceptance so far** (debug + `--features tauri/custom-protocol` binary frozen at `D:\vjc-s5\bin\vjumpkung-comfyui-desktop-s5-02.exe`, SHA-256 `a642d3eb…42d0`; roots under `D:\vjc-s5\`; raw evidence `D:\vjc-s5\evidence\`; register **`docs/handoff/evidence/s5-native-register.md`**): S1 with 21 curated **PASS** (setup + one batch review of 21 + final verify CUDA on RTX 3060 + ComfyUI 0.42.1 `/system_stats` cuda:0 + embedded window + Stop); S4 moved install **detected correctly but rebuild FAILS** (D3); S6 HF download **FAILS** (D4); S16 part (rename while running, Thai name) **PASS**; S8 part (controlled lock refusal + Recycle Bin) **PASS**; original root stayed byte-identical after the restricted start. Curated import findings for the owner: MelBandRoFormer + WanVideoWrapper need torchaudio (omitted by the torch 2.14.1 profiles), rgthree-comfy @5f940ddfbb83 incompatible with ComfyUI 0.42.1.

**Pending packages — prompts written and advisor-approved; run strictly in this order** (rulings: `evidence/s5-advisor-standin-rulings.md`, `evidence/s5-advisor-r6.md`, thinker summary inside the rulings):
1. **D3 + D5** — `prompts/session5/d3-copied-python-link.md`: copied roots break `uv python install` (uv's minor-version junction `data/cache/uv-python/cpython-3.13-windows-x86_64-none` becomes a real dir → os error 145 → Rebuild/any setup on a copied root fails at `tools`); fix in `ensure_managed_python` (python.rs only). D5 = always pass `--no-registry` (uv registered the app's Pythons under `HKCU\Software\Python\Astral`).
2. **D4** — `prompts/session5/d4-hf-home.md`: every `hf` CLI download fails on Windows (child env has no `USERPROFILE`; hf 2.2.0 calls `Path.home()` at import); give the hf child an app-owned home + log a redacted stderr summary.
3. **D1 (R6 closure, security)** — `prompts/session5/d1-r6-closure.md`: residual #6 is a release blocker (pre-consent uv preview / git with node-controlled cwd; DLL policy is process-local). Option B: refuse new node content with top-level native images (Windows), Git checked via `ls-tree` before checkout; children without cwd start in the exe folder; live test that uv's interpreter query imports nothing from the node dir (**stop condition**).
Run each as coder (Codex `gpt-6.1-sol xhigh`, `CODEX_SANDBOX=danger-full-access` via `docs/handoff/tooling/run-codex.sh`, or the Sonnet xhigh Agent fallback pointed at the prompt file), then main re-runs the full check list (§5.6 + smoke test) and saves `s5-04…`, `s5-05…`, `s5-06…` checkpoints.

**Then (R + docs):** rebuild the frozen acceptance binary (`pnpm -C ui build` → `cargo build -p desktop --locked --features tauri/custom-protocol` → copy to `D:\vjc-s5\bin\…-s5-NN.exe`, record SHA-256) and do ONE native pass per `evidence/s5-advisor-r.md`: S1 default (21 curated) on a fresh root; S4 Retry the failed rebuild on `D:\vjc-s5\b` + N2b (original absent) + check no new HKCU key; S6 download/pause/resume (N4) + crash (N5, kill only the recorded PID); S10 registry node (N7); S11 dirty Git + enable/disable (N8); D1 live test with pinned uv 0.13.1; packaging build `pnpm -C ui tauri build` (do NOT run installers on the owner's host). Then write `docs/RELEASE_ACCEPTANCE.md` (structure in `evidence/s5-advisor-r.md`: snapshot / requirements / scenarios S01–S17 / evidence register / residuals R01–R2x / release gates; inputs `evidence/s5-exp-r.md`, `s5-native-register.md`), refresh IMPLEMENTATION_STATUS + REQUIREMENTS §9, and generate final README.md + docs/DOCUMENTATION.md (resolve every `<!-- VERIFY -->`). Release verdict stays **NO-GO** while Linux, installer, provider-token and signing gates are open.

**Owner questions (non-blocking, ask at the end):** (1) PlagueKind long-term: keep listed as unavailable (default) / remove / ask upstream to untrack `__pycache__` and bump the pin. (2) Stale registry keys created by dev builds: `HKCU\Software\Python\Astral\CPython3.13.16` (→ `D:\vjc-s5\c\…`) and `CPython3.12.15` (→ `D:\vjump\test-comfyui\…`) — owner may `reg delete` them. (3) Provider tokens / a disposable VM / a Linux NVIDIA machine are needed to close S5/S7/S12, installer and Linux gates. (4) Curated catalog maintenance: torchaudio-dependent nodes under the default profile, rgthree pin vs ComfyUI 0.42.1.

**Acceptance roots left on disk (disposable, ~40 GB on D:):** `D:\vjc-s5\a` (failed default run, evidence), `a2` (healthy 21-curated install = original of the moved copy; do not modify until N2a is re-verified), `b` (moved copy with the failed rebuild run `cbab877d…` — Retry it after D3), `c` (healthy, no curated; used for N4/N6/N9). Scripts: `D:\vjc-s5\drive.ps1` (PID-targeted launch/screenshot/click), `D:\vjc-s5\uia.ps1` (UI Automation: buttons/checkboxes/radios by name — robust for the WebView2 UI), `D:\vjc-s5\manifest.py` (tree hash manifest, run with `python -I`). Records: prompts `docs/handoff/prompts/session5/`, evidence `docs/handoff/evidence/s5-*.md`.

**Tooling lessons (session 5):** another Claude session may have left a `cargo test` running at start — check processes first. UI Automation (`uia.ps1`) beats coordinate clicks (42 consent checkboxes toggled exactly); the first synthetic click on an unfocused WebView2 window only focuses it. `ReadAllText` on a live `app.log` fails — open with `FileShare.ReadWrite`. Python on PATH is 3.11 (no `DirEntry.is_junction`).

## 0a. Session 4 summary (2026-10-19 16:28–23:50) — read this first

**Latest verification (main, 23:44, working copy = s4-11):** `cargo fmt --all -- --check` clean; `cargo clippy --workspace --all-targets --locked -j 6 -- -D warnings` clean; `cargo test --workspace --locked -j 6 --no-fail-fast` **709 passed, 0 failed, 9 ignored**; `node scripts/check-arch-deps.mjs` OK + its 10 self-tests; `cargo test -p desktop --test acl` **11 passed**; bindings drift test passes (1 passed, 1 ignored export); UI `pnpm -C ui lint` + `typecheck` pass, Vitest **28 files / 160 tests**. Migrations 001–008. Test-count history: 561 → 567 (first-run fix) → 582 (H3) → 593 (WP4) → 615 (WP5) → 633 (E1) → 652 (E2) → 657 (E2 fixes) → 697 (E3 + flake fix) → 709 (E3 fixes).

**Done and verified this session** (each re-verified by main, not only the coder; checkpoints are cumulative):

| # | Package | What | Checkpoint |
|---|---|---|---|
| 1 | **Owner-reported P0** | A clean first-run setup from the app always failed at `clone_comfyui` ("ComfyUI folder exists but is not a git checkout") because startup creates `ComfyUI/models` (DownloadStack/LibraryRoots, pinned) and `ComfyUI/custom_nodes/.disabled/.vjump-no-hooks` (node scan). Advisor-approved fix: a bounded non-following directory-only scaffold = `Absent`; the clone is merged in place with pinned no-replace renames, `.git` last (commit point), a durable `.ComfyUI.publish.json`, identity-verified rollback and recovery on Retry (`crates/infrastructure/src/environment/source/publish.rs`). Also: the xterm log frame no longer overflows its border (`ui/src/shared/ui/log-view.tsx`). | `s4-01-first-run-scaffold-fix.patch` |
| 2 | **H3** | Rejecting `IFileOperationProgressSink`, positive completion + `NotFound`; close policy (exit lease in `CloseCoordinator`, exact maintenance message, only `NodeRescanFailed` tolerated after a verified stop, stale/headless rechecks); `Services.gate`; `windows-core =0.62.2`. h3b fixed a **pre-existing** bug: model Recycle Bin deletion always failed on verbatim `\\?\` paths. | `s4-02-through-h3-verified.patch` |
| 3 | **WP4** | `NodeOperation::blocks_start` policy; final snapshot before Verified (equality without hook, fresh digest after a consented hook); identity-based recovery (source + commit + content); legacy `git:` copies preserved. | `s4-03-through-wp4-verified.patch` |
| 4 | **WP5** (+ advisor gap ruling) | `NodeRequirementSource` (`requirement_sets(&Path, Retained)`, `record_outcome`, `needs_repair(&EnvId, &Path, Retained)`), `NodeRequirementSet.file: Option<GuardedRequirements>`, mandatory `EnvironmentDeps.node_requirements`, `dependency_identity`, three-key lookup, fatal node requirement failure in every replacement mode, repair_check rules, `NodeAdapters::{open,bind}`; an oversized/non-UTF-8 `requirements.txt` = `UnsafeSource` (pre-existing silent skip fixed); conflicted/empty-digest nodes project `NotRun`. | `s4-05-through-wp5-verified.patch` |
| 5 | **E1 merged** | `e1-accelerators-verified.patch` applied with `git apply --3way` via a temporary index; WP1/WP5 + E1 provisioning combined; `reassert_pins` cancelled-token bug fixed (`installed_pins_in`); diagnostics render stored accelerator outcomes; migration 007. | `s4-06-through-e1-merge-verified.patch` |
| 6 | **E2** (+ thinker review corrections) | 22-entry curated catalog, `NodeTarget::{Active, Setup(RunId)}`, `SetupNodes` port (`SetupNodeError`, `pending`, `installed`), `setup_list_curated_nodes`, `RunSnapshotDto.curatedReviews: PlanReviewDto[]` + `curated: CuratedProgressDto[]`, Retry `curatedDecisions: CommitInput[]`, `StepFailureDto.canSkip`, `curated_selection_invalid`; corrections: persisted `CuratedStatus::Failed`, published entries never re-prepared, Skip refused while any entry is Failed, `publication_started` fails closed, install.py guidance = uninstall + reinstall with approval. | `s4-08-through-e2-final-verified.patch` |
| 7 | **E3** (+ thinker review corrections C1–C10, D1–D10) | `SetupMode::Rebuild`, `InstallationProbe`, `ExistingInstallOpener`, `RelocationStore`, `Db::open_existing`, migration 008, `setup_open_install` / `setup_new_install` → `RootSelectionDto { restartRequired }`, phase `rebuild_required` (`recovery.code` moved / interpreter_broken), `recovery_required` + `root_missing` / `root_inaccessible` with the recorded `installRoot`; corrections: restricted composition only for a moved root (no interpreter probe at compose), current-root reconciliation in restricted mode, status precedence, chained relocation, no stored `\\?\` paths, logging before compose, quiesce only on a successful restart, narrower open_existing refusal, whole-tree old-root assertion. | `s4-09…`, `s4-10…`, **`s4-11-through-e3-final-verified.patch`** |
| 8 | **Flaky runtime tests** | Root causes: machine-wide process snapshot (`runtime_guard::assess` treats any unreadable python process as Unknown) + a fixture port race. Fix: injectable `procscan::SnapshotFn` (production = real snapshot, behaviour-neutral), unique test ports, bounded waits. | in `s4-10` / `s4-11` |

Records: prompts used → `docs/handoff/prompts/session4/*.md` (incl. **`i-DRAFT-needs-advisor-review.md`**); advisor outputs → `docs/handoff/evidence/s4-advisor-first-run-fix.md`, `s4-advisor-wp5-gaps.md`; E2/E3 thinker reviews + flake root cause → `docs/handoff/evidence/s4-reviews.md`; residuals 10–21 → `docs/handoff/evidence/s3-residuals.md`; status ledger → `docs/IMPLEMENTATION_STATUS.md` (updated 23:50).

**Open points carried forward:**
- E3 decision D8 also refuses `open_existing` on the recorded old root itself, so a user in the moved restricted state cannot "Locate" the original install — decide in I (advisor).
- A moved root with a valid manifest but no `data/` still ends in startup-failed recovery; C10 and D2 lack dedicated tests.
- `real_setup.rs` composes setup dependencies directly and never ran after app startup → scenario 1 must be re-run from the app. The owner's failed run at `D:\vjump\test-comfyui` should now succeed with "Retry same run" (not confirmed). Real WebView2 xterm geometry not checked.
- Residual #6 (Windows DLL policy not inherited by child processes) still needs an advisor re-check.

**Tooling lessons:** Codex hit usage limits twice (the reset time is printed in the run log; probe with a one-line `codex exec` before launching). Claude Sonnet also hit a session limit once. Fallbacks that worked: Agent `general-purpose`, `model: sonnet`, `effort: xhigh`, pointed at the prompt file; thinker = Agent `model: opus`, read-only, standing in for the advisor. Bash heredocs containing quotes/backticks fail in the Bash tool — write prompt files with the Write tool. Apply foreign patches with `--3way` only through a temporary `GIT_INDEX_FILE` (the real index stays at HEAD). An orphaned `codex exec` survives a session end — check `Get-CimInstance Win32_Process -Filter "Name='codex.exe'"` before relaunching.

## 0. Session 3 summary (2026-10-19)
- **Environment changes:** repo moved to `D:\vjump\vjump-comfyui-portable-with-resource-manager`; the machine now has **32 GB RAM** (~8–10 GB free), 12 logical CPUs, still no NVIDIA GPU; worktrees under `C:\Users\vjump\vjc-wt\` are gone (P existed only as a patch). The owner's commits `929779f`/`4749d8a` put the session-2 working copy (incl. the unverified WP2+WP3 partial) and the handoff patches into HEAD.
- **Done and verified this session** (each verified by main re-running the checks, not only the coder):
  1. **WP2+WP3** finished (Codex) + advisor ruling on registry review warnings (see §3.1) → `s3-01-wp23-verified.patch`.
  2. **P merged** (updater + diagnostics) and three plan gaps closed: model-library scan/watch is quiesced with downloads before `install()`, buffered app-log lines are flushed (tracing guard dropped) immediately before `install()`, the library is restored after a failed install → `s3-02-wp23-p-verified.patch`.
  3. **Thinker security review of WP1/WP6** (Opus, read-only) → `docs/handoff/evidence/thinker-wp1-wp6-review.md` (2 high, 2 medium, 12 low). Advisor turned it into packages **H1/H2/H3** → `docs/handoff/plans/m5-h-hardening-approved-plan.md`.
  4. **H1** bounded, pinned filesystem work (Codex) → `s3-03-wp23-p-h1-verified.patch`.
  5. **H2** execution ownership, BOOT/stdin integrity, Git config safety, DLL policy, editable-requirement refusal (Codex partial → finished by Sonnet fallback) → `s3-04-h2-partial-unverified.patch` (the Codex partial) and cumulative **`s3-05-through-h2-verified.patch`**.
- **Latest verification (2026-10-19 15:15, working copy = s3-05):** `cargo fmt --check` clean; `cargo clippy --workspace --all-targets --locked -- -D warnings` clean; `cargo test --workspace --locked --no-fail-fast` **561 passed, 0 failed, 9 ignored**; `node scripts/check-arch-deps.mjs` OK. Last UI run (after P, unchanged since): lint, typecheck pass, **28 files / 158 tests pass**; bindings regenerated after P, drift test passes; ACL 10 tests pass.
- **Not yet done:** H3, WP4, WP5, merge E1, E2, E3, I, R (+ `docs/RELEASE_ACCEPTANCE.md`), README/DOCUMENTATION VERIFY pass. See §5.4.

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
4. `docs/handoff/`: approved plans (`plans/`), evidence (`evidence/`), coder prompts (**session-4 prompts as actually run: `prompts/session4/`**, each already contains `prompts/session3/common.md` and the session-4 baseline notes; next package prompt draft: `prompts/session4/i-DRAFT-needs-advisor-review.md`), saved patches (`patches/`, authoritative = `s4-11-through-e3-final-verified.patch`) and local build tooling (`tooling/`).
5. `docs/handoff/evidence/s3-residuals.md` (residuals 1–21) and `docs/handoff/evidence/s4-reviews.md` (E2/E3 review decisions + notes for package I).

Reference material:

| What | Where now |
|---|---|
| Reference portable installer (setup steps, 22 curated nodes, accelerator wheels) | Owner's private GitHub repo `vjumpkung/pixellatent-happy-meals-scripts`, file `comfyui-python-3-12-happy-meals/portable_version/install_comfyui_3_12_uv_nvidia_torch_2100_portable.bat` (`gh api repos/.../contents/<path> -H "Accept: application/vnd.github.raw"`). Everything M6 needs is extracted into `docs/handoff/plans/m6-reference-data.md`. |
| Python backend reference | `C:\Users\vjump\Documents\vjumpkung-sd-ui-manager-backend` (not needed for remaining work). |
| Code inventory for documentation | `docs/handoff/evidence/docfacts.md` (explorer, 2026-10-17, file:line evidence of the committed app — predates session 3). |
| UI surface for package I | `docs/handoff/evidence/exp-ui-i.md` (explorer, 2026-10-19). |

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
| **Commit only when the user asks**; checkpoints are patch files | User | working agreement |

### 3.1 Advisor decisions (AI-decidable — accepted; details in `docs/handoff/plans/*`)
- **Custom-node containment:** real pinned-parent directory operations for all node mutations (NFR-SEC-5 check/use races), incl. pinned recycle with no permanent-delete fallback.
- **WP1 gap decision (`plans/m5-wp1-gap-decision.md`):** registry archive downloads into a create-new pinned file reused for extraction; a typed **`NodeLocationGuard`** keeps pins and pinned inputs alive through preview, dependency install, hook execution and cleanup (incl. `CleanupFailed`).
- **Git node identity = content tree digest** (not `git:<commit>`); Git updates stage a full checkout of the frozen target; post-apply validation before deps/hooks; `GitRefused { status }` keeps bounded `git status`. **Implemented (WP2).**
- **Node operations can never authorize protected-package (torch/triton) changes**; constrained preview only; FR-ENV-3 keeps its own confirmation flow. **Implemented (WP3).**
- **Registry review warnings (new, 2026-10-19):** compatibility (OS/accelerator/ComfyUI) comes **exclusively from the exact release** being installed (missing dimensions stay "unknown"); flagged/deprecated = exact release OR the **top-level node's** own flags; flags present only on an unrelated `latest_version` never propagate; one `warnings()` call, naturally deduplicated. `RegistryNode` gained `node_flagged`/`node_deprecated`; registry cache schema bumped to 2. **Implemented.**
- **Final tree digest** must equal the reviewed digest unless a consented hook ran (then refresh); ambiguous recovery in any non-terminal phase **blocks launch** (`NodeOperation::blocks_start`). **Implemented (WP4).**
- **Repair/replacement:** node requirement sets from a `NodeRequirementSource` port in application `custom_nodes/requirements.rs`; any node requirement failure is **fatal in every replacement mode**; `RepairRequired` always rebuilds; dependency state per `(node, env, dependency_identity)`; missing = `not_run`; no migration. **Amended (H plan §7):** `NodeRequirementSet.file: Option<GuardedRequirements>` carrying protected bytes + resource token; propagate the lease retention; remove `repair_requirements`. **Implemented (WP5)**; advisor gap ruling: only NotFound = no requirements file (oversized/non-UTF-8 → `UnsafeSource`, validated before the preview's early return); conflicted / empty-digest nodes project `NotRun`.
- **Session 4 decisions (accepted; details `evidence/s4-advisor-first-run-fix.md`, `evidence/s4-reviews.md`):** first-run scaffold = bounded directory-only tree treated as Absent, clone merged in place with a durable publication record (never replace pinned dirs); verbatim `\\?\` paths converted for the Windows shell recycle API (other verbatim forms refused); curated nodes install detached at the exact commit, install.py never runs in setup (guidance: uninstall + reinstall with approval), Skip refused while any curated entry is Failed; E3 restricted composition only for a moved root, current-root runs reconciled in restricted mode, relocation rebases from every recorded root and stores non-verbatim paths, quiesce only on a successful restart; runtime tests use an injectable process snapshot (`procscan::SnapshotFn`).
- **Launch lifecycle:** `NodeLifecycle` runtime port; Start/Restart/Stop ordering under one lease. **Implemented (WP6); thinker verified the ordering.**
- **Frontend contract additions (C0):** implemented/committed.
- **M6:** curated nodes run before activation against an explicitly bound setup candidate (`NodeTarget::Setup(RunId)`, `SetupNodes` port injected into `ExecuteSetup`), one batch in the multi-plan review, never `install.py`; accelerators = hashed wheels + package component, py312 wheels experimental/unchecked, py313/314 lists empty; relocation detected before any startup reconciliation; separate `setup_open_install` / `setup_new_install`; rebuild into a new app-owned env with the exact recorded profile revision; HF relay for every HF attempt; updater env names `VJUMP_UPDATER_PUBKEY` / `VJUMP_UPDATER_ENDPOINT`, install bound to the reviewed offer, no check-on-launch setting; diagnostics redact the whole report, clipboard from Rust with a textarea fallback.
- **M5 hardening (new, 2026-10-19, `plans/m5-h-hardening-approved-plan.md`):** split into **H1 → H2 → H3, all before WP4**; caps 64 path components / 1024-byte relative paths / 255-byte components / 8 MiB archive name budget; registry archives refuse `.git` and `__pycache__`; Git commands run with an app-owned empty global config + `GIT_CONFIG_NOSYSTEM=1` and a non-executing `git config --local --no-includes` preflight refusing `filter.*`, includes and worktree redirection; keep stdin + node-dir cwd for guarded requirements; BOOT verifies length + SHA-256; **retain rejection of all reparse points** (no allowance for WOF/cloud/dedup tags); **F16 relay authentication = accepted Release-1 residual**; Linux post-`renameat2` identity checks and `PR_SET_PDEATHSIG` deferred to a native Linux package (never "rename back").
- **Main's integration decisions:** coders may make the minimal desktop wiring needed to make their package testable (commands, ACL cases, bindings regeneration, compile-fix TS); package I builds the UI and remaining composition.

## 4. Open items (spec §9 — need runtime validation) and residuals
- Accelerator wheel compatibility (hashes ≠ ABI/CUDA proof); FA3 import module name unverified.
- Native HF engine vs Xet files; whether newer `hf` restores cross-run resume; opt-in relay test `real_hf_xet_obeys_relay_with_direct_egress_denied` not run.
- WebKitGTK performance and Thai fonts on Linux; Windows code signing and updater key custody.
- **All Linux-only code paths (WP1/H1/H2: openat pins, `/proc/<pid>/fd/<n>/.` anchors, `renameat2`, `O_NONBLOCK`, FIFO test, trash placement, Unix BOOT/preflight branches) have never been compiled or run** — no Linux target C toolchain on this machine.
- **This machine cannot produce NVIDIA or Linux evidence.** All CUDA/setup/accelerator/Linux acceptance remains open; Windows packaging only partly covered (unsigned NSIS/MSI built on 2026-10-17 from an older tree, installers not run).
- Session-3 residuals (details + evidence in `docs/handoff/evidence/s3-residuals.md`): downloads need an app restart after a failed self-update install; unauthenticated loopback HF relay (accepted); Windows DOS-path remapping boundary and all-reparse-points refusal; **`SetDllDirectoryW("")` is not inherited by children** (uv/git/python started with a node-dir cwd keep the default DLL search — needs an advisor re-check); early non-zero exit before stdin is consumed reports `InputFailed`; Git config preflight is a separate process (same-user edit window).

## 5. Implementation status and next steps

### 5.1 Environment facts (important)
- Machine: Windows 11 x64, **NVIDIA GeForce RTX 3060 12 GiB present (driver 610.88 / CUDA 13.3, verified 2026-10-20 — the earlier "no NVIDIA" note was wrong)**, 12 logical CPUs, **32 GB RAM, ~8–10 GB free** (the owner's VS Code + rust-analyzer and a VS Code Codex extension also run). `-j 4`–`-j 6` cargo builds are fine; still run only one heavy cargo command at a time.
- **Build toolchain = MSVC (default)**: VS 2022 Community 17.14, MSVC v143 14.44.35207, Windows 11 SDK 10.0.22621; `rust-toolchain.toml` = `1.99.0-x86_64-pc-windows-msvc`, default `target/` (now warm). GNU toolchain = fallback only (`docs/handoff/tooling/gnu-env.sh`).
- Tools on PATH: git 2.55.0.windows.3, Node 24.13.0, pnpm 10.24.0, Python 3.12 (uv-managed), uv (system; the pinned **uv 0.13.1** can be fetched from the URL in `crates/infrastructure/src/environment/pins.rs` and checked against its SHA-256).
- **Codex:** use `docs/handoff/tooling/run-codex.sh <model> <effort> <name>` with `CODEX_SCRATCH=<dir holding <name>.prompt.md>`. It now honours **`CODEX_SANDBOX`**: coders **must** use `danger-full-access` (the default workspace-write sandbox blocks git `CreateFileMapping`, node spawn and loopback tests, which produced 19 false Git failures); advisors/explorers use `read-only`. Models per the user's CLAUDE.md: coder/advisor `gpt-6.1-sol xhigh`, explorer `gpt-6-luna xhigh`, hard advisor `gpt-6-astra xhigh`. **Usage limits hit on 2026-10-19 at 14:54, ~18:20 and ~22:20 (last reset stated 2026-10-20 02:30)** — probe with a one-line `codex exec` before launching a package. Never overwrite `run-codex.sh` while a run is using it (bash reads scripts incrementally).
- **Fallback that works:** Agent tool, `general-purpose`, `model: sonnet`, `effort: xhigh`, pointed at the session3 prompt file + `common.md` + the isolated partial-delta patch (H2 was finished this way). Thinker = Agent `model: opus`.
- **The git index is not a reliable checkpoint:** it was found reset to HEAD twice (14:22 and ~14:50), both times while a Codex run was active, with no reflog entry and no git write by any agent — suspected the VS Code Codex extension or Codex CLI. **Build checkpoints with temporary index files** (never touches the real index):
  ```bash
  export GIT_INDEX_FILE=$TEMP/idx-work; rm -f $GIT_INDEX_FILE; git read-tree HEAD
  git add -A crates ui docs Cargo.toml Cargo.lock HANDOFF.md; work=$(git write-tree)
  git diff --binary HEAD $work -- . ':!docs/handoff/patches' > docs/handoff/patches/s3-NN-<name>.patch; unset GIT_INDEX_FILE
  ```
  For a package-only delta, build `base` the same way from `git read-tree HEAD && git apply --cached <previous cumulative patch>`.
- Generated permission TOMLs under `crates/desktop/permissions/autogenerated/` (five new ones from P are untracked files — keep them). Codex on Windows sometimes writes CRLF; normalize edited files to LF (`.gitattributes` = `* text=auto`).
- Vitest: `VITEST_MAX_WORKERS=2`.

### 5.2 Milestone status
| Milestone | Status |
|---|---|
| M0–M4 | Implemented in earlier sessions (native acceptance mostly open, see IMPLEMENTATION_STATUS). |
| M5 Custom nodes | **Backend + UI complete and verified on Windows** (C0/FE, WP6, WP1, WP2+WP3, H1, H2, H3, WP4, WP5). Native Linux and scenarios 10/11/14/16/17 open. |
| M6 Release 1 | **Backend complete and verified on Windows:** B, P, E1 (merged), E2, E3 (each with review corrections). **I (UI integration) and R (release audit/packaging) not started.** |
| Docs | README.md + docs/DOCUMENTATION.md = session-2 drafts (`<!-- VERIFY -->` markers: 1 in README, 26 in DOCUMENTATION); `docs/RELEASE_ACCEPTANCE.md` not written; **IMPLEMENTATION_STATUS refreshed 2026-10-19 23:50** (milestones + verification). |

### 5.3 Work packages — exact state
Specs: M5 backend = `docs/handoff/plans/m5-backend-approved-plan.md` (+ `m5-wp1-gap-decision.md`); M5 hardening = `m5-h-hardening-approved-plan.md`; M5 frontend = `m5-frontend-approved-plan.md`; M6 = `m6-approved-plan.md` + `m6-reference-data.md`. **Prompts: `docs/handoff/prompts/session3/<pkg>.md` + `common.md`.**

| Package | State | Evidence / checkpoint |
|---|---|---|
| C0 + FE (M5 contract + UI) | Committed in HEAD. Missing gateway test `node_subscription_keeps_tauri_channel_inside_gateway` → fold into I. | — |
| WP6 launch lifecycle | Done (HEAD). Thinker: ordering correct, no deadlock. | — |
| WP1 pinned containment + gap fix | Done (HEAD); hardened by H1/H2/H3. | thinker review |
| B bandwidth + HF relay | Done (HEAD). Residual F16 accepted. | — |
| **WP2+WP3** | **Done + verified.** Content tree digest for Git+registry; full staged checkout of the frozen Git target; post-apply validation before deps/hooks; `GitRefused { status }` with bounded redacted status; constrained-only previews, protected changes refused; registry detail errors propagate; warnings per §3.1. Named tests all pass (git tests really executed). | `s3-01-wp23-verified.patch` |
| **P** updater + diagnostics | **Merged + verified.** Commands `app_update_status/check/install`, `app_diagnostics`, `app_copy_diagnostics`; plugins `tauri-plugin-updater =2.14.0`, `tauri-plugin-clipboard-manager =2.5.0`; quiesce downloads + library, flush log before `install()`, restore library after failure. Gap: downloads need restart after failed install. Accelerator section shows an availability warning until E1 is merged. | `s3-02-wp23-p-verified.patch` |
| **H1** bounded pinned filesystem | **Done + verified.** Limits in `custom_nodes/tree.rs`; `(parent_id, folded)` trie; iterative `remove_owned_tree` (≤64 frames); pinned `read_tree/validate_tree/tree_digest(&Arc<PinnedDirectory>)`; `PinnedDirectory::create_new_child`; archive `.git`/`__pycache__` refusal; relative-only cache exclusions; Unix `O_NONBLOCK` + unsupported-Unix `compile_error!` (uncompiled). 14 named tests pass (FIFO test Linux-only). | `s3-03-wp23-p-h1-verified.patch` |
| **H2** execution ownership / BOOT / Git | **Done + verified.** application `shared/retained.rs` (`Retained::merge`), `MaintenanceLease::retained()`, `StepContext.retained`, `TokioLease` with `Arc<OwnedMutexGuard>`; retention through node git_status/publish/recover/scan/snapshot; `RunError::InputFailed` + supervised stdin writer; BOOT length/SHA-256 verification, clean `__main__`, safe-path respect; Git empty global config + `GIT_CONFIG_NOSYSTEM=1` + config preflight; fixed Git error messages (`"The Git command failed."`, `"Repository configuration requires external inspection."`); `SetDllDirectoryW("")` for the app/runner (not inherited by children — residual); pre-consent refusal of editable/source requirements (PEP 508 markers accepted). 15 named tests + 2 extra pass. | `s3-04-h2-partial-unverified.patch` (Codex partial), **`s3-05-through-h2-verified.patch` (cumulative, authoritative)** |
| **First-run scaffold fix** (owner P0) | **Done + verified** (advisor-approved). See §0a #1. | `s4-01-first-run-scaffold-fix.patch`, `evidence/s4-advisor-first-run-fix.md` |
| **H3** Windows recycle sink + close policy | **Done + verified** (+ h3b verbatim-path recycle fix). | `s4-02-through-h3-verified.patch` |
| **WP4** final digest + identity-based recovery | **Done + verified.** | `s4-03-through-wp4-verified.patch` |
| **WP5** NodeRequirementSource, fatal node deps, repair_check, projection, NodeAdapters split | **Done + verified** (+ wp5b advisor gap fixes). | `s4-05-through-wp5-verified.patch`, `evidence/s4-advisor-wp5-gaps.md` |
| **E1** accelerators | **Merged + verified** (Sonnet fallback). Real `uv` wheel install not exercised. | `s4-06-through-e1-merge-verified.patch` |
| **E2** curated nodes | **Done + verified** (+ e2b thinker-review corrections). No real-Git curated test. | `s4-08-through-e2-final-verified.patch`, `evidence/s4-reviews.md` |
| **E3** relocation / guided rebuild | **Done + verified** (+ e3b thinker-review corrections C1–C10/D1–D10; flaky runtime tests isolated). | **`s4-11-through-e3-final-verified.patch`**, `evidence/s4-reviews.md` |
| **I** UI + remaining composition | **DONE + verified in session 5 (§0b, `s5-02-through-i-verified.patch`); the text below is historical.** Prompt drafted by main: `prompts/session4/i-DRAFT-needs-advisor-review.md` — send it to the advisor first** (also decide E3 D8: may the user "Locate" the recorded old root?). Inputs: `evidence/exp-ui-i.md`, `evidence/s4-reviews.md` (notes for I), current bindings. DTO additions owned by I: `ProfileDto.accelerators`, `SetupRequestDto.accelerators` (E1 added it only in application), `EnvironmentInfoDto.components`. Already in bindings: `setupListCuratedNodes`, `setupOpenInstall`, `setupNewInstall`, `RunSnapshotDto.{curated,curatedReviews}`, `SetupActionDto.retry.curatedDecisions`, `StepFailureDto.canSkip`, phase `rebuild_required`. Missing gateway test `node_subscription_keeps_tauri_channel_inside_gateway` belongs to I. | — |
| **R** release audit + docs | Not started: `docs/RELEASE_ACCEPTANCE.md` (every P0/P1 + 17 scenarios → evidence or open blocker), IMPLEMENTATION_STATUS/REQUIREMENTS §9 refresh, resolve every `<!-- VERIFY -->` in README/DOCUMENTATION, packaging audit (`pnpm -C ui tauri build`). Use `evidence/s3-residuals.md`. | — |

### 5.4 Resume here — SUPERSEDED by §0b (session 5); kept for history
1. `git status` (expect ~120 modified/untracked files; HEAD `a879fc0`). Confirm the working copy matches `s4-11-through-e3-final-verified.patch` (build a temp-index tree as in §5.1 and compare, or `git diff HEAD --stat`) and run `cargo test --workspace --locked -j 6 --no-fail-fast` (expect **709 passed / 0 failed / 9 ignored**). If the owner committed the working copy in the meantime, HEAD will differ — trust the commit and re-run the checks.
2. Probe Codex availability (one-line `codex exec`); otherwise use the Sonnet/Opus fallbacks (§0a tooling lessons).
3. **I**: send `prompts/session4/i-DRAFT-needs-advisor-review.md` to the advisor (gpt-6.1-sol xhigh, read-only) together with the E3 D8 question and `evidence/s4-reviews.md` notes for I → corrected prompt → coder → main verifies the full check list (§5.6 incl. `pnpm -C ui build` and `cargo build -p desktop --features tauri/custom-protocol`) → checkpoint `s4-12-…` / `s5-01-…`.
4. **R**: write `docs/RELEASE_ACCEPTANCE.md` (every P0/P1 + all 17 scenarios → evidence or explicit open blocker; use `evidence/s3-residuals.md` 1–21 and `docs/IMPLEMENTATION_STATUS.md`), refresh REQUIREMENTS §9, packaging audit (`pnpm -C ui tauri build`, NSIS/MSI), keep workflows disabled, placeholder icons and license status. Ask the advisor to re-check residual #6 (DLL policy not inherited by children).
5. Ask the owner to re-run a clean first-run setup **from the app** (scenario 1 through app startup); `real_setup.rs` bypasses startup.
6. Generate final **README.md** and **docs/DOCUMENTATION.md** from verified behaviour + the acceptance ledger: resolve every `<!-- VERIFY -->` marker (1 in README, 26 in DOCUMENTATION — most now refer to implemented packages WP1–WP6, B, P, E1–E3), document residuals and which acceptance scenarios remain unverifiable on this machine (NVIDIA, Linux, packaging, providers).

### 5.5 Verification history
| Check | Result |
|---|---|
| HEAD `4749d8a` (2026-10-19 13:30): `cargo test --workspace --no-run` | compiles |
| After WP2+WP3 (13:57) | fmt, clippy, 495 Rust tests (8 ignored), arch OK |
| After P merge (14:16) | fmt, clippy, 526 Rust tests, UI 28 files/158 tests, lint, typecheck, bindings drift, ACL 10, arch OK |
| After H1 (14:36) | fmt, clippy, 540 Rust tests, arch OK |
| After H2 (15:15) | fmt, clippy, **561 Rust tests (9 ignored)**, arch OK |
| Session 4 (16:28–23:44), after each package | first-run fix 567 · H3 582 · WP4 593 · WP5 615 · E1 633 · E2 652 → 657 · E3 + flake fix 697 · E3 corrections **709 passed / 0 failed / 9 ignored**; fmt, clippy, arch (+10 self-tests), ACL 10→11, bindings drift, UI lint/typecheck, 28 files / 160 tests |
| Earlier tree (2026-10-17): `pnpm -C ui tauri build` | unsigned NSIS + MSI built; installers not run |
| Everything else (native NVIDIA/Linux/packaging/provider acceptance) | not verified |

### 5.6 Full check list (run on the merged tree)
```bash
pnpm -C ui install --frozen-lockfile && pnpm -C ui lint && pnpm -C ui typecheck && VITEST_MAX_WORKERS=2 pnpm -C ui test && pnpm -C ui build
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -j 4 -- -D warnings
cargo test --workspace --locked -j 4 --no-fail-fast
node --test scripts/check-arch-deps.test.mjs && node scripts/check-arch-deps.mjs
cargo test -p desktop --test bindings --locked
cargo build -p desktop --locked --features tauri/custom-protocol
pnpm -C ui tauri build   # Windows NSIS + MSI bundles (MSVC only; release build, ~12 min)
```
Also: coverage (domain ≥90 %, application ≥80 %, `cargo llvm-cov`), and the acceptance scenarios in REQUIREMENTS §8 (`docs/RELEASE_ACCEPTANCE.md` must map every P0/P1 item and all 17 scenarios to evidence or an explicit open blocker).

## 6. Working agreement and tooling
- Follow `AGENTS.md` and the user's global CLAUDE.md (main orchestrates; explorer → plan → advisor → coder → main verifies; ask the user only for advisor-classified user-only decisions).
- One writer per file set; packages run sequentially in the main working copy; main re-runs the checks after every coder and saves a cumulative checkpoint patch.
- Coders must not run git write commands (add/reset/checkout/restore/stash/commit/apply/clean).
- Commit only when the user asks.
