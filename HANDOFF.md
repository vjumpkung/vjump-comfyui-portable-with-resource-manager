# HANDOFF — vjumpkung-rust-comfyui-desktop

Last updated: **2026-10-19 15:20 (+07:00)**. **Paused by the user during M5 hardening package H3.** All subagents and Codex jobs were stopped on request; no build, test or agent process of this session is running (the owner's VS Code Codex extension `app-server` and the MCP servers are separate). Branch `feat/m0-skeleton`, HEAD `4749d8a`. **Nothing from session 3 is committed.** The working copy holds verified work through **H2** (checkpoint `docs/handoff/patches/s3-05-through-h2-verified.patch` = `git diff HEAD → working copy`). H3 had not changed any file when it was stopped. README.md and docs/DOCUMENTATION.md are still the session-2 drafts with `<!-- VERIFY -->` markers.

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
4. `docs/handoff/`: approved plans (`plans/`), evidence (`evidence/`), coder prompts (**current set: `prompts/session3/`**, append `prompts/session3/common.md`), saved patches (`patches/`) and local build tooling (`tooling/`).
5. `docs/handoff/evidence/s3-residuals.md`: accumulated residuals/limitations for the release audit and docs.

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
- **Final tree digest** must equal the reviewed digest unless a consented hook ran (then refresh); ambiguous recovery in any non-terminal phase **blocks launch** (`NodeOperation::blocks_start`). *(WP4, not started.)*
- **Repair/replacement:** node requirement sets from a `NodeRequirementSource` port in application `custom_nodes/requirements.rs`; any node requirement failure is **fatal in every replacement mode**; `RepairRequired` always rebuilds; dependency state per `(node, env, dependency_identity)`; missing = `not_run`; no migration. **Amended (H plan §7):** `NodeRequirementSet.file: Option<GuardedRequirements>` carrying protected bytes + resource token; propagate the lease retention; remove `repair_requirements`. *(WP5, not started.)*
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
- Machine: Windows 11 x64, **AMD Radeon GPU only (no NVIDIA)**, 12 logical CPUs, **32 GB RAM, ~8–10 GB free** (the owner's VS Code + rust-analyzer and a VS Code Codex extension also run). `-j 4`–`-j 6` cargo builds are fine; still run only one heavy cargo command at a time.
- **Build toolchain = MSVC (default)**: VS 2022 Community 17.14, MSVC v143 14.44.35207, Windows 11 SDK 10.0.22621; `rust-toolchain.toml` = `1.99.0-x86_64-pc-windows-msvc`, default `target/` (now warm). GNU toolchain = fallback only (`docs/handoff/tooling/gnu-env.sh`).
- Tools on PATH: git 2.55.0.windows.3, Node 24.13.0, pnpm 10.24.0, Python 3.12 (uv-managed), uv (system; the pinned **uv 0.13.1** can be fetched from the URL in `crates/infrastructure/src/environment/pins.rs` and checked against its SHA-256).
- **Codex:** use `docs/handoff/tooling/run-codex.sh <model> <effort> <name>` with `CODEX_SCRATCH=<dir holding <name>.prompt.md>`. It now honours **`CODEX_SANDBOX`**: coders **must** use `danger-full-access` (the default workspace-write sandbox blocks git `CreateFileMapping`, node spawn and loopback tests, which produced 19 false Git failures); advisors/explorers use `read-only`. Models per the user's CLAUDE.md: coder/advisor `gpt-6.1-sol xhigh`, explorer `gpt-6-luna xhigh`, hard advisor `gpt-6-astra xhigh`. **Usage limit hit at 14:54 on 2026-10-19 (reset stated 18:27).** Never overwrite `run-codex.sh` while a run is using it (bash reads scripts incrementally).
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
| M5 Custom nodes | C0 contract + UI committed. **WP6, WP1, WP2+WP3, H1, H2 done and verified.** **H3, WP4, WP5 not started.** |
| M6 Release 1 | **B and P merged and verified.** E1 done as a verified patch, **not merged** (merge after WP5). E2, E3, I, R not started. |
| Docs | README.md + docs/DOCUMENTATION.md = session-2 drafts (`<!-- VERIFY -->` markers); `docs/RELEASE_ACCEPTANCE.md` not written; IMPLEMENTATION_STATUS refreshed 2026-10-19 (milestone table only). |

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
| **H3** Windows recycle sink + close policy | **Not started** (stopped by the user before any edit). Prompt `session3/h3.md`. | — |
| **WP4** final digest + identity-based recovery | Not started. Prompt `session3/wp4.md` (with H amendment). | — |
| **WP5** NodeRequirementSource, fatal node deps, repair_check, projection, NodeAdapters split | Not started. Prompt `session3/wp5.md` (with H amendment). Prerequisite for E1 merge/E2/E3. | — |
| **E1** accelerators | Verified patch `e1-accelerators-verified.patch` (base `0ffa136`), **not merged**. Dry-run 3-way vs HEAD conflicts only in `application/src/environment/ports.rs` (`InstallSpec.guarded` vs `no_deps` + `WheelFetcher`); after WP5 expect more. Prompt `session3/merge-e1.md` (incl. rendering stored accelerator outcomes in P's diagnostics). | — |
| **E2** curated nodes | Not started. Prompt `session3/e2.md`. After WP5 + E1. | — |
| **E3** relocation / guided rebuild | Not started. Prompt `session3/e3.md`. After E2. | — |
| **I** UI + remaining composition | Not started; prompt not yet written. Evidence `evidence/exp-ui-i.md`: wizard has no accelerator/curated state; `StepFailureDto` lacks `can_skip` (Skip inferred from policy); `App.tsx` recovery branch has no Locate/New; Environment page shows only an `accelerators: string[]`; custom-nodes barrel exports only `CustomNodesPage` (ReviewDialog must be exported publicly for reuse); stale copy at `App.tsx:66`, `setup-wizard.tsx:671`, `:711`, `app-shell.tsx:107`. Bandwidth/updates/diagnostics UI already present with tests. | — |
| **R** release audit + docs | Not started: `docs/RELEASE_ACCEPTANCE.md` (every P0/P1 + 17 scenarios → evidence or open blocker), IMPLEMENTATION_STATUS/REQUIREMENTS §9 refresh, resolve every `<!-- VERIFY -->` in README/DOCUMENTATION, packaging audit (`pnpm -C ui tauri build`). Use `evidence/s3-residuals.md`. | — |

### 5.4 Resume here (recommended order)
1. `git status` (expect many modified + untracked files; the index may be empty — that is fine). Confirm the working copy matches `s3-05-through-h2-verified.patch` (`git diff HEAD --stat` ≈ 103 files) and run a quick `cargo test --workspace --locked -j 6` (expect 561 passed / 9 ignored).
2. **H3** (`session3/h3.md` + `common.md`) → verify → cumulative checkpoint `s3-06-...`.
3. **WP4** → **WP5** sequentially (full workspace checks after each; checkpoint after each).
4. **Merge E1** (`git apply --3way docs/handoff/patches/e1-accelerators-verified.patch`, then coder with `session3/merge-e1.md`); regenerate bindings; full check list.
5. **E2 → E3** (prompts ready), then write the **I** prompt from `m6-approved-plan.md` §I + `evidence/exp-ui-i.md` (main drafts → advisor review), run it, then **R**.
6. Ask the advisor to re-check residual #6 (DLL policy not inherited by children) when Codex is available.
7. Generate final **README.md** and **docs/DOCUMENTATION.md** from verified behaviour + the acceptance ledger; record which acceptance scenarios remain unverifiable on this machine.

### 5.5 Verification history
| Check | Result |
|---|---|
| HEAD `4749d8a` (2026-10-19 13:30): `cargo test --workspace --no-run` | compiles |
| After WP2+WP3 (13:57) | fmt, clippy, 495 Rust tests (8 ignored), arch OK |
| After P merge (14:16) | fmt, clippy, 526 Rust tests, UI 28 files/158 tests, lint, typecheck, bindings drift, ACL 10, arch OK |
| After H1 (14:36) | fmt, clippy, 540 Rust tests, arch OK |
| After H2 (15:15) | fmt, clippy, **561 Rust tests (9 ignored)**, arch OK |
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
