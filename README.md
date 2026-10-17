# vjumpkung-comfyui-desktop

A native desktop application that installs, runs and maintains a local [ComfyUI](https://github.com/comfyanonymous/ComfyUI). It manages the Python environment and PyTorch build, downloads and organises model files, and installs custom nodes.

The application is a Rust program built on **Tauri 2**. Its whole interface is a web frontend (Vite, React, TypeScript) rendered by the operating system webview: WebView2 on Windows, WebKitGTK on Linux. It does **not** use Electron.

> **Status: work in progress, not release-ready.** See [Project status](#project-status). Native NVIDIA and Linux acceptance testing has not been completed, and nothing in this repository should be read as a statement that a release has been validated.

## Contents

- [Key features](#key-features)
- [Release 1 scope and requirements](#release-1-scope-and-requirements)
- [Project status](#project-status)
- [Quick start for users](#quick-start-for-users)
- [Quick start for developers](#quick-start-for-developers)
- [Repository layout](#repository-layout)
- [Documentation](#documentation)
- [Security notes](#security-notes)
- [Application updates and signing](#application-updates-and-signing)
- [License](#license)

## Key features

| Area | What the application does |
|---|---|
| First-run setup | A wizard checks the NVIDIA hardware, lets you choose a Python source and a tested setup profile, then builds the environment with [uv](https://github.com/astral-sh/uv) and installs ComfyUI. Failed steps can be retried, skipped (optional steps only) or aborted, and a failed setup leaves the previous working environment active. |
| Re-setup | Repair, change torch/CUDA, change Python, or switch to a custom venv. The new environment is built alongside the old one and activated only after verification. |
| Environment | Shows the active environment and lets you list, install, upgrade and uninstall pip packages. A pin guard keeps `torch`, `torchvision`, `torchaudio` and Triton at the installed versions. |
| ComfyUI runtime | Start, stop and restart ComfyUI as a child process owned by the app. Live redacted logs, launch options, port-conflict handling, an update and version picker (fast-forward only, never a reset), and an embedded ComfyUI window or the system browser. |
| Model downloads | A persistent queue for CivitAI, Hugging Face, Google Drive and direct HTTPS links with progress, pause, resume, retry, de-duplication, name-conflict handling, SHA-256 verification where the provider supplies a hash, and `resource_list` import and export. |
| Model library | Scans `ComfyUI/models` and the paths in `extra_model_paths.yaml`. Search, filter, rename, move, delete (to the Recycle Bin or Trash) and identify models on CivitAI. |
| Custom nodes | Browse the Comfy Registry or install from any HTTPS Git URL, with an exact-source review and consent step before any third-party code runs. Update, enable, disable and uninstall, with recovery after interrupted operations. |
| Settings | Download concurrency, Hugging Face engine (`hf` CLI or native), close behaviour, theme, registry cache lifetime, and CivitAI and Hugging Face tokens held in the OS keyring. |

## Release 1 scope and requirements

| Item | Release 1 |
|---|---|
| Operating systems | Windows 10/11 x64 and Linux x64 |
| GPU | NVIDIA only. AMD, Intel, macOS and CPU-only torch builds are out of scope. |
| Webview runtime | Windows: WebView2 (the Windows bundle can install it through a silent bootstrapper). Linux: `libwebkit2gtk-4.1`. |
| Git | Windows: a pinned MinGit is downloaded by the app. Linux: system `git` 2.40 or newer, which you must install yourself. |
| Defaults | Python 3.13, PyTorch 2.14.1 with CUDA 13.0 (`cu130`). A CUDA 12.6 profile exists for older drivers, and a Python 3.12 / PyTorch 2.10 profile exists on Windows for compatibility. |
| Driver and GPU | The wizard disables any profile that your driver or GPU architecture cannot run, and shows the reason. All bundled profiles require compute capability 8.0 or newer. |
| Disk | The default Windows profile budgets 16 GiB (5 GiB environment, 7 GiB other, 4 GiB margin). The wizard checks free space for the profile you pick and blocks the install if it is too low. |
| Interface language | English only |

Not in scope for release 1: more than one ComfyUI install at a time, a hosted or multi-user service, remote access to the manager, and localisation.

## Project status

The authoritative ledgers are [docs/IMPLEMENTATION_STATUS.md](docs/IMPLEMENTATION_STATUS.md) and [HANDOFF.md](HANDOFF.md). Treat the milestone table below as a summary of those files as of 2026-10-17, not as a release statement.

| Milestone | Scope | State |
|---|---|---|
| M0 Skeleton | Workspace, UI shell, settings, keyring, redacted logging, generated IPC, ACL tests, architecture checks | Implemented. |
| M1 Setup | Hardware and profile detection, Python sources, pinned tools, pin guard, re-setup | Implemented. A Windows first-run setup and a repair run passed on an RTX 3060 on 2026-10-16. Linux is untested. |
| M2 Runtime | Start, stop, restart, logs, embedded window, close behaviour, update | Implemented in the working tree. Acceptance is open. |
| M3 Downloads | Queue, providers, pause and resume, `resource_list` | Implemented in the working tree. Acceptance is open. |
| M4 Library | Scan, rename, move, delete, identify | Implemented in the working tree. Acceptance is open. |
| M5 Custom nodes | Registry, Git install, consent, update, enable and disable, recovery | In progress. The UI and IPC contract are largely written. Backend hardening work packages are planned but not complete. |
| M6 Release 1 | Optional accelerators, curated nodes, moved-install rebuild, bandwidth cap, signed updater, packaging | In progress. Bandwidth limiting is partly written. The rest is planned. |

What is **not** established:

- No end-to-end acceptance on a Linux machine, including Secret Service, trash and WebKitGTK behaviour.
- Windows setup has been exercised on one NVIDIA machine only. The development machine used for most work has no NVIDIA GPU, so CUDA, accelerator and acceptance evidence is limited to the 2026-10-16 run above.
- The Windows installers (NSIS and MSI) build locally but have not been run. Linux bundles (AppImage and deb) have not been built.
- Accelerator wheel compatibility, WebKitGTK suitability for the ComfyUI canvas, Thai font rendering on Linux, Windows code signing and updater key custody remain open (see [REQUIREMENTS section 9](docs/REQUIREMENTS.md)).
- Placeholder application icons are still in use.

A release-acceptance ledger, `docs/RELEASE_ACCEPTANCE.md`, is planned. It will map every P0 and P1 requirement and all 17 acceptance scenarios to evidence or to an explicit open blocker. It does not exist yet.

## Quick start for users

There is no published release yet. When bundles are available:

1. Install the bundle for your OS.
   - Windows: run the NSIS setup (`vjumpkung-comfyui-desktop_<version>_x64-setup.exe`, a per-user install) or the MSI.
   - Linux: run the AppImage, or install the `.deb`. Install `git` (2.40 or newer) and `libwebkit2gtk-4.1` first if they are missing.
2. Start the application. On first launch it opens the **setup wizard**: install root, hardware check, Python source, profile, options, review. Setup downloads several GB of packages, so it needs a network connection.
3. When setup finishes, open the **ComfyUI** page and press Start. Use **Downloads** and **Models** to add and manage models.

Full instructions are in the [user guide](docs/DOCUMENTATION.md#part-1-user-guide).

## Quick start for developers

### Prerequisites

| Tool | Requirement |
|---|---|
| Rust | The toolchain in [rust-toolchain.toml](rust-toolchain.toml) (currently `1.99.0` with rustfmt and clippy). `rustup` installs it automatically on first use. |
| Windows build tools | MSVC: Visual Studio 2022 with the **Desktop development with C++** workload (component `Microsoft.VisualStudio.Component.VC.Tools.x86.x64`) and a Windows 10/11 SDK. A GNU fallback exists (see the developer guide). |
| Node.js and pnpm | pnpm `10.24.0` (pinned by `packageManager` in [ui/package.json](ui/package.json), usable through Corepack). No Node version is pinned in the repository. The disabled CI workflows name Node 24. |
| Webview | Windows: WebView2 runtime. Linux: `libwebkit2gtk-4.1-dev`, `libgtk-3-dev` and the other packages listed in the developer guide. |
| Git | Required to run the tests that exercise real Git repositories. |

### Common commands

Run everything from the repository root.

```bash
pnpm -C ui install --frozen-lockfile     # install frontend dependencies
pnpm -C ui tauri dev                     # desktop app with hot reload
pnpm -C ui dev                           # frontend only (no Tauri IPC)
pnpm -C ui build                         # production frontend assets
pnpm -C ui tauri build                   # release bundles (NSIS + MSI on Windows)
```

### Full check list

Run these before reporting work as done. CI workflows are disabled by the owner, so the checks run locally.

```bash
pnpm -C ui install --frozen-lockfile && pnpm -C ui lint && pnpm -C ui typecheck && pnpm -C ui test && pnpm -C ui build
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -j 4 -- -D warnings
cargo test --workspace --locked -j 4 --no-fail-fast
node --test scripts/check-arch-deps.test.mjs && node scripts/check-arch-deps.mjs
cargo test -p desktop --test bindings --locked
cargo build -p desktop --locked --features tauri/custom-protocol
pnpm -C ui tauri build                   # Windows NSIS + MSI bundles (about 12 minutes)
```

Notes:

- `cargo build -p desktop --features tauri/custom-protocol` needs `ui/dist`, so run `pnpm -C ui build` first.
- The IPC bindings in `ui/src/shared/ipc/bindings.ts` are generated. Never edit them by hand. Regenerate them with `cargo test -p desktop --test bindings --locked -- --ignored export_bindings`.
- Coverage targets are domain at least 90% and application at least 80% of lines, measured with `cargo llvm-cov`.

The [developer guide](docs/DOCUMENTATION.md#part-2-developer-guide) covers architecture, adding IPC commands, migrations, setup profiles, download providers, opt-in real-system tests and packaging.

## Repository layout

```text
crates/
  domain/           business types and invariants (no I/O, no serde)
  application/      use cases and port traits
  infrastructure/   adapters: SQLite and migrations, subprocesses, HTTP, filesystem, keyring, trash
  desktop/          Tauri composition root, IPC commands and DTOs, capabilities, integration tests, icons
ui/src/
  app/              application shell and routing
  features/         one folder per feature (setup, environment, comfy-runtime, downloads, library, custom-nodes, settings, overview)
  shared/           shared UI components and the typed IPC gateway
docs/               requirements, design, ADRs, status ledgers and handoff plans
scripts/            Tauri CLI wrapper and the architecture dependency check
```

Dependencies point inward: `domain` <- `application` <- `infrastructure` <- `desktop`. `node scripts/check-arch-deps.mjs` enforces the allowed edges.

## Documentation

| Document | Contents |
|---|---|
| [docs/DOCUMENTATION.md](docs/DOCUMENTATION.md) | Full manual: user guide and developer guide |
| [docs/REQUIREMENTS.md](docs/REQUIREMENTS.md) | Normative requirements, architecture rules, acceptance scenarios and open items |
| [docs/DESIGN.md](docs/DESIGN.md) | Visual and interaction design system for the manager UI |
| [docs/adr/0001-ui-technology.md](docs/adr/0001-ui-technology.md) | Decision: Vite and React inside the Tauri webview |
| [docs/adr/0002-clean-architecture-ddd.md](docs/adr/0002-clean-architecture-ddd.md) | Decision: Clean Architecture and DDD in Rust |
| [docs/IMPLEMENTATION_STATUS.md](docs/IMPLEMENTATION_STATUS.md) | Working completion ledger |
| [HANDOFF.md](HANDOFF.md) | Current engineering state, decisions and next steps |
| `docs/RELEASE_ACCEPTANCE.md` | Planned release evidence ledger (not yet written) |
| [AGENTS.md](AGENTS.md) | Contributor guidelines |

## Security notes

- CivitAI and Hugging Face tokens are stored in the OS keyring (Windows Credential Manager, Linux Secret Service). If the keyring is unavailable they live in memory for that session only. Tokens are never written to SQLite, logs or URLs.
- The ComfyUI window has **no** Tauri capabilities, so web content served by ComfyUI cannot call the application's commands. Only the manager window can.
- Installing a custom node runs third-party code, and a Python venv is not a sandbox. The application shows the exact source and asks for consent first.
- ComfyUI listens on `127.0.0.1` by default. Listening on the LAN is opt-in and requires acknowledgement, because ComfyUI has no login.
- Credentials are attached only to the exact provider origin and stripped on redirects to other origins. Logs redact tokens and signed URL queries.

## Application updates and signing

<!-- VERIFY: M6 plan section P (m6-approved-plan.md): the updater commands app_update_status/app_update_check/app_update_install and the "inactive without both variables" behaviour are not yet implemented or verified -->
Self-update uses the Tauri updater and is **inactive** in any build that does not receive both `VJUMP_UPDATER_PUBKEY` and `VJUMP_UPDATER_ENDPOINT` at build time. Without them the update check is not available. Signing keys are an owner responsibility: the private key (`TAURI_SIGNING_PRIVATE_KEY`) is never stored in the repository, and a generated development key must not be used for a release. Windows bundles are currently unsigned.

## License

**No license has been selected for this project yet.** Until the owner chooses one, no license is granted, and no `LICENSE` file is present. Third-party components keep their own licenses.
