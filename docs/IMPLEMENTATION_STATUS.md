# Implementation status

This is the working completion ledger. `REQUIREMENTS.md` remains the release scope;
passing a subset of checks does not establish release completion.

## Milestones

| Milestone | Current evidence | Remaining work |
| --- | --- | --- |
| M0 — skeleton | Workspace, UI shell, settings, keyring, redacted logging, generated IPC, compiled ACL tests and architecture checks are implemented. | Repeat platform acceptance with the complete application. |
| M1 — setup | Hardware/profile detection, managed/custom Python, pinned tools, hash-locked hf, provisioning journal, activation recovery, package plans and pin guard exist. Windows default setup passed on 2026-10-16 through the direct `real_setup` test. **Fixed 2026-10-19:** a clean first-run setup started from the app always failed at `clone_comfyui` ("The ComfyUI folder exists but is not a git checkout"), because startup creates `ComfyUI/models` and `ComfyUI/custom_nodes/.disabled/.vjump-no-hooks` before setup. A directory-only scaffold is now treated as absent and the clone is merged into it in place, with a recovery record so an interrupted publication can be retried. | Repeat scenario 1 from the running app (the 2026-10-16 evidence bypassed app startup); custom/re-setup acceptance; Linux acceptance. |
| M2 — runtime | Implemented: start/stop/restart under the maintenance lease with node reconciliation (WP6), logs, embedded window, update. Close policy (H3): closing while maintenance runs is refused with a message; Stop-and-exit tolerates only a node-rescan failure after a verified stop; an exit lease is held until the process exits. | Native acceptance (headless detection, whole-tree termination, embedded-window capability denial). |
| M3 — downloads | Implemented: queue, providers, pause/resume, `resource_list`, shared bandwidth cap incl. the HF relay (package B). | Real provider acceptance (CivitAI, HF gated/LFS/Xet, Drive), throughput and redaction checks. |
| M4 — library | Implemented: scan, rename, move, delete to the Recycle Bin/trash, identify. **Fixed 2026-10-19:** moving a model to the Windows Recycle Bin always failed because verbatim `\\?\` paths were passed to the shell API. Recycling now requires a positive per-item completion and never falls back to permanent deletion. | Native locked-file acceptance; non-recyclable volume, disabled Recycle Bin and oversized-item cases. |
| M5 — custom nodes | **Implemented and verified on Windows:** UI + contract (C0/FE), launch lifecycle (WP6), pinned containment (WP1), Git content identity / staged updates / refusal status and protected-package refusal / registry warnings (WP2+WP3), hardening H1 (bounded pinned traversal), H2 (lease/pin retention, BOOT/stdin integrity, Git configuration safety, editable-requirement refusal) and H3 (rejecting Recycle Bin sink, close policy), WP4 (final content digest, identity-based recovery, launch blocking only for ambiguous recovery) and WP5 (exact node requirements restored during Repair/replacement; any node requirement failure prevents activation; dependency state per environment and content). An oversized or non-UTF-8 `requirements.txt` is now refused instead of silently skipped. | Native Linux runs of the Linux-only code paths; acceptance scenarios 10, 11, 14, 16, 17 on real installs. |
| M6 — release | **Backend implemented and verified on Windows:** bandwidth cap (B), signed updater + redacted diagnostics (P), hashed accelerator wheels with pin safety and stored outcomes (E1), curated setup nodes installed through the M5 review before activation (E2, 22-entry catalog, damage can never be skipped), missing-root recovery and guided rebuild (E3: missing root reported before any directory is created; a moved root detected before startup reconciliation and restricted to setup/recovery; open-existing / new-install with a controlled restart; rebuild into a new app-owned environment from the exact recorded profile revision; migration 008 relocation rebasing that never touches the original root). E2 and E3 passed a review with corrections, all applied. | UI integration (I: accelerator and curated selection in the wizard, batch curated review, missing-root/rebuild screens, accelerator results on the Environment page — prompt drafted, not run); release audit and packaging (R). No native NVIDIA evidence for accelerators, curated node dependencies or a moved-install rebuild (scenario 4). |
| README | Session-2 draft with verification markers. | Final pass after R from verified behaviour and the acceptance ledger. |

## Verification recorded on 2026-10-19, evening (Windows, MSVC, no NVIDIA GPU)

- Working copy = HEAD `a879fc0` + uncommitted session-4 work through E3 and its review
  corrections (checkpoint `docs/handoff/patches/s4-11-through-e3-final-verified.patch`).
  Each package was re-verified by the orchestrator after its coder finished.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`,
  `node scripts/check-arch-deps.mjs` (and its 10 self-tests): pass.
- `cargo test --workspace --locked --no-fail-fast`: **709 passed, 0 failed, 9 ignored**
  (opt-in real-system tests and the binding export helper). Git tests executed with git 2.55;
  native Recycle Bin tests ran on a fixed NTFS volume.
- UI: lint and typecheck pass; 28 files / 160 tests pass; bindings drift and 11 ACL tests pass.
- Migrations: 001–008 (007 environment components, 008 install relocation).
- Progress of the test count during the session: 561 (start) → 567 (first-run fix) →
  582 (H3) → 593 (WP4) → 615 (WP5) → 633 (E1 merge) → 652 (E2) → 657 (E2 corrections) →
  697 (E3) → 709 (E3 corrections).
- Two runtime tests were intermittently failing under parallel load (machine-wide process scan
  and a test port race); the tests were isolated without changing production behaviour, and the
  infrastructure suite was then clean in repeated and loaded runs.
- Not established by these runs: Linux compilation of the Linux-only code paths, NVIDIA/CUDA
  behaviour (accelerator wheels, curated node dependencies), a first-run setup driven from the
  running app, real WebView2 rendering, native recycle edge cases, packaging and installers.
  Accumulated residuals: `docs/handoff/evidence/s3-residuals.md`.

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
  every installed core distribution. The Python 3.12 compatibility profile includes
  torchaudio 2.10.0. Do not silently substitute an incompatible torchaudio build.
- Desktop ACL and binding tests are integration targets so their Windows executable
  manifests can be linked correctly.
- Linux GPU, Secret Service, trash and WebKitGTK acceptance have not been established
  by Windows test results.

## Release decisions

Updater signing is inactive until the owner supplies the public key and endpoint at
build time and retains the private key. Placeholder app icons remain in use. No
license has been selected; implementation must not add a license on the owner's behalf.
