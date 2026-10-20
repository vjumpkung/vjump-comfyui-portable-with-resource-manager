# Session 6 — packaging build and final automated snapshot (2026-10-20, 16:29 +07:00)

Machine: Windows 11 Education 10.0.26300 x64, NVIDIA GeForce RTX 3060 12 GiB (driver 610.88, CUDA 13.3), 32 GB RAM,
MSVC toolchain `1.99.0-x86_64-pc-windows-msvc` (VS Build Tools 2022), Node 24.13.0, pnpm 10.24.0, git 2.55.0.windows.3.
Source: branch `feat/m0-skeleton`, HEAD `e384d43` + uncommitted verified session-6 work = checkpoint
`docs/handoff/patches/s6-14-final-code-verified.patch`.

## Packaging (`pnpm -C ui tauri build`, release profile)
| Artifact | Size | SHA-256 | Authenticode |
|---|---|---|---|
| `target/release/bundle/nsis/vjumpkung-comfyui-desktop_0.1.0_x64-setup.exe` | 9,336,453 B (8.90 MiB) | `d2f328a438bf3b83edf5dab09c4f7f35305d5ab0353235770c86c16e4c800308` | NotSigned |
| `target/release/bundle/msi/vjumpkung-comfyui-desktop_0.1.0_x64_en-US.msi` | 13,340,672 B (12.72 MiB) | `51005095abcbfe20f5842591cfb50a383e1cea507a7523cd9e80cc96ea6da299` | NotSigned |
| `target/release/vjumpkung-comfyui-desktop.exe` | — | `c49e79323fe6fe9df46e5b41a0648d595304c17aef5b7c552320be011dc3ab56` | NotSigned |
- Build exit 0. The release exe's PE `SizeOfStackReserve` = 8,388,608 (S0 applies to release builds).
- The updater is inert (no `VJUMP_UPDATER_PUBKEY` / `VJUMP_UPDATER_ENDPOINT` at build time).
- Installers were NOT executed and the release exe was NOT launched on this machine (the release build has no debug
  root override and the bootstrap pointer targets the owner's `D:\comfyui-test\v1`). Installer acceptance needs a
  disposable machine/VM. Placeholder icons; no license.

## Final automated check list (16:29–16:45)
| Check | Result |
|---|---|
| `pnpm -C ui install --frozen-lockfile` | ok |
| `pnpm -C ui lint` / `typecheck` | ok / ok |
| `VITEST_MAX_WORKERS=2 pnpm -C ui test` | 30 files, 202 tests passed |
| `pnpm -C ui build` | ok |
| `cargo fmt --all -- --check` | ok |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | ok |
| `cargo test --workspace --locked --no-fail-fast` | **802 passed, 0 failed, 12 ignored** |
| `node --test scripts/check-arch-deps.test.mjs` / `node scripts/check-arch-deps.mjs` | 10/10 pass / OK |
| `cargo test -p desktop --test bindings --locked` | 1 passed, 1 ignored (exporter) |
| `cargo test -p desktop --test acl --locked` | 11 passed |
| `cargo build -p desktop --locked --features tauri/custom-protocol` | ok |
| native `startup_smoke::fresh_root_starts_and_closes_cleanly` (ignored, run explicitly) | 1 passed |

## Ignored tests (12 definitions)
| Test file | Purpose | Session 6 |
|---|---|---|
| `crates/desktop/tests/bindings.rs` | exporter: regenerates `bindings.ts` | run by coders to regenerate; drift test passes |
| `crates/desktop/tests/startup_smoke.rs` | native start/close of a fresh root | **executed, pass** (G check + final list) |
| `crates/infrastructure/src/comfy_runtime/process/tests.rs` (×2) | subprocess fixtures for the bridge ownership test | executed indirectly by that test |
| `crates/infrastructure/src/shared/process/tests.rs` (×2) | helpers for `children_without_cwd_start_in_the_executable_directory` and `windows_dll_initializer_is_process_local` | executed indirectly |
| `crates/infrastructure/src/models/acquisition/hf_engine.rs` | live `hf` download without profile variables | **executed, pass** (N23) |
| `crates/infrastructure/src/models/acquisition/hf_proxy/tests.rs` | live Xet download with denied direct egress | not executed (needs external egress control) |
| `crates/infrastructure/src/environment/python/tests.rs` | needs `VJ_TEST_PYTHON` | not executed |
| `crates/infrastructure/src/environment/tools/tests.rs` | downloads uv, MinGit, Python | not executed (tools step exercised by N11/N19/N20/N28) |
| `crates/infrastructure/src/settings/secrets/tests.rs` | real OS credential store | not executed |
| `crates/infrastructure/tests/real_setup.rs` | direct real setup (bypasses app startup) | not executed (superseded by app-driven first runs N11/N28) |
Not ignored but environment-dependent: `node_preview_never_imports_python_modules_from_node_directory` (skips without uv) —
ran with the pinned uv 0.13.1 (N27, and in the N coder run).
