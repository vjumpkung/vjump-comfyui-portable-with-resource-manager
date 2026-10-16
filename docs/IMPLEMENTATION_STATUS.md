# Implementation status

This is the working completion ledger. `REQUIREMENTS.md` remains the release scope;
passing a subset of checks does not establish release completion.

## Milestones

| Milestone | Current evidence | Remaining work |
| --- | --- | --- |
| M0 — skeleton | Workspace, UI shell, settings, keyring, redacted logging, generated IPC, compiled ACL tests and architecture checks are implemented. | Repeat platform acceptance with the complete application. |
| M1 — setup | Hardware/profile detection, managed/custom Python, pinned tools, hash-locked hf, provisioning journal, activation recovery, package plans and pin guard exist. Windows default setup passed on 2026-10-16. | Finish wizard and Environment UI, verify interaction and custom/re-setup acceptance. Linux acceptance remains open. |
| M2 — runtime | Approved plan; core implementation in progress. | Integrate runtime core, desktop/webview controls, logs, update/recovery UI and acceptance. |
| M3 — downloads | Approved plan; core implementation in progress. | Integrate providers, durable scheduler/publication, controls, import/export and acceptance. |
| M4 — library | Approved plan with recovery, path coordination and trash corrections. | Implementation and acceptance. |
| M5 — custom nodes | Approved plan with exact-source consent and dependency recovery. | Implementation and acceptance. |
| M6 — release | Approved plan with pinned accelerators, global cap, moved-root rebuild, optional signed updater and packaging. | Implementation and release checks. |
| README | Required after implementation. | Document actual behavior, installation, operation, development and limitations. |

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
