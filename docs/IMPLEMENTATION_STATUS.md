# Implementation status

This is the working completion ledger. `REQUIREMENTS.md` remains the release scope;
passing a subset of checks does not establish release completion.

Last updated: **2026-10-20, ~16:50 (+07:00)**, end of session 6. Branch `feat/m0-skeleton`; the
verified session-6 work is committed as `8dc99b1` (its code is the final checkpoint
`s6-14-final-code-verified` plus the Linux test-fixture fix in
`crates/infrastructure/src/environment/tools/tests.rs`).
Native evidence: `docs/handoff/evidence/s6-native-register.md` (N1–N29, Windows 11, NVIDIA GeForce
RTX 3060 12 GiB, driver 610.88 / CUDA 13.3). Packaging and final snapshot:
`docs/handoff/evidence/s6-packaging.md`. Acceptance ledger: [RELEASE_ACCEPTANCE.md](RELEASE_ACCEPTANCE.md).
Release verdict: **NO-GO** (see "Release gates" below).

## Milestones

| Milestone | Current evidence | Remaining work |
| --- | --- | --- |
| M0 — skeleton | Workspace, UI shell, settings, keyring, redacted logging, generated IPC, compiled ACL tests and architecture checks. **Session 6:** the shell keeps all scrolling inside the main area (no second document scrollbar, sidebar full height — N3); the active row in the Models and Custom Nodes tables is a muted row with an accent bar instead of a pink fill with unreadable text (N4); a debug build no longer overflows the 1 MiB Windows main-thread stack when custom nodes are installed (8 MiB main-thread reserve and 8 MiB tokio worker stacks, PE and worker tests — N1, N6, N7). | Linux acceptance. |
| M1 — setup | Hardware/profile detection, managed/custom Python, pinned tools, hash-locked hf, provisioning journal, activation recovery, pin guard. **Session 6, verified natively on a fresh root (N8–N13):** nothing is created inside `ComfyUI/` before setup clones it, and the app restarts once after first-time setup; the recommended Windows profile is now **Python 3.12.15 + PyTorch 2.10.0 cu130 with torchaudio 2.10.0** (listed first, preselected); the first-run wizard ticks ComfyUI-Manager by default and offers the curated nodes unticked; the review step discloses the third-party sources and **Start setup** is the confirmation, so setup never pauses for consent; curated nodes install the **latest commit of each repository's default branch** (no commit lock) and track that branch; ComfyUI source choice offers Latest release (default), **Latest commit (master)** and a specific tag. Python 3.12 children now receive `USERNAME` (Python 3.12's `getpass` fallback to `pwd` crashed `torch._dynamo`/ComfyUI before — N5). A full first run with ComfyUI-Manager + 21 curated nodes finished in ~8.5 min with no pause; all nodes imported on cuda:0 (N11, N13). | Linux; custom-venv/adopted and change-torch scenarios natively; a first run with exactly the final defaults (Manager only). The final-code binary ran a first run with Manager + PlagueKind (N28), and the final defaults were checked in the wizard (N25, N26). |
| M1 — moved install (FR-SETUP-12) | **Verified natively on a `robocopy /E` copy (N19, N20):** the copy is detected as moved, Rebuild removes uv's copied minor-version folders for every managed Python (D3/D3b) and succeeds; managed Python is no longer registered under `HKCU\Software\Python\Astral` (`--no-registry`, D5); ComfyUI then runs with all nodes on cuda:0. | Linux symlink branch; "original root absent" variant. |
| M2 — runtime | Start/stop/restart under the maintenance lease, logs, embedded window, close policy, update. **Session 6:** ComfyUI stays on a git branch that tracks `origin`, so `git pull` works — `master` for `.0` releases and latest commits, `release/vX.Y` for patch releases (G2b, N12/N18); an older detached install is attached by switching to its current version (G2c, N18); the Version card offers **Update to latest commit (git pull)**, Update to latest release, Switch to a tag and Fast-forward the current branch, all with the requirements reinstall and pin guard (U, N21, N22). **extra_model_paths.yaml generator:** choose a model base folder and the app writes its own `vjumpkung_desktop` section with every model folder, creates missing folders, keeps other sections (backup in `data/backups/`), and can remove its section (X, N15–N17; ComfyUI loads the paths and the library lists the files). | Linux; headless detection and whole-tree termination natively. |
| M3 — downloads | Queue, providers, pause/resume, `resource_list`, bandwidth cap incl. the HF relay. **Session 6:** every `hf` CLI download failed on Windows before (no profile directory in the child environment); the `hf` child now gets a private home (`data/cache/hf/home`) and runtime failures are logged redacted (D4). Native: live engine test and an in-app Hugging Face Xet download, 334,641,190 B, SHA-256 verified (N23, N24). **Session 7:** Google Drive downloads use the pinned `gdown` 6.4.2 tool (`tools/gdown`, hash-locked, installed by setup and Repair); the native Drive path remains only as the missing-tool fallback; automated tests only, no live run yet (S07). | Provider-token scenarios (CivitAI, gated HF, Google Drive quota), pause/resume and crash recovery natively, redaction grep. |
| M4 — library | Scan, rename, move, delete to the Recycle Bin/trash, identify; extra roots from `extra_model_paths.yaml` (N16). Session 5: rename while ComfyUI runs with a Thai name, controlled-lock refusal and Recycle Bin (S5-NATIVE-WIN-N6-001, N9-001). | Lock held by a loaded model; non-recyclable volume cases; Linux. |
| M5 — custom nodes | UI + backend complete and verified on Windows (WP1–WP6, H1–H3). **Session 6, D1 + N (+D1b) done:** on Windows, new node content whose top-level folder has a native image (`.dll`, `.exe`, `.com`, `.cpl`, `.drv`, `.ocx`, `.scr`, `.sys`) is refused before consent — Git sources from a raw, NUL-delimited pre-checkout `ls-tree` of the exact commit, Registry archives after extraction (residual #6 closed before consent); children without an explicit working directory start in the executable's folder; uv 0.13.1's interpreter query imports nothing from the node folder (N27). An approved `install.py` now runs **before** the reviewed `requirements.txt` everywhere (Custom Nodes page: order only, its refusals stay), with pin restoration, profile-constraint restore and a frozen post-hook preview. Native: a Git install with `install.py` through the page, hook before requirements, pins unchanged (N29). | Scenarios 10, 11, 14, 16a, 17 natively; residual #28; Linux. |
| M6 — release | Backend + UI integration complete (B, P, E1–E3, I, D2). **Session 6, N:** consented first-run setup installs every selected curated node, including content-policy findings (shipped `__pycache__`, Windows top-level native images) as acknowledged warnings; ComfyUI-PlagueKind-Nodes is selectable again and was installed and imported natively (N28). Accelerators remain experimental and unchecked for the py312 profile. **Packaging done:** `pnpm -C ui tauri build` produced unsigned NSIS (9,336,453 B) and MSI (13,340,672 B) bundles; release exe reserves an 8 MiB stack; updater inert (`s6-packaging.md`). Installers not executed. | Installer run on a disposable VM; Linux bundles; Authenticode and updater key custody (owner); native accelerator run. |
| Docs | **Done (session 6):** README rewritten, docs/DOCUMENTATION.md updated with every `VERIFY` marker resolved (0 left), new [RELEASE_ACCEPTANCE.md](RELEASE_ACCEPTANCE.md) ledger, REQUIREMENTS §9 refreshed. Report: `docs/handoff/evidence/s6-docs-report.md`. | Keep the ledger in step with new evidence. |

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
- Linux x64: compiles, clippy-clean, 789 automated tests pass in a Debian container; the `.deb` and AppImage build
  and run in a GPU container (first run, ComfyUI on CUDA, Google Drive download — `docs/handoff/evidence/s7-native-register.md`).
- Not established: Linux desktop runs with a native NVIDIA driver, installer execution,
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
| Windows native scenarios | S2, S3, S4 original-absent, S6 pause/fallback, S8 loaded-model lock, S10, S11, S13, S14, S16a, S17; one first run with the final defaults. |
| Coverage, performance, UX | `cargo llvm-cov`; NFR-PERF timings; offline/theme/keyboard/200 % zoom audit. |
| Residuals | #19 accelerators, #21 moved root without a valid manifest, #28 Git staging leftovers. |

Closed in session 6: security closure D1 (residual #6 before consent, N + D1b, N27), packaging
build (unsigned bundles built, `s6-packaging.md`), acceptance ledger (`RELEASE_ACCEPTANCE.md`).

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
- Linux GPU, Secret Service, trash and WebKitGTK acceptance have not been established
  by Windows test results.

## Release decisions

Updater signing is inactive until the owner supplies the public key and endpoint at
build time and retains the private key. Placeholder app icons remain in use. No
license has been selected; implementation must not add a license on the owner's behalf.
