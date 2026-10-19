# Implementation status

This is the working completion ledger. `REQUIREMENTS.md` remains the release scope;
passing a subset of checks does not establish release completion.

## Milestones

| Milestone | Current evidence | Remaining work |
| --- | --- | --- |
| M0 — skeleton | Workspace, UI shell, settings, keyring, redacted logging, generated IPC, compiled ACL tests and architecture checks are implemented. | Repeat platform acceptance with the complete application. |
| M1 — setup | Hardware/profile detection, managed/custom Python, pinned tools, hash-locked hf, provisioning journal, activation recovery, package plans and pin guard exist. Windows default setup passed on 2026-10-16. | Custom/re-setup acceptance; Linux acceptance remains open. |
| M2 — runtime | Implemented: start/stop/restart under the maintenance lease with node reconciliation (WP6), logs, embedded window, close behaviour, update. | Native acceptance (headless detection, whole-tree termination, embedded-window capability denial). |
| M3 — downloads | Implemented: queue, providers, pause/resume, `resource_list`, shared bandwidth cap incl. the HF relay (package B). | Real provider acceptance (CivitAI, HF gated/LFS/Xet, Drive), throughput and redaction checks. |
| M4 — library | Implemented: scan, rename, move, delete to trash, identify. | Native locked-file and trash acceptance. |
| M5 — custom nodes | Implemented and verified on Windows: UI + contract (C0/FE), launch lifecycle (WP6), pinned containment (WP1), Git content identity / staged updates / refusal status and protected-package refusal / registry warnings (WP2+WP3), hardening H1 (bounded pinned traversal) and H2 (lease/pin retention, BOOT/stdin integrity, Git configuration safety, editable-requirement refusal). | H3 (Windows recycle sink, close policy), WP4 (final digest, identity-based recovery), WP5 (exact node requirements during replacement), native Linux runs, scenarios 10/11/14/16/17. |
| M6 — release | Implemented and merged: bandwidth cap (B), signed updater + redacted diagnostics (P). E1 accelerators verified as a separate patch, not yet merged. | Merge E1; curated setup nodes (E2); missing-root recovery and guided rebuild (E3); UI integration (I); release audit and packaging (R). |
| README | Session-2 draft with verification markers. | Final pass after R from verified behaviour and the acceptance ledger. |

## Verification recorded on 2026-10-19 (Windows, MSVC, no NVIDIA GPU)

- Working copy = HEAD `4749d8a` + uncommitted session-3 work through hardening H2.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`,
  `node scripts/check-arch-deps.mjs`: pass.
- `cargo test --workspace --locked --no-fail-fast`: 561 passed, 0 failed, 9 ignored (opt-in
  real-system tests and a helper). Git tests executed with git 2.55; the guarded-requirements
  test ran with the pinned uv 0.13.1.
- UI (after the updater/diagnostics merge): lint and typecheck pass; 28 files / 158 tests pass;
  bindings drift and 10 ACL tests pass.
- Not established by these runs: Linux compilation of the Linux-only code paths, NVIDIA/CUDA
  behaviour, native recycle edge cases, packaging and installers.

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
