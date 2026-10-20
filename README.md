# vjumpkung-comfyui-desktop

A native desktop application that installs, runs and maintains a local [ComfyUI](https://github.com/comfyanonymous/ComfyUI). It manages the Python environment and PyTorch build, downloads and organises model files, and installs custom nodes.

The application is a Rust program built on **Tauri 2**. Its whole interface is a web frontend (Vite, React, TypeScript) rendered by the operating system webview: WebView2 on Windows, WebKitGTK on Linux. It does **not** use Electron.

> **Status: not release-ready. Release verdict: NO-GO.** All Release 1 features are implemented, and the core Windows flows have been verified natively on one NVIDIA machine. Linux compiles and passes its automated tests in a container but has never run on a Linux desktop or GPU, the installers have not been executed, the provider-token scenarios are open and the builds are unsigned. The evidence and every open gate are in [docs/RELEASE_ACCEPTANCE.md](docs/RELEASE_ACCEPTANCE.md).

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
| First-run setup | A wizard checks the NVIDIA hardware, lets you choose a Python source and a tested setup profile, then builds the environment with [uv](https://github.com/astral-sh/uv) and installs ComfyUI fully automatically. The review step lists any third-party sources, and clicking **Start setup** is the single confirmation: setup does not pause for consent. Nothing is created inside `ComfyUI/` before setup clones it, and the application restarts once after the first setup. Failed steps can be retried, skipped (optional steps only) or aborted. |
| Recommended Windows profile | Python 3.12 with PyTorch 2.10.0, CUDA 13.0 (`cu130`) and torchaudio 2.10.0, listed first and preselected. Python 3.13 / PyTorch 2.14.1 profiles (CUDA 13.0 and 12.6) remain available. |
| ComfyUI-Manager and curated nodes | ComfyUI-Manager is ticked by default. A list of 22 recommended custom nodes is offered unticked; each selected node installs at the latest commit of its repository's default branch and keeps tracking that branch. |
| Re-setup | Repair, change torch/CUDA, change Python, or switch to a custom venv. The new environment is built alongside the old one and activated only after verification. |
| Moved install | A copied or moved install folder is detected and rebuilt into a new environment from the recorded profile, keeping models, nodes and user data. |
| Environment | Shows the active environment and stored accelerator results, and lets you list, install, upgrade and uninstall pip packages. A pin guard keeps `torch`, `torchvision`, `torchaudio` and Triton at the installed versions. |
| ComfyUI runtime | Start, stop and restart ComfyUI as a child process owned by the app, with live redacted logs, launch options, port-conflict handling, an embedded ComfyUI window or the system browser. |
| ComfyUI updates | ComfyUI stays on a git branch that tracks `origin` (`master`, or `release/vX.Y` for patch releases), so `git pull` works. The Version card offers **Update to latest commit (git pull)**, **Update to latest release**, **Switch to** a tag and **Fast-forward** the current branch, each followed by a pinned requirements reinstall. Dirty or diverged checkouts are refused, never reset. |
| extra_model_paths.yaml | Generate an app-managed `vjumpkung_desktop` section from one model base folder, creating the standard model folders; other sections are kept and the previous file is backed up. |
| Model downloads | A persistent queue for CivitAI, Hugging Face, Google Drive and direct HTTPS links with progress, pause, resume, retry, de-duplication, name-conflict handling, SHA-256 verification where the provider supplies a hash, a shared bandwidth cap, and `resource_list` import and export. Hugging Face downloads use the pinned `hf` CLI by default (Xet support), with a private home folder. |
| Model library | Scans `ComfyUI/models` and the paths in `extra_model_paths.yaml`. Search, filter, rename, move, delete (to the Recycle Bin or Trash) and identify models on CivitAI. |
| Custom nodes | Browse the Comfy Registry or install from any HTTPS Git URL, with an exact-source review and consent before any third-party code runs. An approved `install.py` runs before the reviewed `requirements.txt`. Update, update all, enable, disable and uninstall, with recovery after interrupted operations. |
| Settings | Download concurrency, bandwidth cap, Hugging Face engine (`hf` CLI or native), close behaviour, theme, registry cache lifetime, CivitAI and Hugging Face tokens held in the OS keyring, app updates and redacted diagnostics. |

## Release 1 scope and requirements

| Item | Release 1 |
|---|---|
| Operating systems | Windows 10/11 x64 and Linux x64 |
| GPU | NVIDIA only. AMD, Intel, macOS and CPU-only torch builds are out of scope. |
| Webview runtime | Windows: WebView2 (the Windows bundle is configured to install it through a silent bootstrapper). Linux: `libwebkit2gtk-4.1`. |
| Git | Windows: a pinned MinGit is downloaded by the app. Linux: system `git` 2.40 or newer, which you must install yourself. |
| Defaults | Windows: Python 3.12.15, PyTorch 2.10.0 with CUDA 13.0 (`win-py312-torch2100-cu130`). Linux: Python 3.13.16, PyTorch 2.14.1 with CUDA 13.0. CUDA 12.6 profiles exist for older drivers, and Python 3.14 profiles are experimental. ComfyUI: the latest release by default, or the latest commit on `master`, or a specific tag. |
| Driver and GPU | The wizard disables any profile that your driver or GPU architecture cannot run, and shows the reason. All bundled profiles require compute capability 8.0 or newer. |
| Disk | The wizard checks free space against the selected profile's budget and blocks the install if it is too low. The recommended Windows profile budgets 17 GiB (6 GiB environment, 7 GiB other, 4 GiB margin); this estimate is not calibrated. |
| Interface language | English only |

Not in scope for release 1: more than one ComfyUI install at a time, a hosted or multi-user service, remote access to the manager, and localisation.

## Project status

The authoritative ledger is [docs/RELEASE_ACCEPTANCE.md](docs/RELEASE_ACCEPTANCE.md). Engineering progress is in [docs/IMPLEMENTATION_STATUS.md](docs/IMPLEMENTATION_STATUS.md) and [HANDOFF.md](HANDOFF.md).

**Verified natively** on Windows 11 with an NVIDIA GeForce RTX 3060 12 GiB (driver 610.88, CUDA 13.3) on 2026-10-20, using debug builds with the production asset protocol on fresh install roots:

- first-time setup on the recommended profile with ComfyUI-Manager and curated nodes, final verification, and ComfyUI 0.42.1 running on `cuda:0`;
- the final wizard defaults, ComfyUI branch tracking, update to the latest commit and tag switching;
- the `extra_model_paths.yaml` generator, the moved-install rebuild, a Hugging Face Xet download, and a Custom Nodes Git install with `install.py` before requirements and unchanged pins.

**Automated checks** (final run, 2026-10-20): Rust 802 passed, 0 failed, 12 ignored; Vitest 30 files and 202 tests; formatting, Clippy, lint, typecheck, bindings drift, 11 ACL tests, the architecture check and the native start-up smoke test pass. Unsigned NSIS and MSI bundles build.

**Not established:**

- Linux: the workspace compiles, is clippy-clean and passes its automated tests on Linux x64 in a Debian container (778 passed, 0 failed — [docs/handoff/evidence/s6-linux.md](docs/handoff/evidence/s6-linux.md)), but the app has never run on a Linux desktop or NVIDIA GPU; Secret Service, trash on a real desktop, WebKitGTK and the AppImage and .deb bundles are unverified.
- The Windows installers and the release executable have never been run; there is no clean-machine acceptance.
- Provider-token scenarios: CivitAI, gated Hugging Face repositories, Google Drive quota and the secret-redaction check.
- Several Windows scenarios (custom venv, change torch, crash recovery, Registry node install, an edited Git node, a failed dependency install and Repair, among others), accelerator wheels on real hardware, performance and accessibility measurements, and coverage.
- Authenticode signing and updater key custody (owner decisions). Placeholder application icons are still in use.

## Quick start for users

There is no published release yet. The Windows bundles below have been built locally but not yet run.

1. Install the bundle for your OS.
   - Windows: run the NSIS setup (`vjumpkung-comfyui-desktop_<version>_x64-setup.exe`, a per-user install) or the MSI. The bundles are unsigned, so Windows shows an "unknown publisher" warning.
   - Linux: run the AppImage, or install the `.deb`. Install `git` (2.40 or newer) and `libwebkit2gtk-4.1` first if they are missing. Linux bundles have not been built yet.
2. Start the application. On first launch it opens the **setup wizard**: install root, hardware check, Python source, profile, options, review. Click **Start setup** on the review step. Setup downloads several GB of packages, so it needs a network connection, and the application restarts once when it finishes.
3. Open the **ComfyUI** page and press **Start**. Use **Downloads** and **Models** to add and manage models, and **Custom Nodes** to add nodes.

Full instructions are in the [user guide](docs/DOCUMENTATION.md#part-1-user-guide).

## Quick start for developers

### Prerequisites

| Tool | Requirement |
|---|---|
| Rust | The toolchain in [rust-toolchain.toml](rust-toolchain.toml) (`1.99.0` with rustfmt and clippy). `rustup` installs it automatically on first use. |
| Windows build tools | MSVC: Visual Studio 2022 with the **Desktop development with C++** workload (component `Microsoft.VisualStudio.Component.VC.Tools.x86.x64`) and a Windows 10/11 SDK. A GNU fallback exists (see the developer guide). |
| Node.js and pnpm | pnpm `10.24.0` (pinned by `packageManager` in [ui/package.json](ui/package.json), usable through Corepack). No Node version is pinned in the repository; the checks were last run with Node 24.13.0. |
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
pnpm -C ui lint                          # frontend lint
pnpm -C ui typecheck                     # frontend types
pnpm -C ui test                          # Vitest
cargo test --workspace --locked          # Rust tests, including binding drift and ACL checks
cargo fmt --all -- --check               # Rust formatting
cargo clippy --workspace --all-targets --locked -- -D warnings
node scripts/check-arch-deps.mjs         # dependency boundaries
```

### Full check list

Run these before reporting work as done. CI workflows are disabled by the owner, so the checks run locally.

```bash
pnpm -C ui install --frozen-lockfile && pnpm -C ui lint && pnpm -C ui typecheck && VITEST_MAX_WORKERS=2 pnpm -C ui test && pnpm -C ui build
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -j 4 -- -D warnings
cargo test --workspace --locked -j 4 --no-fail-fast
node --test scripts/check-arch-deps.test.mjs && node scripts/check-arch-deps.mjs
cargo test -p desktop --test bindings --locked
cargo build -p desktop --locked --features tauri/custom-protocol
pnpm -C ui tauri build                   # Windows NSIS + MSI bundles (MSVC only; release build, about 12 minutes)
```

Also run the native start-up smoke test, which opens a window and fails if another instance is running:

```bash
cargo test -p desktop --test startup_smoke --features tauri/custom-protocol fresh_root_starts_and_closes_cleanly -- --ignored --exact --nocapture --test-threads=1
```

Notes:

- `cargo build -p desktop --features tauri/custom-protocol` needs `ui/dist`, so run `pnpm -C ui build` first.
- The IPC bindings in `ui/src/shared/ipc/bindings.ts` are generated. Never edit them by hand. Regenerate them with `cargo test -p desktop --test bindings --locked -- --ignored export_bindings`.
- Coverage targets are domain at least 90% and application at least 80% of lines, measured with `cargo llvm-cov`. Coverage has not been measured for the current tree.

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
docs/               requirements, design, ADRs, status and acceptance ledgers, handoff records
scripts/            Tauri CLI wrapper and the architecture dependency check
```

Dependencies point inward: `domain` <- `application` <- `infrastructure` <- `desktop`. `node scripts/check-arch-deps.mjs` enforces the allowed edges.

## Documentation

| Document | Contents |
|---|---|
| [docs/DOCUMENTATION.md](docs/DOCUMENTATION.md) | Full manual: user guide and developer guide |
| [docs/RELEASE_ACCEPTANCE.md](docs/RELEASE_ACCEPTANCE.md) | Release 1 acceptance ledger: requirements, scenarios, evidence, residuals and release gates |
| [docs/REQUIREMENTS.md](docs/REQUIREMENTS.md) | Normative requirements, architecture rules, acceptance scenarios and open items |
| [docs/DESIGN.md](docs/DESIGN.md) | Visual and interaction design system for the manager UI |
| [docs/adr/0001-ui-technology.md](docs/adr/0001-ui-technology.md) | Decision: Vite and React inside the Tauri webview |
| [docs/adr/0002-clean-architecture-ddd.md](docs/adr/0002-clean-architecture-ddd.md) | Decision: Clean Architecture and DDD in Rust |
| [docs/IMPLEMENTATION_STATUS.md](docs/IMPLEMENTATION_STATUS.md) | Working completion ledger |
| [HANDOFF.md](HANDOFF.md) | Current engineering state, decisions and next steps |
| [AGENTS.md](AGENTS.md) | Contributor guidelines |

## Security notes

- CivitAI and Hugging Face tokens are stored in the OS keyring (Windows Credential Manager, Linux Secret Service). If the keyring is unavailable they live in memory for that session only. Tokens are never written to SQLite, logs or URLs. The only process that receives a token is the pinned `hf` downloader, which gets the Hugging Face token in its own environment, never as an argument; ComfyUI, node scripts, uv and Git never receive tokens.
- The ComfyUI window has **no** Tauri capabilities, so web content served by ComfyUI cannot call the application's commands. Only the manager window can.
- Installing a custom node runs third-party code, and a Python venv is not a sandbox. The Custom Nodes page shows the exact source and asks for consent first. **First-run exception (NFR-SEC-3):** in first-run setup, the review disclosure and the **Start setup** click confirm ComfyUI-Manager and the selected curated sources without another prompt; the full repository URL and resolved commit are logged before installation. Repair and the Custom Nodes page keep their per-plan review.
- Selected curated Git nodes in consented first-run setup may ship `__pycache__` content and top-level native programs or libraries, including ComfyUI-PlagueKind-Nodes. These findings are logged as acknowledged warnings; containment, provenance and resource limits still apply. The Custom Nodes page and non-consented setup keep both refusals.
- On Windows, before consent, new node content whose top-level folder holds a program or library is refused. Git sources are checked from the exact commit's file list before checkout, Registry archives after extraction into staging. The refusal reads: "This node has programs or libraries (.dll, .exe and similar) in its top-level folder. Tools could load them before you approve the node, so it cannot be installed." The checked suffixes are `.dll`, `.exe`, `.com`, `.cpl`, `.drv`, `.ocx`, `.scr` and `.sys`. Installed and user-placed nodes are not re-checked.
- An approved custom-node `install.py` runs **before** reviewed requirements for installs, updates and dependency retries, including selected curated setup nodes. Protected pins are restored after each mutation, profile constraints are restored after the hook, and the frozen reviewed requirements are validated again before installation. App-managed resolution and installation remain wheel-only; arbitrary approved scripts can invoke their own installers or builds and run with your permissions.
- ComfyUI listens on `127.0.0.1` by default. Listening on the LAN is opt-in and requires acknowledgement, because ComfyUI has no login.
- Credentials are attached only to the exact provider origin and stripped on redirects to other origins. Logs, errors and diagnostics redact tokens and signed URL queries.
- Known residuals, such as the unauthenticated loopback Hugging Face relay, are listed in the [acceptance ledger](docs/RELEASE_ACCEPTANCE.md#residuals).

## Application updates and signing

Self-update uses the Tauri updater and is **inactive** in any build that does not receive both `VJUMP_UPDATER_PUBKEY` and `VJUMP_UPDATER_ENDPOINT` at build time. No build so far has had them: Settings then says "This build has no update source configured, so it cannot update itself.", and the update commands return a controlled refusal. In a configured build, the application installs only the signed release you reviewed, with ComfyUI stopped. This flow is covered by automated tests only.

Signing keys are an owner responsibility: the private key (`TAURI_SIGNING_PRIVATE_KEY`) is never stored in the repository, and a generated development key must not be used for a release. The Windows installers are not Authenticode-signed. Key custody and code signing are open owner decisions.

## License

**No license has been selected for this project yet.** This is an owner decision. Until the owner chooses one, no license is granted, and no `LICENSE` file is present. Third-party components keep their own licenses.
