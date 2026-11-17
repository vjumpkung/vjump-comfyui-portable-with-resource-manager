# Implementation status

This is the working completion ledger. `REQUIREMENTS.md` remains the release scope;
passing a subset of checks does not establish release completion.

Last updated: **2026-11-17 (+07:00)**. Branch `main` at `881aa90`, version 0.1.5; v0.1.5 was
published from `48f3f60` (Windows NSIS, MSI and plain exe; Linux .deb and AppImage; no signing step;
updater inert). Hosted CI on `881aa90` is green on Windows and Ubuntu, coverage included.
Native evidence: `docs/handoff/evidence/s7-native-register.md` (Windows W1–W10, Linux container
L1–L8, session-7 packaging) and `docs/handoff/evidence/s6-native-register.md` (N1–N29), Windows 11,
NVIDIA GeForce RTX 3060 12 GiB, driver 610.88 / CUDA 13.3, plus the session 9–13 rows in
RELEASE_ACCEPTANCE.md (sessions 11–13 ran on a second Windows machine without an NVIDIA GPU;
narrative: `git show 881aa90:HANDOFF.md`). Session-6 packaging and snapshot:
`docs/handoff/evidence/s6-packaging.md`. Acceptance ledger: [RELEASE_ACCEPTANCE.md](RELEASE_ACCEPTANCE.md).
Release verdict: **NO-GO** (see "Release gates" below).

## Milestones

| Milestone | Current evidence | Remaining work |
| --- | --- | --- |
| M0 — skeleton | Workspace, UI shell, settings, keyring, redacted logging, generated IPC, compiled ACL tests and architecture checks. **Session 6:** the shell keeps all scrolling inside the main area (no second document scrollbar, sidebar full height — N3); the active row in the Models and Custom Nodes tables is a muted row with an accent bar instead of a pink fill with unreadable text (N4); a debug build no longer overflows the 1 MiB Windows main-thread stack when custom nodes are installed (8 MiB main-thread reserve and 8 MiB tokio worker stacks, PE and worker tests — N1, N6, N7). **After session 7:** English + Thai UI (S9-NATIVE-WIN-001, S13-NATIVE-WIN-003), Prettier owns `ui/` formatting, responsive startup (tests only), hosted CI green (HCI-WIN-001, HCI-LINUX-001). | Linux acceptance. |
| M1 — setup | Hardware/profile detection, managed/custom Python, pinned tools, hash-locked hf, provisioning journal, activation recovery, pin guard. **Session 6, verified natively on a fresh root (N8–N13):** nothing is created inside `ComfyUI/` before setup clones it, and the app restarts once after first-time setup; the recommended Windows profile is now **Python 3.12.15 + PyTorch 2.10.0 cu130 with torchaudio 2.10.0** (listed first, preselected); the first-run wizard ticks ComfyUI-Manager by default and offers the curated nodes unticked; the review step discloses the third-party sources and **Start setup** is the confirmation, so setup never pauses for consent; curated nodes install the **latest commit of each repository's default branch** (no commit lock) and track that branch; ComfyUI source choice offers Latest release (default), **Latest commit (master)** and a specific tag. Python 3.12 children now receive `USERNAME` (Python 3.12's `getpass` fallback to `pwd` crashed `torch._dynamo`/ComfyUI before — N5). A full first run with ComfyUI-Manager + 21 curated nodes finished in ~8.5 min with no pause; all nodes imported on cuda:0 (N11, N13). **After session 7:** CPU testing profiles (S11-NATIVE-WIN-001, S11-NATIVE-WIN-005), py312 cu130 profile revision 3, Restart now after an install-root change (tests only, R43), LF clones + portable MinGit block (S13-NATIVE-WIN-002), `hf version` timeout 60 s, ComfyUI-Manager `config.ini` created if missing (FR-SETUP-14, tests only). | Linux desktop (a first run with `linux-py313-torch2141-cu130` passes in a GPU container — S7 L4); custom-venv/adopted and change-torch scenarios natively; a first run with exactly the final defaults (Manager only). The final-code binary ran a first run with Manager + PlagueKind (N28), and the final defaults were checked in the wizard (N25, N26). Residuals R33, R39, R42. |
| M1 — moved install (FR-SETUP-12) | **Verified natively on a `robocopy /E` copy (N19, N20):** the copy is detected as moved, Rebuild removes uv's copied minor-version folders for every managed Python (D3/D3b) and succeeds; managed Python is no longer registered under `HKCU\Software\Python\Astral` (`--no-registry`, D5); ComfyUI then runs with all nodes on cuda:0. | Linux symlink branch; "original root absent" variant. |
| M2 — runtime | Start/stop/restart under the maintenance lease, logs, embedded window, close policy, update. **Session 6:** ComfyUI stays on a git branch that tracks `origin`, so `git pull` works — `master` for `.0` releases and latest commits, `release/vX.Y` for patch releases (G2b, N12/N18); an older detached install is attached by switching to its current version (G2c, N18); the Version card offers **Update to latest commit (git pull)**, Update to latest release, Switch to a tag and Fast-forward the current branch, all with the requirements reinstall and pin guard (U, N21, N22). **extra_model_paths.yaml generator:** choose a model base folder and the app writes its own `vjumpkung_desktop` section with every model folder, creates missing folders, keeps other sections (backup in `data/backups/`), and can remove its section (X, N15–N17; ComfyUI loads the paths and the library lists the files). **After session 7:** Save As for downloads started in the ComfyUI window (Windows, no test), the ComfyUI update fetch change and the Fast-forward button removal (tests only), every listed extra-model folder created (S11-NATIVE-WIN-004), `--cpu` launch (S11-NATIVE-WIN-002), exit with a foreign process in the folder (S12-NATIVE-WIN-005). | Linux desktop (ComfyUI starts on CUDA from the Linux app, xterm log and embedded window render in WebKitGTK — S7 L6, L7, container only); headless detection and whole-tree termination natively. R34; Save As tests and native check. |
| M3 — downloads | Queue, providers, pause/resume, `resource_list`, bandwidth cap incl. the HF relay. **Session 6:** every `hf` CLI download failed on Windows before (no profile directory in the child environment); the `hf` child now gets a private home (`data/cache/hf/home`) and runtime failures are logged redacted (D4). Native: live engine test and an in-app Hugging Face Xet download, 334,641,190 B, SHA-256 verified (N23, N24). **Session 7:** Google Drive downloads use the pinned `gdown` 6.4.2 tool (`tools/gdown`, hash-locked, installed by setup and Repair); the native Drive path remains only as the missing-tool fallback. Verified live on Windows (a `/file/d/<id>/view?usp=sharing` link through the confirmation page, 498,881,336 B, exact progress, pause restarts from 0, nonexistent-file and folder errors, Repair reinstalls a missing tool — S7 W1–W8) and in a Linux container (L5, identical SHA-256). **After session 7:** removing finished, failed or cancelled jobs from the list (`dl_remove`, tests only). | Provider-token scenarios (CivitAI, gated HF, Google Drive quota), native-engine pause/resume and crash recovery natively, redaction grep; R29 (Repair rebuilds the environment for a missing tool), R31 (`0600` file mode on Linux). |
| M4 — library | Scan, rename, move, delete to the Recycle Bin/trash, identify; extra roots from `extra_model_paths.yaml` (N16). Session 5: rename while ComfyUI runs with a Thai name, controlled-lock refusal and Recycle Bin (S5-NATIVE-WIN-N6-001, N9-001). **After session 7:** Git-tracked files under `models/` hidden and protected (tests only), open the models folder or reveal a model (FR-MODEL-7, tests only). | Lock held by a loaded model; non-recyclable volume cases; Linux. |
| M5 — custom nodes | UI + backend complete and verified on Windows (WP1–WP6, H1–H3). **Session 6, D1 + N (+D1b) done:** on Windows, new node content whose top-level folder has a native image (`.dll`, `.exe`, `.com`, `.cpl`, `.drv`, `.ocx`, `.scr`, `.sys`) is refused before consent — Git sources from a raw, NUL-delimited pre-checkout `ls-tree` of the exact commit, Registry archives after extraction (residual #6 closed before consent); children without an explicit working directory start in the executable's folder; uv 0.13.1's interpreter query imports nothing from the node folder (N27). An approved `install.py` now runs **before** the reviewed `requirements.txt` everywhere (Custom Nodes page: order only, its refusals stay), with pin restoration, profile-constraint restore and a frozen post-hook preview. Native: a Git install with `install.py` through the page, hook before requirements, pins unchanged (N29). **After session 7:** Disable/Enable fix (found in S10-NATIVE-WIN-003, confirmed in S11-NATIVE-WIN-003), an up-to-date update journaled as completed, migration 010 + "Keep files as they are" (S12-NATIVE-WIN-002) + Superseded recovery (tests only) + Update all stops after a blocking item (S12-NATIVE-WIN-004), CRLF normalization and a read-only Rescan (S13-NATIVE-WIN-001), a routine scan in the background (tests only), reveal the node folder (FR-NODE-12, tests only). | Scenarios 10a/c, 11a, 14, 16a, 17 natively; residual #28; R35–R41; Linux. |
| Gallery (FR-GALLERY-1..5) | Gallery over `ComfyUI/input` and `ComfyUI/output` (`c8f330c`): `gallery` preview scheme, image/video/3D previews, rename/move/copy, delete to the Recycle Bin/trash, open in the file manager; audio previews (`5503168`). Native: image, H.264 video and STL previews, copy and rename (S10-NATIVE-WIN-002). | Audio, other 3D formats, move, delete and reveal natively; WebKitGTK codecs; Linux. |
| M6 — release | Backend + UI integration complete (B, P, E1–E3, I, D2). **Session 6, N:** consented first-run setup installs every selected curated node, including content-policy findings (shipped `__pycache__`, Windows top-level native images) as acknowledged warnings; ComfyUI-PlagueKind-Nodes is selectable again and was installed and imported natively (N28). Accelerators remain experimental and unchecked for the py312 profile. **Packaging done:** `pnpm -C ui tauri build` produced unsigned NSIS (9,336,453 B) and MSI (13,340,672 B) bundles; release exe reserves an 8 MiB stack; updater inert (`s6-packaging.md`). Installers not executed. **Session 7:** unsigned NSIS (9,343,473 B) and MSI (13,336,576 B) rebuilt from `0502a5e`; Linux `.deb` (16,610,902 B) and AppImage (120,621,560 B) built in Docker; the `.deb` installs and runs and the AppImage launches in a GPU container (S7 L2, L3, L8; hashes in `s7-native-register.md`). **After session 7:** workflows re-enabled by the owner (2026-10-21) and green (HCI-WIN-001, HCI-LINUX-001); `release.yml` builds draft bundles on a version change on `main` and the owner publishes them; releases v0.1.2, v0.1.3 and v0.1.5 (plain Windows exe attached since v0.1.2; HCI-REL-001); session 10 launched a locally built release exe once (S10-NATIVE-WIN-001). | Installer run on a disposable VM; Linux bundles on a Linux desktop; Authenticode and updater key custody (owner); native accelerator run. |
| Docs | **Done (session 6):** README rewritten, docs/DOCUMENTATION.md updated with every `VERIFY` marker resolved (0 left), new [RELEASE_ACCEPTANCE.md](RELEASE_ACCEPTANCE.md) ledger, REQUIREMENTS §9 refreshed. Report: `docs/handoff/evidence/s6-docs-report.md`. **After session 7:** Ledger, status and HANDOFF refreshed 2026-11-17. | Keep the ledger in step; README "Project status", REQUIREMENTS §9 and the DOCUMENTATION check-list result still describe 2026-10-20. |

## Verification recorded on 2026-11-12 (hosted CI on 881aa90; release v0.1.5)

- Hosted CI (GitHub Actions) run 43076757035 on `881aa90`, 2026-11-12 01:20–01:31 (+07:00); every
  job succeeded (HCI-WIN-001, HCI-LINUX-001, HCI-COV-001). Ledger:
  [RELEASE_ACCEPTANCE.md](RELEASE_ACCEPTANCE.md#automated-and-packaging-evidence).
- Windows, job `check (windows-latest)` (HCI-WIN-001): every `ci.yml` step passes; Vitest **44 files /
  321 tests**; Rust **1,111 passed, 0 failed, 14 ignored** (application 294; desktop lib 105 + 1
  ignored; acl 15; bindings 1 + 1 ignored; gallery 6; navigation 2; stack_reserve 2; startup_smoke
  0 + 1 ignored; domain 82; infrastructure lib 594 + 10 ignored; real_setup 0 + 1 ignored;
  workflow 10); architecture 10/10 and `OK`; custom-protocol build ok.
- Ubuntu, job `check (ubuntu-24.04)` (HCI-LINUX-001): every step passes; Vitest 44 files / 321
  tests; Rust **1,086 passed, 0 failed, 11 ignored** (application 292, desktop 104, acl 15,
  bindings 1 + 1, gallery 6, navigation 2, domain 82, infrastructure 574 + 9, real_setup 0 + 1,
  workflow 10); architecture OK.
- Coverage, job `Core layers and coverage (no desktop libraries)` (HCI-COV-001): domain
  **95.73 %** lines (3,113 lines, 133 missed; gate 90), application **85.83 %** (7,302 lines, 1,035
  missed; gate 80).
- Release bundles (HCI-REL-001): `Release bundles` run 41341889476 on `48f3f60` (jobs `version`,
  `bundle (windows-latest)`, `bundle (ubuntu-22.04)` with the `.deb` `Depends` check) succeeded; CI
  run 41341889498 on the same commit is green. The owner published v0.1.5 on 2026-11-02 11:43; the
  five assets with sizes and SHA-256 values are in the ledger
  [Snapshot](RELEASE_ACCEPTANCE.md#snapshot). No updater JSON; `release.yml` has no Authenticode
  step; signatures not checked.
- History (HCI-HIST-001): first green CI 39717549473 (2026-10-22 10:34, `9083bc2`, branch
  `fix/ci-green`); first green on `main` 40041424227 (`712dfea`, 2026-10-24 06:45); one failure on
  `main`, 40043238278 (`a815a3e`, 2026-10-24 07:05); `0a37028` has no run and `5503168` only
  cancelled runs, both covered by green runs on `384dca8` and `48f3f60`.

## Verification recorded 2026-10-21 to 2026-11-01 (sessions 8–13)

Native rows: [Native evidence, sessions 9 to 13](RELEASE_ACCEPTANCE.md#native-evidence-sessions-9-to-13);
narrative: `git show 881aa90:HANDOFF.md`.

- Session 8 (`2ffb885`): 821 passed / 0 failed / 13 ignored, ACL 12, Vitest setup + app 5 files / 44
  tests; no native run (R43).
- Session 9 (`0000452`): infrastructure lib 471 passed / 0 failed / 10 ignored, Vitest 33 files / 236
  tests, other suites green; S9-NATIVE-WIN-001…004.
- Session 10: no code change; S10-NATIVE-WIN-001…003 (findings fixed in session 11).
- Session 11 (`33ac014`): Vitest 36 files / 275 tests; domain 82, application 251, infrastructure
  lib 522 + 10 ignored, workflow 10, desktop 76 + acl 13 + bindings 1 + gallery 6 + navigation 2 +
  stack_reserve 2; coverage domain 95.71 %, application 85.51 %; S11-NATIVE-WIN-001…005.
- Session 12 (`fb3fc5e`, version 0.1.4): check counts not recorded (Vitest needed `--maxWorkers=2`;
  one rustc ICE passed with `CARGO_INCREMENTAL=0`); S12-NATIVE-WIN-001…005.
- Session 13 (`0a37028`, `384dca8`): Vitest 38 files / 285 tests; application 269, domain 82,
  infrastructure lib 556 + 10 ignored, workflow 10, desktop as in session 11; coverage domain
  95.71 %, application 85.70 %; S13-NATIVE-WIN-001…003.
- `5503168`, `3dce71d`, `5905813`, `881aa90`: no session record; automated evidence only
  (HCI-WIN-001, HCI-LINUX-001).

## Verification recorded on 2026-10-22 (gallery, Explorer actions, Manager config — committed as `c8f330c`)

- Scope: FR-GALLERY-1..5 (Gallery page over `ComfyUI/input` and `ComfyUI/output`, `gallery` preview
  scheme, three.js 0.186.1 lazy 3D viewer), FR-MODEL-7 and FR-NODE-12 (Open folder / Show in
  Explorer), FR-SETUP-14 (create-only `ComfyUI/user/__manager/config.ini`). Explorers and the first
  coder pass ran on Codex; advisor (Codex, read-only) ruled GO with corrections, then accepted the
  staging-directory protocol (no journal or migration) and create-if-missing config. Codex hit its
  usage limit mid-run; remaining coders were Sonnet 5.5 xhigh. An Opus review of the gallery core
  found no critical issues; its 8 findings were fixed (handle-based source removal, verified
  case-only rename, staging removal order, depth limit, no folder creation for Explorer targets,
  exact-case delete pruning, Unix modes on copies, gate retained by the blocking worker).
- Windows (main, every `ci.yml` step locally): UI install, lint, typecheck ok; Vitest **36 files /
  273 tests**; UI build ok (three.js in a separate lazy chunk); `cargo fmt --check` ok; Clippy
  `-D warnings` ok; `cargo test --workspace --locked` **957 passed, 0 failed, 13 ignored**; release
  desktop build (`--features tauri/custom-protocol`) ok; architecture self-tests and check OK;
  coverage domain **92.98 %** (gate 90), application **85.45 %** (gate 80).
- Linux x64 (Docker `rust:1.99.0-trixie`, `docs/handoff/tooling/linux-check.sh` plus fmt and the
  core-coverage job): build ok, Clippy clean (after removing a Windows-only test import that was
  unused on Linux), **935 passed, 0 failed, 11 ignored**; coverage domain **95.40 %**, application
  **85.20 %**. UI steps were not repeated on Linux.
- Not yet verified natively: the app was not launched; gallery previews (WebView2/WebKitGTK video
  codecs, real glTF/OBJ/FBX/STL/PLY files), Explorer actions, Recycle Bin deletes from the gallery,
  and a real ComfyUI-Manager reading the generated `config.ini`. Later the same day, session 10
  previewed images, H.264 video and STL files and copied and renamed files natively from a release
  build of `c8f330c` (S10-NATIVE-WIN-002).

## Verification recorded on 2026-10-20, evening (session 7)

- Package G7 (Google Drive through gdown 6.4.2): explorer Haiku 5.5, advisor read-only Opus stand-in
  (Codex was rate-limited; ruling GO with nine corrections), coder Sonnet 5.5 xhigh; main re-ran
  every check. Committed and pushed as `0502a5e`.
- Windows (main, 19:03–19:06): `cargo fmt --check` ok; Clippy `-D warnings` ok;
  `cargo test --workspace --locked --no-fail-fast` **813 passed, 0 failed, 13 ignored** (+11 tests,
  +1 ignored live gdown test, which passed when run explicitly); bindings regenerated, drift 1 passed /
  1 ignored; ACL 11 passed; UI lint and typecheck ok; Vitest **30 files / 202 tests**; architecture
  self-tests 10/10 and check OK.
- Linux x64 (Docker `rust:1.99.0-trixie`): build ok, Clippy clean, **789 passed, 0 failed,
  11 ignored**; `.deb` and AppImage built (`docs/handoff/tooling/linux-bundle.sh`).
- Native: Windows W1–W10 (first run provisions `tools/gdown`, Drive download through the
  confirmation page, pause/resume from 0, error messages, Repair reinstalls a missing tool, ComfyUI on
  cuda:0); Linux L1–L8 in a GPU container (first GUI run: `.deb` install, first run, Drive download,
  ComfyUI on cuda:0 with torch 2.14.1+cu130, WebKitGTK log view and embedded window, AppImage).
  Register: `docs/handoff/evidence/s7-native-register.md`. Drivers: `docs/handoff/tooling/cdp.mjs`
  (WebView2 over CDP) and `docs/handoff/tooling/linux-gui/` (`tauri-driver` + WebKitWebDriver).
- Packaging: unsigned NSIS + MSI rebuilt (19:34); installers not executed.
- New residuals R29–R32 (RELEASE_ACCEPTANCE.md): Repair rebuilds the environment for a missing
  download tool; ComfyUI-Manager reports "PyTorch is not installed" on Linux; gdown's `0600` file
  mode on Linux; Linux graceful shutdown unverified.
- Not established: Linux desktop runs with a native NVIDIA driver, Secret Service, trash on a real
  desktop, WebKitGTK performance, installer execution, provider-token scenarios (incl. the Drive quota
  case), Authenticode signing.

## Verification recorded on 2026-10-20 (session 6, Windows, MSVC, RTX 3060)

- Each package was implemented by a coder (Claude Sonnet xhigh while Codex was rate-limited,
  then Codex gpt-6.1-sol xhigh) from an advisor-reviewed prompt,
  then re-verified by the orchestrator: `cargo fmt --check`, `cargo clippy --workspace
  --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, UI lint, typecheck,
  Vitest, bindings drift, ACL, architecture check; native checks per the register.
- Test counts (Rust passed / failed / ignored; Vitest tests): start 718/0/10, 184 → Q 719, 187 →
  S0 722 → F 731, 188 → G 742, 189 (+ native startup smoke test) → X 753, 193 → G2b 758, 194 →
  G2c 761 → D3+D5 768 → D3b 770 → U 781, 195 → W 197 → W2 200 → D4 783 / 0 / 11, 200 → D1
  (partial) 792 / 0 / 12 → N (+D1b) **802 / 0 / 12, Vitest 30 files / 202 tests**.
- **Final automated snapshot (main, 16:29–16:45, checkpoint `s6-14`):** `pnpm -C ui install
  --frozen-lockfile` ok; lint and typecheck ok; Vitest 30 files, 202 tests passed; `pnpm -C ui build`
  ok; `cargo fmt --check` ok; Clippy ok; `cargo test --workspace --locked --no-fail-fast` **802
  passed, 0 failed, 12 ignored**; architecture self-tests 10/10 and check OK; bindings 1 passed,
  1 ignored (exporter); ACL 11 passed; `cargo build -p desktop --locked --features
  tauri/custom-protocol` ok; native `startup_smoke` 1 passed. `pnpm -C ui tauri build` exit 0
  (unsigned NSIS + MSI, hashes in `s6-packaging.md`). Ignored tests: 12 definitions; executed
  explicitly in session 6: the start-up smoke test and the live Hugging Face download (N23); four
  are helper subprocesses run by their parent tests; the exporter regenerates bindings; the other
  five were not run (see RELEASE_ACCEPTANCE.md).
- Checkpoints: `s6-01-q-verified` … `s6-14-final-code-verified`. Checkpoint patches, coder
  prompts/reports and superseded plans/evidence were removed from the tree after `8dc99b1`; read
  them with `git show 8dc99b1:docs/handoff/<path>`.
- Advisor rulings: `docs/handoff/evidence/s6-advisor-rulings.md`, `s6-advisor-u.md`,
  `s6-advisor-n.md`.
- Linux x64: compiles, clippy-clean, 778 automated tests pass in a Debian container (`docs/handoff/evidence/s6-linux.md`).
- Not established: Linux desktop/GPU runs, installer execution,
  provider-token scenarios, Authenticode signing, accelerator wheels on the py312 profile,
  coverage (`cargo llvm-cov` not run on this tree), performance and accessibility measurements.

## Release gates (open)

Full list with closure evidence and owners: [RELEASE_ACCEPTANCE.md](RELEASE_ACCEPTANCE.md#release-gates).

| Gate | Needed to close |
| --- | --- |
| Linux | A Linux x64 NVIDIA desktop: S1 with a native driver, Secret Service, trash, WebKitGTK performance (container runs pass). |
| Installers | Run the hashed NSIS/MSI bundles on a disposable Windows machine/VM: install, launch, uninstall, data preservation. |
| Providers | CivitAI and gated Hugging Face tokens, the Google Drive quota case (S5, S6 gated, S7b, S12). |
| Signing | Authenticode certificate and updater key custody (owner). |
| Windows native scenarios | S01b default run, S02, S03, S04c, S06d/f, S08a, S10 (Registry consent and id@version record, dependencies, torch blocking), S11a, S13 (old-driver/cu126 case, first-run wizard on a GPU-less host), S14, S16a, S17; R37, R43; other "Native" gaps in the requirements tables, including OPEN-NATIVE-001. |
| Performance, UX | NFR-PERF timings and native startup/scan checks; offline/theme/keyboard/200 % zoom audit. |
| Residuals | #19 accelerators, #21 moved root without a valid manifest, #28 Git staging leftovers (G08); R29–R43 engineering residuals. |

Closed in session 6: security closure D1 (residual #6 before consent, N + D1b, N27), packaging
build (unsigned bundles built, `s6-packaging.md`), acceptance ledger (`RELEASE_ACCEPTANCE.md`).

Closed in session 7: Google Drive through gdown with live evidence on Windows and in a Linux
container (S07a), Linux `.deb`/AppImage build, first Linux GUI run (container).

Closed on 2026-11-12: coverage (G06, HCI-COV-001) and hosted CI (G10, HCI-WIN-001, HCI-LINUX-001).

## Verification recorded on 2026-10-20, morning (session 5)

- Startup panic (`state() called before manage()`) fixed (P0); UI integration (I); curated
  PlagueKind entry marked unavailable (D2). 718 Rust tests passed, 10 ignored; Vitest 29 files /
  184 tests; native smoke test passes. Native session-5 evidence:
  `docs/handoff/evidence/s5-native-register.md`.

## Verification recorded on 2026-10-19, evening

- Working copy = HEAD `a879fc0` + session-4 work through E3 and its review corrections
  (checkpoint `s4-11-through-e3-final-verified.patch`, kept in history at `8dc99b1`).
- `cargo test --workspace --locked --no-fail-fast`: 709 passed, 0 failed, 9 ignored; UI 28 files /
  160 tests; bindings drift and 11 ACL tests pass. Migrations 001–008.
- Test-count progress during that session: 561 → 567 → 582 → 593 → 615 → 633 → 652 → 657 → 697 → 709.
- Accumulated residuals: `docs/handoff/evidence/s3-residuals.md`.

## Verification recorded on 2026-10-19, afternoon

- Working copy = HEAD `4749d8a` + session-3 work through hardening H2: 561 Rust tests
  passed, 9 ignored; UI 28 files / 158 tests; bindings drift and 10 ACL tests pass.

## Verification recorded on 2026-10-16

- Before resumed edits: 244 deterministic Rust tests passed; Clippy with warnings
  denied and the architecture checker passed. Opt-in real-system tests are separate.
- Real first-run test: `cargo test -p infrastructure --locked --test real_setup --
  --ignored --nocapture`, with `VJ_REAL_ROOT=D:/vjc-acceptance-20261016`.
  It completed in 163 seconds with Python 3.13.16, torch 2.14.1+cu130,
  torchvision 0.29.1+cu130 and triton-windows 3.8.0.post29. Verification imported
  torch, triton and comfy and performed a CUDA tensor operation on the RTX 3060.
  Repair of the healthy environment returned `NothingToRepair`.
- Package mutation recovery was strengthened: persist `RepairRequired` before
  spawning a mutating installer; return to `Verified` only after successful
  post-install verification. Failure to save that intent prevents mutation.
  An unrelated prior repair state is retained. All 14 package workflow tests pass.

The test installation is disposable and is retained for runtime acceptance.
No ordinary test accesses real credentials or deletes user installations.

## Known evidence-backed differences

- PyTorch 2.14.1 profiles omit torchaudio because a matching artifact was unavailable
  when prepared; the bundled profiles record the registry evidence. They protect
  every installed core distribution. The recommended Python 3.12 profile includes
  torchaudio 2.10.0 (needed by e.g. ComfyUI-MelBandRoFormer and ComfyUI-WanVideoWrapper,
  which import on it — N13). Do not silently substitute an incompatible torchaudio build.
- Desktop ACL and binding tests are integration targets so their Windows executable
  manifests can be linked correctly.
- Linux desktop acceptance (native NVIDIA driver, Secret Service, trash, WebKitGTK performance)
  has not been established; the container runs (S7 L1–L8) do not replace it.
- gdown cannot tell a missing Drive file from a private one; both show the permission message.
- The CPU testing profiles exist only to test without an NVIDIA GPU: they are never recommended or
  preselected, ComfyUI starts with `--cpu`, the Linux one has never run natively (R42), and
  custom-node compatibility treats them as CUDA (R40).

## Release decisions

Updater signing is inactive until the owner supplies the public key and endpoint at
build time and retains the private key. Placeholder app icons remain in use. No
license has been selected; implementation must not add a license on the owner's behalf.
Published releases are built by `release.yml` as drafts and published by the owner; the workflow has
no Authenticode signing step and produces no updater artifacts.
