# vjumpkung-rust-comfyui-desktop — Requirements Specification

| | |
|---|---|
| Status | Draft v0.3 (v0.2: UI decision + Clean Architecture/DDD; v0.3: `hf` CLI default for HF downloads, design system → [DESIGN.md](DESIGN.md)) |
| Date | 2026-10-15 |
| Main language | Rust |
| Release 1 platforms | Windows 10/11 x64 and Linux x64, NVIDIA GPUs only |

Requirement keywords: **MUST** = mandatory for the release it is tagged with, **SHOULD** = expected unless there is a documented reason not to, **MAY** = optional.
Priority tags: **[P0]** MVP, **[P1]** release 1, **[P2]** later.

Fact labels used in this document:
- *(observed 2026-10-15)* — checked against the live registry or API on that date.
- *(reference)* — taken from the local reference projects listed in §1.3.
- *(to validate)* — a combination that is expected to work but must be proven by a runtime test before it ships.

---

## 1. Overview

### 1.1 Purpose
This is a native desktop app for installing, running and maintaining a local ComfyUI. It is a Rust program whose whole UI is a web frontend rendered in the OS webview (WebView2 on Windows, WebKitGTK on Linux). It does **not** use Electron.

### 1.2 Goals
1. Download and manage model files from CivitAI, Hugging Face, Google Drive and direct HTTPS URLs. Management covers download, rename and delete.
2. Manage ComfyUI custom nodes and their Python dependencies. Nodes can be installed from the Comfy Registry, which also supplies the browsable node list, or by `git clone` of any Git URL.
3. Manage the Python interpreter, the venv and the PyTorch build through a first-run setup wizard. The user may point the app at a custom venv path, and can run setup again at any time.
4. Install ComfyUI with an "essentials" profile modeled on the reference portable installer (§1.3), then start, stop and update it from the app.

### 1.3 Reference material
| Reference | Location | Used for |
|---|---|---|
| Model-manager backend (Python/FastAPI) | `D:\vjump\vjumpkung-sd-ui-manager-backend` | Download logic, provider handling, model-pack format, custom-node install flow |
| Model-manager frontend (Next.js) | `D:\vjump\vjump-resource-manager-frontend` | UI components, UX, design conventions |
| Portable installer | `D:\vjump\pixellatent-happy-meals-scripts\comfyui-python-3-12-happy-meals\portable_version\install_comfyui_3_12_uv_nvidia_torch_2100_portable.bat` (+ `update_comfyui_portable.bat`) | Setup steps, environment isolation, essential packages, verification, update safety |

### 1.4 Non-goals for release 1
- AMD (ROCm), Intel (XPU), Apple/macOS and CPU-only torch builds. The architecture MUST NOT preclude them, but they are [P2]. The exception is the bundled CPU testing profiles, which exist only so the app can be exercised without an NVIDIA GPU and are not a supported generation path.
- The reference backend's Forge, InvokeAI, Z-Image and RunPod variants. This app is ComfyUI only.
- Multi-user use, remote access to the manager, or a hosted service.
- Managing more than one ComfyUI install at a time. This is a single active install [P2].
- Making the install folder run unchanged after a move. A move is handled by a guided rebuild (§6.1, FR-SETUP-12).

---

## 2. Technology stack

Versions are the latest stable releases *(observed 2026-10-15)*. Implementation pins exact versions in `Cargo.lock` / `pnpm-lock.yaml` and upgrades them deliberately.

### 2.1 Desktop shell and backend (Rust)
| Concern | Choice | Version | Rationale |
|---|---|---|---|
| App shell / webview | **Tauri 2** | `tauri` 2.12.4, `@tauri-apps/cli` 2.12.x, `@tauri-apps/api` 2.12.4 | Rust host with the system webview (WebView2 or WebKitGTK through wry 0.57 / tao 0.37). Provides IPC, capability-based permissions, bundler, updater and single-instance. Tauri 3 is still alpha (v3.0.0-alpha.5), so it is out. |
| Async runtime | `tokio` | 1.53.x | Standard. Tauri runs on it. |
| HTTP | `reqwest` (rustls, stream) | 0.13.x | Streaming downloads, Range resume, redirect control. |
| Persistence | `rusqlite` (feature `bundled`) | 0.40.x | Single local DB for the job queue, history, model catalog and node provenance. No server. |
| Hashing | `sha2` (+ `blake3` optional) | 0.11.x | Verify CivitAI and Hugging Face hashes and identify local files. |
| Archives | `zip` | 9.x | Registry `node.zip` extraction, with hardened checks (NFR-SEC-4). |
| Secrets | `keyring` | 4.x | Windows Credential Manager on Windows, Secret Service on Linux. |
| Recycle bin | `trash` | 5.x | Recoverable delete on both OSes. |
| GPU detection | `nvml-wrapper` | 0.13.x | Driver version, CUDA driver API version and compute capability. Falls back to `nvidia-smi`. |
| System info | `sysinfo` | 0.39.x | Disk space, RAM, process tree. |
| Logging | `tracing` + `tauri-plugin-log` | 0.1.x / 2.10.x | Structured logs with secret redaction. |
| Tauri plugins | `dialog` 2.8, `single-instance` 2.5, `updater` 2.14, `shell` 2.5 (restricted) | — | File pickers, one running instance, app self-update. |
| Typed IPC bindings | `tauri-specta` + `specta` **2.0.0-rc.25 (pinned, prerelease)** | rc.25 | Generates TypeScript bindings for both commands and events. This is a deliberate exception to "stable only": ts-rs 12 is stable but generates types only, which leaves command wrappers hand-written and liable to drift. It is used only in the desktop crate. The compatible version set is validated at M0. |
| Async ports | `async-trait` | 0.1.x | Makes injected `Arc<dyn Port + Send + Sync>` ports dyn-compatible. Native `async fn` in traits is not dyn-compatible. |
| Errors | `thiserror` | 2.x | One error type per layer and context, never a shared catch-all enum (§3.4). |
| Test doubles | `mockall` (or hand-written fakes) | 0.15.x | Application-layer tests against ports. |

Rejected alternatives:
- **Raw wry/tao.** Would mean re-implementing IPC, asset serving, permissions, packaging and updates.
- **Electron.** Excluded by the brief.
- **gix / git2 instead of a git subprocess.** See §2.3.

### 2.2 Frontend (rendered in the webview)
| Concern | Choice | Version |
|---|---|---|
| Build | **Vite** | 8.x |
| UI | React + TypeScript | React 19.x |
| Styling | Tailwind CSS | 4.x |
| Components | shadcn (**`radix-vega`** style, **lucide-react** icons, Sonner toasts). Logic is ported from the reference frontend and restyled per [DESIGN.md](DESIGN.md). | — |
| Design system | [docs/DESIGN.md](DESIGN.md), adapted from vjump.dev: slate canvas, pastel yellow/cyan/pink accents, Inter + Geist Mono (bundled locally, Noto Sans Thai fallback), `next-themes` | — |
| Server state | `@tanstack/react-query` over Tauri `invoke` | 5.x |
| Log view | `@xterm/xterm` (as the reference frontend uses) | — |

**UI technology decision.** Three options were considered; the full record is in [ADR-0001](adr/0001-ui-technology.md).

| Option | Verdict | Reason |
|---|---|---|
| A. Fully native Rust GUI (egui 0.37, iced 0.14, Slint 1.18, gpui) | Rejected | Contradicts the brief ("webview inside a Rust GUI"). ComfyUI's own web UI still needs an embedded webview (FR-COMFY-3), so this means mixing a native toolkit with a child webview. Every screen would be rewritten. |
| B. Rust compiled to WASM in the webview (Leptos 0.8 / Dioxus 0.7) | Rejected for release 1 | One language and shared types, but the reference React components can't be reused. The component ecosystem is smaller (xterm and virtual tables need JS interop), and both frameworks are mid-upgrade (Leptos 0.9 RC, Dioxus 0.8 alpha). |
| **C. Vite + React + TypeScript in the Tauri webview** | **Chosen** | Reuses the reference frontend's presentation components, has a large ecosystem and hot reload, and gets typed IPC through generated bindings. |

Under Clean Architecture the UI is a detail (§3.3). The domain and application crates don't depend on it, so this choice can be reversed later without touching business rules.

Why Vite and not Next.js:
- The reference frontend is a single page with a static export. It uses no Next server features, API routes or middleware *(reference)*.
- Vite is Tauri's first-class template and removes the dev-only `/api` and `/ws` rewrites.
- Components are copied from the reference. Their `axios` and WebSocket calls are replaced by typed Tauri IPC.

Frontend conventions:
- Tailwind v4 utility classes.
- Local shadcn components in `ui/src/shared/ui/`.
- Sonner for toasts.
- System, light and dark themes.
- **Visual design follows [DESIGN.md](DESIGN.md) (NFR-UX-1).** This supersedes the reference frontend's `base-vega` style and Remixicon icons. Components copied from the reference must be adapted from Base UI to Radix APIs.

### 2.3 Bundled and managed external tools
| Tool | Windows | Linux | Notes |
|---|---|---|---|
| **uv** | Bundled, pinned (latest observed 0.13.1) | Bundled, pinned | Creates venvs, installs Python, resolves packages. Reference installer pinned 0.12.13 *(reference)*. |
| **Python** | uv-managed CPython (python-build-standalone), pinned patch | Same | Ships headers and import libs, so Triton JIT works without the reference installer's include/libs download and `._pth` edits. |
| **hf** (Hugging Face CLI) | App-owned, isolated tool venv `tools/hf/` on the pinned uv-managed Python. Package `hf==2.2.0` + `hf-xet` 1.7.0 *(observed)*, installed with `uv pip sync --require-hashes --only-binary :all:` from a complete per-platform lock file shipped with the app. | Same | The default Hugging Face download engine (FR-DL-3). Never installed into the ComfyUI venv, so node dependencies can't break it. Invoked by direct executable path. |
| **gdown** (Google Drive downloader) | App-owned, isolated tool venv `tools/gdown/` on the same pinned uv-managed Python as `hf`. Package `gdown==6.4.2` with its dependencies, installed with `uv pip sync --require-hashes --only-binary :all:` from a complete per-platform lock file shipped with the app. | Same | The Google Drive download engine (FR-DL-4). Never installed into the ComfyUI venv. Run as `<venv python> -I -B -X utf8 -c <driver>`; the native Drive path is only a fallback. |
| **Git** | Bundled, pinned **MinGit** (reference used 2.55.0.3) | System `git` ≥ 2.40, detected. The app MUST show install guidance if it is missing. | Run as a subprocess, as the reference installer does. Supports any host, auth helper or proxy the user's git supports. |

Each bundled binary is downloaded at build time or first run. Its SHA-256 is pinned in the app, as the reference installer does *(reference)*.

### 2.4 Python and PyTorch defaults
| Item | Default | Alternatives offered |
|---|---|---|
| Python | **Windows: 3.12** (3.12.15); **Linux: 3.13** (latest patch observed 3.13.16) | Windows: 3.13 (the torch 2.14.1 profile). Linux: no 3.12 profile. 3.14 is offered as "experimental". 3.15.0 was released 2026-10-09 and is not offered until ComfyUI supports it. |
| PyTorch | **Windows: 2.10.0 + cu130; Linux: 2.14.1 + cu130** *(observed: Windows and manylinux x86_64 cp313 wheels for 2.14.1 exist on download.pytorch.org/whl/cu130)* | 2.14.1 + cu130 on Windows, 2.14.1 + cu126 for older GPUs (the last torch release with CUDA 12.x wheels), and earlier versions in the profile list |
| ComfyUI | Latest release tag (observed v0.40.0, 2026-10-13) | Any tag or `master` (the wizard offers Latest release, Latest commit (master) and a specific tag — owner decision 2026-10-20) |

The Windows recommended profile is `win-py312-torch2100-cu130` (Python 3.12 + torch 2.10.0 cu130), by owner decision 2026-10-20 after it was verified on an RTX 3060 (ComfyUI 0.42.1 runs on `cuda:0`). The Linux default is unchanged.

Basis for the defaults:
- docs.comfy.org recommends Python 3.13, with 3.12 as the fallback.
- The official portable build ships Python 3.13 with torch cu130.
- PyTorch 2.15 is expected to drop Maxwell, Pascal and Volta from the default wheels, so the app MUST gate profiles on GPU compute capability.

Accelerator wheel compatibility (triton, SageAttention, FlashAttention, onnxruntime-gpu, llama-cpp-python) differs by Python, torch, CUDA and OS. It is defined per setup profile (§5.4) and is *(to validate)* for every profile shipped.

---

## 3. Architecture

### 3.1 Processes
```
┌──────────────────── vjumpkung-rust-comfyui-desktop (Rust, Tauri) ────────────────────┐
│  Manager window (webview)  ── typed IPC (invoke + Channel/events) ──►  Core services   │
│   React/Vite UI                                                        ├ setup/env     │
│                                                                        ├ comfy process │
│  ComfyUI window/tab (webview) ── http://127.0.0.1:<port> ──┐           ├ downloads     │
│   no IPC capabilities                                      │           ├ models        │
│                                                            │           ├ custom nodes  │
└────────────────────────────────────────────────────────────┼───────────├ secrets       │
                                                             ▼           └ SQLite        │
                                              ComfyUI child process (venv python)       │
                                              spawned and owned by the core ──────────┘
uv / git / python subprocesses are spawned by the core with a minimal, controlled environment.
```

- **Manager transport.** The manager UI talks to the core only through Tauri IPC. There is **no** localhost HTTP API or WebSocket for the manager, which replaces the reference `/api/*` and `/ws/*`.
- **ComfyUI transport.** ComfyUI itself still serves HTTP and WebSocket on loopback. The app binds it to `127.0.0.1` by default.
- **Webview isolation.** The ComfyUI webview MUST have no Tauri capabilities: no IPC commands, no filesystem or shell access, and no secrets. Manager commands are allow-listed for the manager window only through Tauri capabilities.

### 3.2 Layout on disk (default)
```
<InstallRoot>/                    user-chosen; default %LOCALAPPDATA%\vjumpkung-comfyui (Win) / ~/.local/share/vjumpkung-comfyui (Linux)
  ComfyUI/                        git checkout on master or release/vX.Y (models/, custom_nodes/, user/, input/, output/)
  envs/<env-id>/                  app-owned venv(s)
  tools/uv/ tools/git/            pinned bundled tools (git: Windows only)
  data/
    app.db                        SQLite
    cache/{uv,pip,hf,torch,triton,registry}/
    downloads/                    .part staging (same volume as the models dir)
    logs/
  install.json                    marker: install root path, env manifest, profile id, schema version
```

Environment isolation, carried over from the reference installer:
- Every subprocess the app starts gets `UV_CACHE_DIR`, `UV_PYTHON_INSTALL_DIR`, `PIP_CACHE_DIR`, `HF_HOME`, `TORCH_HOME` and `TRITON_HOME` pointed under `data/cache/`.
- `PYTHONNOUSERSITE=1` and `PYTHONUTF8=1` are set.
- `GIT_DIR` and `GIT_WORK_TREE` are cleared.

### 3.3 Clean Architecture + Domain-Driven Design (normative)
The rationale is recorded in [ADR-0002](adr/0002-clean-architecture-ddd.md).

This is a **Rust adaptation** of the local clean-architecture guidance, approved by the project owner:
- Use cases are concrete typed structs with an `execute(input) -> Result<Output, Error>` method.
- Traits (ports) are defined only where dependency inversion is needed (repositories, providers, processes, filesystem, secrets, clock) or for streaming output (progress sinks).
- There is no `InputPort` trait or presenter trait per use case.

#### 3.3.1 Contexts (DDD)
These are the proposed business areas. Each one is a module inside each layer crate.

| Context | Aggregates / key types | Owns |
|---|---|---|
| `environment` | `PythonEnvironment` (aggregate: owned/adopted, Python/torch/CUDA, verification state, `repair_required` flag). Active-env selection enforces **exactly one verified active env**. `SetupProfile` and `HardwareObservation` are immutable inputs. `ProvisioningRun` is a persisted application workflow, not a domain aggregate. | FR-SETUP, FR-ENV |
| `comfy_runtime` | `ComfyInstallation` (revision, launch options); lifecycle policy (`Stopped → Starting → Running → Stopping`, `Crashed`). OS process handles stay in infrastructure. | FR-COMFY |
| `models` | Two submodules. `acquisition`: `DownloadJob` (aggregate; state machine per FR-DL-6), `SourceRef` (CivitAI / HF / Drive / Direct), `ExpectedHash`. `library`: `ModelFile` (aggregate; hash and provenance optional for scanned files), extensible `ModelCategory`. | FR-DL, FR-MODEL |
| `custom_nodes` | `InstalledNode` (aggregate: provenance `Registry{id,version}` / `Git{remote,commit}` / `Manual`, enabled, dependency outcome). `InstallConsent` names the exact resolved source and version that will run. Dependency changes go through `environment` application services. | FR-NODE |
| `gallery` | `GalleryLocation` (area `input`/`output` + relative path; the reserved staging name is rejected), media kind by extension. Listing, transfer and delete planning are application workflows over a filesystem port; no persisted state. | FR-GALLERY |
| `settings` (supporting) | Validated settings and credential **references or presence only**. Token values never enter domain or application types. | FR-SET |
| `shared` (shared kernel) | `SafeFileName`, `RelativeModelPath`, `Sha256`, `ByteSize`, ids. Value objects validate **names and relative components only**. Real filesystem containment is checked by adapters (NFR-SEC-5). | — |

#### 3.3.2 Cargo workspace and dependency rule
```
crates/
  domain/          entities, value objects, domain services, transitions — per-context modules
  application/     use cases, input/output models, ports (traits), orchestration — per-context modules
  infrastructure/  adapters: reqwest providers, rusqlite repos + migrations, uv/git/python processes,
                   hardened zip, fs + trash, keyring, nvml — per-context modules
  desktop/         Tauri: commands (controllers), presenters, serde IPC DTOs, tauri-specta bindings,
                   capabilities, composition root
ui/                Vite + React
```

Allowed dependency edges (everything else is forbidden):
```
application    → domain
infrastructure → application, domain
desktop        → application, infrastructure, domain
```

Rules by layer:
| Layer | MAY contain | MUST NOT contain |
|---|---|---|
| domain | Pure rules, validated constructors, state transitions, context errors (`thiserror`) | serde, Tokio, reqwest, rusqlite, Tauri, filesystem or clock access |
| application | Use cases, plain input/output models, port traits (`async-trait`, `Send` futures), port error enums that describe meaningful failures, orchestration and the maintenance gate (§3.3.3) | serde/specta derives, reqwest/SQLite/Tauri/Tokio types |
| infrastructure | Provider and persistence DTOs, SQL rows, subprocesses, Tokio scheduling, filesystem containment checks, translation of adapter errors into port errors | Tauri types, UI DTOs |
| desktop | serde IPC DTOs, generated bindings, controllers/presenters, sanitized error codes, wiring in Tauri `run`/`setup` (`main.rs` stays a thin launcher) | Business rules. Passing `AppHandle` or Tauri types inward. Querying SQLite directly. |

- **Errors.** Each layer owns its errors. Domain violations map to application errors, which map to sanitized IPC error codes. No secret, signed URL or raw path from outside an authorized root crosses into an IPC error.
- **Composition.** Concrete adapters are created and wired **only** in the desktop composition root. Use cases receive `Arc<dyn Port + Send + Sync>`.
- **Granularity.** Crates are split by layer now, with contexts as modules. A context gets its own crate only when independent evolution or build time justifies it. Context internals are private, and each context exposes a small facade.
- **Frontend layering.** `ui/src/app → ui/src/features/<context> → ui/src/shared` (`shared/ui` = shadcn, `shared/ipc` = generated bindings plus a gateway). Components never call `invoke` directly; they go through feature hooks and the IPC gateway. One feature never imports another feature's internals.

#### 3.3.3 Cross-context coordination
- **Maintenance gate.** A single application-level scheduler serializes **maintenance operations**: setup and activation, environment changes, node operations, ComfyUI updates, and ComfyUI start/restart.
  - The gate is acquired **before** checking process state.
  - ComfyUI starts are blocked for the whole maintenance window.
  - Process termination is verified, including a surviving headless ComfyUI.
  - Ownership is held through subprocess cancellation and cleanup.
  - Composed workflows acquire the gate **once**, so there is no recursive locking.
  - The gate only controls work issued by this app. ComfyUI-Manager or user scripts are outside it, so the app re-scans after ComfyUI stops (FR-NODE-10).
- **Not gated.** Downloads keep their own concurrency limit (FR-DL-6). Model rename, move and delete are **allowed while ComfyUI runs**. They are coordinated per destination path and rely on locked-file recovery (FR-MODEL-2/3).
- **No general event bus initially.** Required effects use awaited orchestration, e.g. finishing a download registers the catalog entry (FR-DL-9). Channels and events are used for UI notification only.

#### 3.3.4 Operation matrix
| Operation | Maintenance gate | ComfyUI must be stopped | Consent | Failure state | Recovery |
|---|---|---|---|---|---|
| Setup / re-setup / activate env | yes | yes | — | new env `failed`; old env stays active | Retry / discard new env |
| Install/upgrade/uninstall pip package | yes | yes | — | env `repair_required` | Retry / Repair (reinstall profile + node requirements) |
| Install custom node (registry or git) | yes | yes | yes (NFR-SEC-3) | files: no partial folder (staged). deps or `install.py`: node `deps-failed` + env `repair_required` | Retry deps / Uninstall / Repair env |
| Update / enable / disable / uninstall node | yes | yes | update: yes if the source changed | staged swap; old version kept until success | Retry / restore from trash |
| Update ComfyUI | yes | yes | — | dirty or diverged → refused, no change | Resolve manually, retry |
| Start / restart ComfyUI | yes | n/a | — | `Crashed` with log | Retry / change launch options |
| Download | no (own queue) | no | — | job `failed` / `paused`; no file in the models dir | Retry / resume |
| Rename / move / delete model | per-path lock | no | delete: confirm | locked → unchanged, error shown | Retry / stop ComfyUI and retry |

#### 3.3.5 Enforcement
- **CI dependency check.** Inspect the resolved dependency graph (`cargo metadata`), including build and target-specific dependencies. Fail if a forbidden edge exists, or if `domain`/`application` gain a dependency outside their allow-lists (`domain`: `thiserror` only; `application`: `domain`, `thiserror`, `async-trait`, `futures-core`).
- **Frontend.** `eslint-plugin-boundaries` enforces the frontend layering rules.
- **Bindings.** The generated IPC bindings are checked in. CI regenerates them and fails on any diff.
- **Clippy and format.** `cargo clippy -D warnings` and `cargo fmt --check` run on both OSes.

#### 3.3.6 Testing strategy
| Level | Scope | Examples |
|---|---|---|
| Domain unit | Pure, no I/O | Job and process transitions, env ownership, profile compatibility, consent, `SafeFileName` rules |
| Application | Use cases with fakes or mocks of ports | Start-vs-maintenance race, serialization, cancellation cleanup, torch pin guard, idempotent download finalization |
| Infrastructure integration | Temp dirs, in-memory/temp SQLite, HTTP fixtures | Migrations, redirects and auth stripping, Range resume, malicious zip corpus, Windows case-only rename, trash failure, crash reconciliation |
| Desktop / IPC | Serialization fixtures, binding drift, capability denial | ComfyUI webview `invoke` is denied |
| E2E smoke | Real app on Windows and Linux (tauri-driver) | Acceptance scenarios §8 |

---

## 4. Functional requirements

### 4.1 First-run setup and re-setup (FR-SETUP)

| ID | Pri | Requirement |
|---|---|---|
| FR-SETUP-1 | P0 | On first launch, with no valid `install.json`, the app MUST open a setup wizard. The steps are: install root → hardware check → Python source → setup profile (Python + torch + CUDA) → optional components → review → install. |
| FR-SETUP-2 | P0 | **Hardware check.** Detect the NVIDIA GPU(s), driver version, maximum supported CUDA version and compute capability. Also check free disk space on the install volume, OS version and, on Windows, whether long paths are enabled. Profiles whose CUDA build is newer than the driver supports, or that drop the GPU's architecture, MUST be disabled with the reason shown. The CPU testing profiles are not gated on the GPU or the driver. |
| FR-SETUP-3 | P0 | **Python source options:** (a) **App-managed.** uv installs the pinned CPython and creates a venv under `envs/`. This is the default. (b) **Custom venv path.** The user picks an existing venv, or an empty folder in which the app creates one. (c) **Custom interpreter.** The user picks a `python` executable and the app creates a venv from it. |
| FR-SETUP-4 | P0 | **Validate custom Python.** For options (b) and (c), check: version in the supported range; 64-bit; `pip`/`uv` usable; headers and import library present (needed by Triton). On Windows, also check the MSVC runtime DLLs load. Show pass/fail per check. A failed check blocks the profile features that need it, e.g. accelerators. |
| FR-SETUP-5 | P0 | **Environment ownership.** Each env is marked **app-owned** or **adopted**. Adopted envs are never deleted, and are rebuilt only after explicit confirmation. |
| FR-SETUP-6 | P0 | **Install steps.** Each step shows progress and a live log, and all of them are idempotent on retry. Steps, following the reference installer: (1) fetch and verify tools (uv, MinGit on Windows, and the `hf` tool venv from its hash-locked requirements); (2) Python/venv; (3) seed `pip setuptools wheel` (+ `msvc-runtime` on Windows, where app-local runtime DLLs are needed); (4) clone ComfyUI at the selected tag; (5) install torch/torchvision/torchaudio from the profile index URL; (6) verify the torch version and CUDA availability; (7) Triton (`triton-windows` on Windows, `triton` on Linux, at the profile version); (8) optional accelerators; (9) `ComfyUI/requirements.txt`; (10) optional ComfyUI-Manager; (11) optional curated custom nodes; (12) final verification (`import torch, triton, comfy`; report presence of sageattention, flash_attn and llama_cpp); (13) cache clean (optional). |
| FR-SETUP-7 | P0 | **Torch pin guard.** Dependency installs after step 5 MUST run with a uv constraints file pinning `torch`, `torchvision`, `torchaudio` (and `triton*`) to the installed versions. After every dependency operation, the app re-asserts the installed versions, as the reference updater does. |
| FR-SETUP-8 | P1 | **Optional accelerators.** Install is per-component and non-fatal, with failures shown as warnings, matching the reference. Components per profile: SageAttention, FlashAttention 2/3, onnxruntime-gpu, llama-cpp-python, and the transformers/onnx/numpy/pillow constraints. |
| FR-SETUP-9 | P1 | **Curated node packs.** Offer an optional list of recommended custom nodes. The default list mirrors the reference installer's 22 nodes, all selectable including ComfyUI-PlagueKind-Nodes. The curated list is offered unticked; the user ticks the nodes they want (owner decision 2026-10-20). Nodes are installed by FR-NODE logic. Each entry installs the latest commit of its repository's default branch (no commit lock), is recorded as a normal git node that tracks that branch, and is covered by the first-run confirmation of NFR-SEC-3. Only selected curated Git sources with persisted FirstRun consent may bypass shipped `__pycache__` and Windows top-level native-image content refusals; each finding is a warning logged as acknowledged. Containment, provenance, safe Git configuration, package preflight and resource limits remain enforced. Setup runs `install.py` if present before reviewed requirements, without remembering source/script trust or authorizing source builds. Revalidate persisted consent and selected repository before publishing a relaxed plan. |
| FR-SETUP-10 | P0 | **Re-setup.** Available any time from Settings, with ComfyUI stopped first. Modes: *Repair* (re-run verification and reinstall missing pieces), *Change torch/CUDA* (same Python), *Change Python* (a new minor version always creates a **new** venv), *Switch to custom venv*. The new env is built alongside the old one. Activation happens only after verification passes. The previous env is kept until the user deletes it. Models, `user/`, `input/`, `output/` and custom node folders MUST be preserved. |
| FR-SETUP-11 | P0 | **Failure handling.** A failed step leaves the previous working env active. The wizard shows the failing command, its exit code and the last log lines, with Retry / Skip (optional steps only) / Abort. |
| FR-SETUP-12 | P1 | **Moved install.** On launch, if `install.json` records a different install root, or the venv's interpreter is broken, the app MUST offer a **guided rebuild**. The rebuild recreates the venv with the same profile and reinstalls node requirements, keeping all user data. |
| FR-SETUP-13 | P1 | **Disk-space preflight.** Estimate the required space per profile and block the install if free space is below it plus a margin. |
| FR-SETUP-14 | P1 | **ComfyUI-Manager default configuration** (owner request 2026-10-22). When `ComfyUI/custom_nodes/ComfyUI-Manager` is present, final verification and a healthy Repair ensure `ComfyUI/user/__manager/config.ini` exists with the app's default contents (`use_uv = True`, `security_level = weak`, `network_mode = public`, `db_mode = cache`, …; exact bytes in `infrastructure::environment::manager_config`). The file is **created only if missing** and never replaced, so settings changed later through ComfyUI-Manager survive Repair and Rebuild (FR-SETUP-10). Links, directories or unsafe entries at that path fail the step. ComfyUI-Manager reads this path only when ComfyUI provides the system user directory; older ComfyUI uses `user/default/ComfyUI-Manager/config.ini`. |

### 4.2 Environment management (FR-ENV)

| ID | Pri | Requirement |
|---|---|---|
| FR-ENV-1 | P0 | Show the active env: Python version and path, torch/CUDA build, triton, accelerators, ComfyUI version/commit, and owned/adopted status. |
| FR-ENV-2 | P1 | List installed packages (`uv pip list --format json`), with search. |
| FR-ENV-3 | P1 | Install, upgrade or uninstall an arbitrary pip package, with the torch pin guard (FR-SETUP-7). The user must confirm when the resolver wants to change a pinned package. |
| FR-ENV-4 | P0 | All dependency-changing operations are **serialized** through one queue and require ComfyUI to be stopped. The app offers to stop it. |
| FR-ENV-5 | P2 | Multiple envs side by side, with a switcher. |

### 4.3 ComfyUI lifecycle (FR-COMFY)

| ID | Pri | Requirement |
|---|---|---|
| FR-COMFY-1 | P0 | **Start / stop / restart.** ComfyUI runs as a child process owned by the app: `python -s main.py --listen 127.0.0.1 --port <port> [--windows-standalone-build] [user args]`. Readiness is detected by polling `/system_stats`. Port conflicts are detected and the next free port is offered. |
| FR-COMFY-2 | P0 | **Logs.** Stream stdout/stderr live to an xterm view, with a ring buffer. Logs are saved to `data/logs/` with rotation. tqdm-style progress lines render correctly. |
| FR-COMFY-3 | P0 | **Embedded UI.** Open ComfyUI's web UI in an app webview (tab or window) with no IPC capabilities (§3.1). Also offer "Open in system browser". On Linux this is the fallback if WebKitGTK performance is inadequate *(to validate)*. |
| FR-COMFY-4 | P0 | **Process cleanup.** Closing the app MUST ask to stop ComfyUI, or keep it running headless (setting). The whole child process tree is terminated: a Job Object on Windows, a process group on Linux. |
| FR-COMFY-5 | P1 | **Launch options UI.** Port, `--lowvram`/`--highvram`, attention backend (`--use-sage-attention`, etc.), preview method, `--enable-manager`, extra args. Stored per install. |
| FR-COMFY-6 | P0 | **Update ComfyUI.** `git fetch` then switch to the selected tag, explicitly update to the latest development commit on origin’s default branch (normally `master`; owner decision 2026-10-20), or `pull --ff-only` on the current branch, then reinstall `requirements.txt` with the pin guard. If the repo is dirty or has diverged, the update MUST stop and show `git status`, with no reset or force. This is the reference updater's behavior. ComfyUI stays on a branch that tracks `origin`, so `git pull` works: `master` when the installed tag is on it, otherwise the tag's release branch `release/vMAJOR.MINOR`, where ComfyUI publishes patch releases. Switching tags moves that local branch pointer only when the tree is clean and the branch has no unpublished commits. A tag on neither branch, or a checkout on a branch of the user's own, is checked out detached as before. |
| FR-COMFY-7 | P1 | **Version picker.** List release tags, show the current commit, allow rollback to a previous tag. |
| FR-COMFY-8 | P1 | **`extra_model_paths.yaml`.** Read it, and optionally manage it, so the model library (FR-MODEL) and ComfyUI agree on model locations. Implemented: the app manages its own `vjumpkung_desktop` section from a chosen base folder; other sections are kept. |

### 4.4 Model downloads (FR-DL)

| ID | Pri | Requirement |
|---|---|---|
| FR-DL-1 | P0 | **Add a download** from a URL plus a target category (a ComfyUI `models/` subfolder, §4.6) plus an optional filename. The URL is detected as CivitAI, Hugging Face, Google Drive, or direct HTTPS. |
| FR-DL-2 | P0 | **CivitAI.** Accept model page URLs (`/models/{id}?modelVersionId=`), version URLs and `api/download/models/{versionId}?fileId=` URLs, on the domains `civitai.com`, `civitai.green` and `civitai.red` *(reference)*. Steps: (1) resolve metadata with `GET /api/v1/model-versions/{id}` *(observed)*. (2) If there are several files, let the user pick, defaulting to `primary`, or match `fileId`/`type`/`format`/`size`/`fp` *(reference)*. (3) Use the API filename as the default. (4) Download from `/api/download/models/{versionId}?fileId=`, which 307-redirects to a signed URL *(observed)*. (5) Verify the SHA-256 against `files[].hashes.SHA256`. The token is sent as an `Authorization: Bearer` header to CivitAI only. (6) Suggest a category from `model.type` (Checkpoint→checkpoints, LORA→loras, VAE→vae, TextualInversion→embeddings, Controlnet→controlnet, Upscaler→upscale_models, …); the user can override it. Clear errors are required for: token required (401/403), early-access/paid (`usageControl`/`paidAccess`), not found. |
| FR-DL-3 | P0 | **Hugging Face.** Accept `/resolve/` and `/blob/` URLs, including `datasets/` and `spaces/`, a revision, and URL-encoded paths, and reject traversal *(reference)*. A repo URL opens a file browser via `GET /api/models/{repo}/tree/{rev}` *(observed)*, which supports multi-select and creates **one job per file**. Gated, private or missing repos MUST give distinct errors; gated shows "accept the license on HF" with a link. Integrity: when `X-Linked-ETag` is present (an LFS file) it is the SHA-256 and MUST be verified. Otherwise the result is marked *unverified*. **Engine:** `hf` CLI by default (FR-DL-3a); native HTTP when selected or as a fallback (FR-DL-3b). |
| FR-DL-3a | P0 | **HF default engine: official `hf` CLI** (owner decision 2026-10-15; matches the reference backend, which prefers `hf download`). **Before transfer:** resolve repo type, an **immutable commit**, file size and SHA-256 if available through the HF API (native). **Command:** run `hf download <repo> <path> --type <model/dataset/space> --revision <commit> --local-dir <data/downloads/hf/<job-id>> --format json`. **Environment:** minimal allow-listed environment plus `HF_HOME=data/cache/hf` and `HF_HUB_DISABLE_TELEMETRY=1`. The token is passed **only** as `HF_TOKEN` in this child's environment (NFR-SEC-2), never as `--token` and never via `hf auth login`. stdout/stderr are redacted before logging. **After exit 0:** check the staged file exists with the expected size, fsync, verify SHA-256 when available, then run the durable publication workflow (FR-DL-9). **Progress:** estimate it by polling the current attempt's partial file under `<local-dir>/.cache/huggingface/download/**` (and the staged file) at ≤ 4 Hz against the resolved size. This measures written bytes, not network throughput, and the UI labels it as an estimate. **Pause and resume:** `hf` 2.2.0 does **not** resume across runs. Each run writes a new UUID-suffixed `.incomplete` file and deletes it on handled failure *(verified against upstream source)*. So for this engine, **pause = stop**, and **resume, app restart and network-failure retries restart the file from 0** (owner-accepted trade-off). The UI MUST show "will restart from 0%" on pause and resume actions for these jobs. Orphaned partials are deleted on stop and at startup. **Cancel:** kill the process tree and delete the staging dir. **Errors:** classify from stderr and pre-flight API checks: auth/gated, not found, disk full, network. Retries follow FR-DL-12. |
| FR-DL-3b | P0 | **HF native engine** (reqwest, with Range resume per FR-DL-8). It is used: (1) when the user selects it in Settings ("HF download engine: hf CLI (default) or Native"); (2) as an automatic fallback **only** when the `hf` tool is missing or fails to launch. It is **not** a fallback for auth, not-found, hash or other CLI errors. The fallback uses its own staging dir. The token is sent only to `huggingface.co` and its CDN hosts, and stripped on other redirects. |
| FR-DL-4 | P0 | **Google Drive** (owner decision 2026-10-20: use the `gdown` library). **URL forms:** the forms gdown accepts, on `drive.google.com` and `docs.google.com` (`file/d/{id}/view|edit`, `file/u/{n}/d/{id}`, `open?id=`, `uc?id=`) and on `drive.usercontent.google.com` (`download?id=`). Folder links (`/folders/`, `embeddedfolderview`) give the folder error [folders are P2]. Docs, Sheets and Slides editor links and links with several `id` values are rejected. **Engine:** `gdown` 6.4.2 in the app-owned, hash-locked tool venv `tools/gdown/` (§2.3), run with that venv's Python through a small driver script (`python -I -B -X utf8 -c`). The driver calls `gdown.download` with cookies disabled and reports byte progress, the file name and a classified error as tagged JSON lines. It runs in a private home and temp folder (`data/cache/gdown/home`, `data/tmp/gdown`). Its traffic passes through the same attempt-scoped bandwidth relay as the hf CLI (FR-DL-14), restricted to Google hosts (`google.com`, `*.google.com`, `*.googleusercontent.com`), so the shared live cap applies. **Resolve:** the file name comes from gdown without downloading the file; the size is learned from the first progress sample. **Progress** is exact (network bytes). **Pause and resume:** like the hf CLI, **pause = stop**, and resume, app restart and retries restart the file from 0 (the UI shows "will restart from 0%"); gdown reconnects only inside one run. Staging is `data/downloads/gdrive/<job-id>/`, then the durable publication workflow (FR-DL-9). **Errors:** quota, permission (gdown cannot tell a missing file from an unshared one, so the message covers both), network, service and disk each map to a distinct error. **Fallback:** the native Drive path (confirmation-page parsing through `drive.usercontent.google.com`) is used only when the `gdown` tool is missing or fails to launch. gdown 6.x always applies its fuzzy URL handling. This replaces the reference's `gdown --fuzzy` *(reference)*. |
| FR-DL-5 | P0 | **Direct HTTPS.** Any https URL. Filename comes from `Content-Disposition`, then the URL path, then the user. Plain `http://` requires explicit confirmation. |
| FR-DL-6 | P0 | **Queue.** Persistent job queue in SQLite, which survives restarts. This fixes the reference's in-memory history. Concurrency is configurable, default 3 (the reference used 5). Per-job states: `queued`, `resolving`, `downloading`, `verifying`, `paused`, `completed`, `failed`, `cancelled`. |
| FR-DL-7 | P0 | **Progress.** Bytes done/total, speed and ETA per job, pushed to the UI at ≤ 4 Hz through a Tauri Channel. The reference had none. Engines that report written bytes instead of network bytes (hf CLI, FR-DL-3a) mark progress as *estimated*. |
| FR-DL-8 | P0 | **Cancel, pause and resume** (native engines; for the hf CLI engine see FR-DL-3a, for the gdown engine FR-DL-4). Data is written to `data/downloads/<job>.part`. Resume uses `Range` plus `If-Range` (ETag/Last-Modified) when the server supports it. Otherwise it restarts cleanly. CivitAI signed URLs expire, so resume re-resolves the URL. Cancel deletes the `.part` file. |
| FR-DL-9 | P0 | **Atomic finish.** Write to `.part`, fsync, verify, then atomically rename into the target folder. Staging MUST be on the same volume as the target, or fall back to copy + fsync + rename. A failed hash leaves no file in the models folder. **Durable finalization.** Completion is a recorded multi-phase workflow: `verified → published (renamed into place) → cataloged (idempotent provenance upsert) → completed`. Only then is `DownloadCompleted` emitted. The filesystem and SQLite can't share a transaction, so each phase is persisted, and startup reconciliation resumes any job interrupted between phases. Catalog registration is required work, not a best-effort listener. |
| FR-DL-10 | P0 | **Dedupe.** Before queuing: if a job with the same provider identity (CivitAI fileId, HF repo@rev:path, Drive id, URL) is active, reject it as a duplicate. If the catalog already has a file with the expected SHA-256, offer *skip* / *download again* *(reference status values `duplicated`, `already_downloaded`)*. |
| FR-DL-11 | P0 | **Name conflicts.** If the target filename exists, ask: overwrite (moves the old file to the recycle bin), keep both (`name (1).ext`), or skip. |
| FR-DL-12 | P0 | **Retries.** Up to 3 automatic retries on network or 5xx errors, with exponential backoff. 4xx errors are not retried, except a 403 on an expired signed URL, which triggers re-resolve. |
| FR-DL-13 | P0 | **Model packs.** Import the reference **resource_list** format for compatibility. It is a JSON array of `{name, url, type}`, where `type` is the category *(reference)*. Accept a pack index `{category: [{name, url}]}` whose `url`s point to resource lists *(reference: `comfy_model_packs.json`)*. Show a preview with sizes, let the user deselect items, then queue them. Export the current history or selection as resource_list JSON. |
| FR-DL-14 | P1 | **Bandwidth limit.** Optional global speed cap. |
| FR-DL-15 | P1 | Hugging Face Xet-backed files download through the `hf` CLI engine (bundled `hf-xet`). The native engine's Xet compatibility is *(to validate)*; if it can't fetch a Xet file, it MUST error clearly and suggest the hf engine. |

### 4.5 Model library (FR-MODEL)

| ID | Pri | Requirement |
|---|---|---|
| FR-MODEL-1 | P0 | **Scan and list.** Scan `ComfyUI/models/<category>/**` and the paths in `extra_model_paths.yaml`. Show name, category, relative subfolder, size, modified date and source (if downloaded by the app). Search, category filter and sort are required. The scan runs incrementally on demand and on file-watch events. |
| FR-MODEL-2 | P0 | **Rename.** Rename the filename within the same folder; the extension is preserved by default, and changing it requires a warning. Validate per NFR-SEC-5. A case-only rename must work on Windows. Fail clearly if the target exists, or if the file is locked (ComfyUI running), with a *Retry* or *Stop ComfyUI and retry* option. Update the catalog and any sidecar files with the same stem (`.preview.png`, `.civitai.info`, `.json`). |
| FR-MODEL-3 | P0 | **Delete.** Single or multiple selection, with confirmation showing the total size. Files go to the OS recycle bin/trash by default. If trash fails, the app MUST report it and offer *permanent delete* as a separate explicit action. It never falls back to permanent delete silently. If a file is locked, report it and offer *retry*/*stop ComfyUI*. The catalog entry is removed only after the delete succeeds. |
| FR-MODEL-4 | P1 | **Move** to another category or subfolder (same-volume rename, else copy + verify + delete). |
| FR-MODEL-5 | P1 | **Identify.** Hash local files on demand (SHA-256, cached by path + size + mtime) and look them up on CivitAI with `GET /api/v1/model-versions/by-hash/{sha256}`. Store the model name, base model, trigger words and preview image as sidecar files. |
| FR-MODEL-6 | P2 | Disk-usage view per category; duplicate-file finder by hash. |
| FR-MODEL-7 | P1 | **Open in the system file manager.** The Models page opens `ComfyUI/models`; the details pane reveals the selected file. Targets are resolved from validated, pinned roots, never from UI strings. |

### 4.6 Category mapping
Categories are the ComfyUI `models/` subfolders:
- `checkpoints`
- `diffusion_models` (alias `unet`)
- `text_encoders` (alias `clip`)
- `vae`
- `loras`
- `controlnet`
- `clip_vision`
- `embeddings`
- `upscale_models`
- `latent_upscale_models`
- `style_models`
- `hypernetworks`
- `photomaker`
- `gligen`
- `audio_encoders`
- `model_patches`
- `vae_approx`
- `diffusers`

The list is also extensible by any folder present on disk. Unknown categories from a resource_list are accepted after name validation (NFR-SEC-5).

> Note: the reference backend rewrites `checkpoints`→`ckpts` for its RunPod layout *(reference)*. This app MUST NOT do that; it uses ComfyUI's native names.

### 4.7 Custom nodes (FR-NODE)

| ID | Pri | Requirement |
|---|---|---|
| FR-NODE-1 | P0 | **Registry browse.** Page through the Comfy Registry with `GET https://api.comfy.org/nodes?page=&limit=`, and search with `GET /nodes/search?search=` *(observed: ~6,000 nodes)*. Cache results locally (default TTL 6 h, manual refresh). Show: name, publisher, description, downloads, GitHub stars, latest version, repository link, `supported_os`, `supported_accelerators`, `supported_comfyui_version`, and status/deprecated flags. |
| FR-NODE-2 | P0 | **Install from the registry.** `GET /nodes/{id}/install` (optionally `?version=`) returns `downloadUrl` (a cdn.comfy.org zip) and `dependencies` *(observed)*. Download to staging, extract with hardened checks (NFR-SEC-4) into a temp folder in `custom_nodes/`, then atomically rename to the final folder name. Record the node id and exact version. Warn when OS/accelerator/ComfyUI version metadata says the node is incompatible, or when its status is flagged or deprecated. |
| FR-NODE-3 | P0 | **Install by git.** Any `https://` git URL, optionally with a branch, tag or commit. The clone uses `-c core.longpaths=true` on Windows, as the reference does. The folder name comes from the repo name and is validated. A registry node MAY be installed via git when it has a `repository` (user choice: *registry release* vs *git latest*). |
| FR-NODE-4 | P0 | **Dependencies.** After publishing the node, run an approved root `install.py` with the env's Python **before** reviewed requirements; this order applies to installs, updates and dependency retries, including consented curated setup. Script execution requires NFR-SEC-3 consent. Validate and freeze reviewed requirements before the hook. Reassert protected pins after each mutation, including hook/install failure or cancellation. After a successful hook, restore protected-pin and profile constraints, then preview the same guarded reviewed requirements bytes and frozen exact package versions against the post-hook environment; protected drift or preflight failure stops before requirements installation. Do not discover hook-generated requirements when the reviewed source had none. App-managed resolution and install remain wheel-only; arbitrary approved hooks can invoke their own installers/builds. Final pin assertion, environment verification and provenance/containment validation must succeed. Partial failure leaves the node installed but marked **deps-failed**, and the env is marked **repair_required**, with log, Retry and Repair. The app does not snapshot the venv; see §6.2. |
| FR-NODE-5 | P0 | **List installed.** Scan `custom_nodes/` and classify each entry: registry (with version), git (remote, branch, commit, dirty flag), or unmanaged/manual. Disabled nodes are included. |
| FR-NODE-6 | P0 | **Update.** Registry nodes: install the newer version in staging, then swap; the old folder goes to trash. Git nodes: `fetch` then `pull --ff-only`. Dirty or diverged repos MUST NOT be touched; show status instead. "Update all" is a P1 batch. |
| FR-NODE-7 | P0 | **Enable/disable.** Move the folder to `custom_nodes/.disabled/<name>`, the convention ComfyUI-Manager also uses, so both tools agree. |
| FR-NODE-8 | P0 | **Uninstall.** Move the folder to trash. Python packages are left in place, with a note that they are shared. |
| FR-NODE-9 | P0 | All node operations are serialized with env operations (FR-ENV-4) and require ComfyUI to be stopped. After success, offer "Restart ComfyUI". |
| FR-NODE-10 | P1 | **ComfyUI-Manager coexistence.** Installing ComfyUI-Manager is optional; first-run setup ticks it by default (owner decision 2026-10-20). Current ComfyUI supports `manager_requirements.txt` and `--enable-manager`. If the Manager is present, the app re-scans `custom_nodes/` after every ComfyUI stop, so changes made through the Manager are reflected. |
| FR-NODE-11 | P2 | Import/export a node set (a list of registry ids/versions and git URLs/commits) to reproduce an install. |
| FR-NODE-12 | P1 | **Open in the system file manager.** The Custom Nodes page opens `ComfyUI/custom_nodes`; the details pane opens the node's folder (under `.disabled` when disabled) or reveals a single-file node. Nothing is created. |

### 4.8 Settings and secrets (FR-SET)

| ID | Pri | Requirement |
|---|---|---|
| FR-SET-1 | P0 | Store the CivitAI and Hugging Face tokens in the OS keyring. If the keyring is unavailable, keep them in memory for the session only and warn the user. Plaintext storage is never used. Tokens are never sent in URLs or query strings. The reference sends them in a query string (`PUT /api/update_env/…?value=`) *(reference)*; this app MUST NOT. |
| FR-SET-2 | P0 | Each token has a *Test* button: CivitAI `GET /api/v1/me` or an equivalent authenticated call; HF `GET /api/whoami-v2`. The UI shows only whether a token is set and when it was last tested. The value is never read back to the UI. |
| FR-SET-3 | P0 | Settings for: install root (read-only after setup; change via re-setup), download concurrency, bandwidth cap, HF download engine (`hf` CLI default / Native, FR-DL-3a/3b), ComfyUI launch options, close behavior, theme (system/light/dark), interface language (English/Thai), registry cache TTL. |
| FR-SET-4 | P1 | App self-update through `tauri-plugin-updater`, with signed update artifacts. |

### 4.9 Input/output gallery (FR-GALLERY)

| ID | Pri | Requirement |
|---|---|---|
| FR-GALLERY-1 | P1 | **Browse.** A Gallery page lists `ComfyUI/input` and `ComfyUI/output` (fixed; the input/output launch arguments are reserved) one folder at a time: folders first, then files, with name, size and modified time. Links, reparse points and special files are never followed; they and unsafe names are reported as warnings. `ComfyUI` itself is never created; a missing area folder is created only once `ComfyUI` exists. |
| FR-GALLERY-2 | P1 | **Preview.** Images (png, jpg, jpeg, webp, gif, bmp, avif), videos (mp4, webm, mov, m4v; decoding depends on the webview) and 3D models (glb, gltf, obj, fbx, stl, ply; compressed glTF extensions are not supported) open in a preview dialog. Files are served by the app's `gallery` URI scheme from pinned handles: manager webview only, GET/HEAD/OPTIONS, single byte ranges (≤ 8 MiB per response), full bodies ≤ 256 MiB, sanitized errors. The Tauri asset protocol stays disabled. 3D loaders may only fetch from the same gallery area. |
| FR-GALLERY-3 | P1 | **Rename, move, copy.** Files and folders, at most 500 items per batch, while ComfyUI runs. Rename fails if the target exists (case-only renames are a single rename, verified). Move and copy never overwrite or merge: a conflicting name gets ` (n)` (n ≤ 999). Moving or copying an item into itself is refused. Copies and cross-volume moves stage in `<area>/.vjump-gallery-staging/<op>/` (reserved name, owner marker), verify size and SHA-256, publish without replacement, and only then remove the source; source entries that changed meanwhile are kept and reported as `source_retained`. Mutations are serialized by the maintenance gate. |
| FR-GALLERY-4 | P1 | **Delete.** A delete plan (count, total size, identity and full-tree fingerprint) is confirmed first; execution re-verifies it and moves items to the Recycle Bin/Trash only. There is no permanent-delete fallback. |
| FR-GALLERY-5 | P1 | **Open in the system file manager** for the current folder or the selected item. |

---

## 5. Data and contracts

### 5.1 IPC surface (non-normative)
Commands (invoke):
- `setup_detect_hardware`, `setup_validate_python`, `setup_list_profiles`, `setup_run`, `setup_cancel`
- `env_info`, `env_packages`, `env_pip`
- `comfy_start`, `comfy_stop`, `comfy_status`, `comfy_update`, `comfy_list_versions`
- `dl_resolve`, `dl_enqueue`, `dl_pause`, `dl_resume`, `dl_cancel`, `dl_import_pack`, `dl_export`
- `model_list`, `model_rename`, `model_delete`, `model_move`, `model_identify`
- `node_registry_search`, `node_install_registry`, `node_install_git`, `node_list`, `node_update`, `node_set_enabled`, `node_uninstall`
- `secret_set`, `secret_clear`, `secret_test`
- `settings_get`, `settings_set`

Push channels/events:
- `job:progress`
- `job:state`
- `comfy:log`
- `comfy:state`
- `setup:step`

Mapping from the reference API:
| Reference endpoint | Replacement |
|---|---|
| `POST /api/download_custom_model` | `dl_resolve` + `dl_enqueue` |
| `POST /api/import_models` | `dl_import_pack` |
| `POST /api/download_selected` | `dl_import_pack` (pack index) |
| `GET /api/download_history` | `dl_subscribe` (its first event is a snapshot of the persisted queue and history) |
| `PUT /api/update_env/*` | `secret_set` |
| `POST /api/install_custom_node` | `node_install_git` / `node_install_registry` |
| `POST /api/restart` | `comfy_stop` + `comfy_start` |
| `GET /api/logs` + `WS logs` | `comfy:log` |
| `GET /api/checkcuda` | `setup_detect_hardware` / `env_info` |

### 5.2 SQLite tables (non-normative)
- `jobs(id, kind, provider, source_ref_json, target_dir, filename, state, bytes_done, bytes_total, expected_sha256, verified, error, created_at, updated_at)`. Signed URLs and tokens are never stored.
- `models(path, category, size, mtime, sha256, source_job_id, civitai_version_id, meta_json)`
- `nodes(folder, kind{registry,git,manual}, registry_id, version, git_remote, git_commit, enabled, deps_state, installed_at)`
- `envs(id, path, owned, python_version, torch_version, cuda_variant, profile_id, created_at, active)`
- `registry_cache(key, json, fetched_at)`

### 5.3 resource_list compatibility format
```json
[
  { "name": "flux1-dev-fp8.safetensors", "url": "https://huggingface.co/.../resolve/main/flux1-dev-fp8.safetensors", "type": "checkpoints" }
]
```
The pack index (`{ "<Category>": [ { "name": "...", "url": "<resource_list url>" } ] }`) is supported as documented in the reference `resources/comfy_model_packs.json` *(reference)*.

### 5.4 Setup profiles
A profile is a versioned JSON document. Profiles ship **bundled in the app** for release 1. Each profile specifies:
- id, label, OS;
- Python version (exact patch);
- torch/torchvision/torchaudio versions and index URL;
- minimum NVIDIA driver and minimum/maximum compute capability;
- triton package and version range;
- the optional accelerator list. Each item is a package spec or a direct wheel URL with a **pinned SHA-256**, plus a non-fatal flag;
- extra constraints (e.g. `numpy<2.3`);
- verification snippet expectations.

Initial profiles to define and validate *(to validate)*:
1. `win-py312-torch2100-cu130` (default, Windows; the reference installer's stack; owner decision 2026-10-20, verified on an RTX 3060)
2. `linux-py313-torch2141-cu130` (default, Linux)
3. `win-py313-torch2141-cu130` (Windows alternative; the only profile with a calibrated disk estimate)
4. `*-py313-torch2141-cu126` for GPUs or drivers without CUDA 13 support
5. `win-py312-torch2100-cpu` and `linux-py313-torch2141-cpu` (CPU-only testing profiles: experimental, never recommended or preselected, no GPU or driver gate; ComfyUI runs with `--cpu`)

Remote profile updates are **P2**. If added, they MUST be signed, with the signature verified by a key embedded in the app, and they MUST carry artifact hashes and version-rollback protection.

---

## 6. Non-functional requirements

### 6.1 Security (NFR-SEC)
| ID | Requirement |
|---|---|
| NFR-SEC-1 | **Secret redaction.** Tokens, `Authorization` headers and signed URL query strings (`X-Amz-*`, `token=`) MUST be redacted from logs, error messages, the job DB, exports and crash reports. |
| NFR-SEC-2 | **Credential scoping.** Auth headers are attached only to the provider's own origins and are stripped on cross-origin redirects. Tokens are never put in the environment of ComfyUI, `install.py`, or uv/git subprocesses, and never in the app's own process environment. **Single exception:** the pinned `hf` downloader child (FR-DL-3a) receives `HF_TOKEN` in its spawn environment only. It is never passed as an argument, the parent environment is never modified, and the child's output is redacted. Subprocesses get a minimal allow-listed environment. |
| NFR-SEC-3 | **Untrusted code consent.** Installing a custom node, running `install.py` or building a source package executes third-party code, and a venv is not a sandbox. Before first execution, the UI MUST say this, show the source (registry id@version or git URL@commit) and require confirmation. A "trust this source" option may be remembered per repository. **First-run setup:** the review step states this risk and lists the sources (ComfyUI-Manager and each selected curated repository, latest commit of its default branch); starting setup is the confirmation and setup does not ask again. The disclosure states that selected curated nodes may ship Python cache files and top-level native programs/libraries, run `install.py` if present, then install reviewed requirements. Only the two content refusals are relaxed for persisted consent to selected curated Git sources; all containment and preflight checks remain. Each full URL@commit is logged before installation and each finding is logged as acknowledged. Source/script trust is not remembered and app-managed resolution/install stays wheel-only, while arbitrary approved scripts may invoke builds. Manager uses its separate source/requirements path. Repair and the Custom Nodes page keep per-plan review; the page keeps every content refusal and runs approved hooks before requirements. |
| NFR-SEC-4 | **Archive extraction.** Reject entries that are absolute, drive-relative (`C:foo`), UNC, contain `..`, are symlinks or reparse points, contain alternate data streams (`:`), or use Windows reserved names (`CON`, `NUL`, …). Also reject entries whose normalized paths collide case-insensitively. Enforce caps on entry count (e.g. 20k), total uncompressed size (e.g. 2 GiB) and compression ratio. Always extract into a fresh staging dir and verify containment after canonicalization. |
| NFR-SEC-5 | **Path validation** for filenames, categories and folder names from users or remote sources. Rules: no separators, no `..`, no reserved names, no trailing dot or space (Windows), no control characters, max 255 bytes per component. The final canonical path MUST be inside an **authorized root**: the install root, a configured `extra_model_paths.yaml` root, or a user-selected custom venv path. Domain value objects validate names only. Filesystem adapters MUST check real containment at operation time: existing parents, symlinks/reparse points, and races between check and use. |
| NFR-SEC-6 | **Webview hardening.** The manager webview uses a strict CSP and loads only bundled assets. Custom Tauri commands are reachable from every window by default, so they MUST be declared through explicit app-command permissions granted only to the manager window's capability. ComfyUI is loaded in a separately labelled webview with no capability. A test asserts that `invoke` from it is denied. The ComfyUI webview has no capabilities. External links open in the system browser. Custom URI schemes are not covered by capabilities, so the `gallery` scheme (FR-GALLERY-2) checks the requesting webview label itself and answers only the manager window; the CSP admits it only for images, media and connections. |
| NFR-SEC-7 | **Bundled tool integrity.** Pinned SHA-256 for uv, MinGit, Python archives, direct wheel URLs and every artifact in the `hf` and `gdown` tool locks (`--require-hashes`), as the reference installer does *(reference)*. Downloads use HTTPS only. |
| NFR-SEC-8 | ComfyUI listens on `127.0.0.1` by default. Exposing it on the LAN requires an explicit setting and a warning. |

### 6.2 Reliability
- Every long operation (setup step, download, node install, update) is resumable or safely retryable.
- **File placement is atomic.** Downloads, registry node folders and node updates are staged, then renamed into place. Env setup and activation are side-by-side, switching only after verification.
- **In-place dependency changes are not rolled back** (owner decision, 2026-10-15). A failed `uv pip` or `install.py` step inside the active env marks it `repair_required`, shown in the UI with Retry and Repair. Repair reinstalls the profile and node requirements under the pin guard. The app does not snapshot venvs, and it cannot undo effects of arbitrary scripts outside managed state.
- Git operations never reset, force or discard local changes.
- On startup, the app recovers from a crash or kill: interrupted jobs return to `paused`, stale `.part` files are reconciled, and staging dirs are cleaned.

### 6.3 Windows and Linux specifics
- **Windows.**
  - The WebView2 Evergreen runtime is installed by the bundle's bootstrapper if it is missing.
  - The app MUST handle Unicode and spaces in all paths.
  - Detect whether `LongPathsEnabled` is off and warn. Git `core.longpaths` alone does not cover Python, so the install root should be short.
  - Locked files are handled per FR-MODEL-2/3.
- **Linux.**
  - Bundles are AppImage and .deb. Runtime needs `libwebkit2gtk-4.1`.
  - Uses system git and Secret Service (the keyring falls back to session memory).
  - Trash follows the freedesktop spec.
  - WebKitGTK's GPU and canvas performance for the ComfyUI frontend is *(to validate)*. If it is inadequate, "Open in system browser" becomes the default on Linux.
- **Both.** One instance only (`tauri-plugin-single-instance`). A second launch focuses the existing window.

### 6.4 Performance
- UI interactions stay < 100 ms. Long work runs on tokio tasks or blocking threads, never on the UI thread.
- Model library scans of 2,000 files complete in < 2 s when cached (no hashing).
- Download throughput reaches ≥ 90 % of a single-stream `curl` on the same URL. Multi-connection downloads are [P2].

### 6.5 UX and design (NFR-UX)
| ID | Requirement |
|---|---|
| NFR-UX-1 | The manager UI follows [docs/DESIGN.md](DESIGN.md): tokens, typography, components, status-chip mapping, motion and accessibility rules. New tokens or patterns require updating DESIGN.md first. |
| NFR-UX-2 | Release 1 manager UI copy MUST be available in **English and Thai**, selectable at the top of the first setup step (Install root) and in Settings, persisted across launches, with **English** as the default. Backend diagnostic text and logs remain English. Fonts MUST still render Thai glyphs in UI copy, names and paths (DESIGN.md §3). |
| NFR-UX-3 | Fonts and icons are bundled. The UI renders correctly offline under the production CSP in both themes, on WebView2 and WebKitGTK. |
| NFR-UX-4 | WCAG 2.1 AA contrast for text and controls in both themes. Status is never conveyed by color alone. Full keyboard operation. Usable at 200 % zoom. |

### 6.6 Maintainability (NFR-MAINT)
| ID | Requirement |
|---|---|
| NFR-MAINT-1 | The code follows §3.3. CI enforces the dependency rule, frontend boundaries and binding drift (§3.3.5). |
| NFR-MAINT-2 | A new download provider is added by implementing `SourceResolver` / `Downloader` ports in `infrastructure` and registering them in the composition root. No domain or UI-core changes are needed beyond the new `SourceRef` variant. |
| NFR-MAINT-3 | Domain and application crates build and test with no Tauri, network, database or OS GPU present. |
| NFR-MAINT-4 | Coverage targets: domain ≥ 90 % lines, application ≥ 80 % lines (`cargo llvm-cov`). |
| NFR-MAINT-5 | Every architectural deviation is recorded as an ADR in `docs/adr/`. |

### 6.7 Observability
- App log with rotation (`data/logs/app.log`), plus a separate log per ComfyUI session and per setup run.
- "Copy diagnostics" button: OS, GPU and driver, env info, versions, last 200 log lines — all redacted.

---

## 7. Milestones

| Milestone | Scope |
|---|---|
| M0 Skeleton | Cargo workspace (`domain`/`application`/`infrastructure`/`desktop`) + `ui/`. Tauri 2 + Vite/React/Tailwind/shadcn shell. tauri-specta bindings validated. Capability-scoped commands. SQLite. Settings. Keyring. Logging and redaction. CI on Windows and Linux with architecture enforcement (§3.3.5). |
| M1 Setup | Hardware detection, profiles, Python sources (managed/custom venv/custom interpreter), torch install + pin guard, ComfyUI clone, verification, re-setup with side-by-side envs. |
| M2 ComfyUI runtime | Start/stop/restart, logs, embedded webview, process cleanup, update with ff-only safety. |
| M3 Downloads | Queue, providers (CivitAI, HF, Drive, direct), progress, pause/resume/cancel, verify, dedupe, resource_list import/export. |
| M4 Library | Scan, rename, delete-to-trash, locked-file handling, `extra_model_paths.yaml`. |
| M5 Custom nodes | Registry browse/search/install, git install, deps with consent, update, enable/disable, uninstall. |
| M6 Release 1 | Optional accelerators, curated nodes, moved-install rebuild, app updater, packaging (MSI/NSIS, AppImage/.deb). |

---

## 8. Acceptance scenarios

1. **Clean setup (each OS).** Default profile on a fresh machine with a supported NVIDIA GPU → ComfyUI starts, `/system_stats` reports CUDA, and `import torch, triton, comfy` passes.
2. **Custom venv.** The user picks an empty folder → the venv is created there and marked app-owned. The user picks an existing venv → validated, marked adopted, and never deleted by re-setup.
3. **Re-setup: change torch.** The new env is built alongside the old one. If the build fails, the old env stays active and ComfyUI still starts. If it succeeds, it is activated and models, outputs and nodes are untouched.
4. **Moved install.** Copying the install root to another path → the app detects it, offers a rebuild, and after the rebuild ComfyUI starts with all models and nodes present.
5. **CivitAI.** A version URL with a valid token downloads, SHA-256 verifies, and lands in the suggested category. Without a token, a gated model shows a "token required" error. Killing the network at 50 % and resuming finishes with a verified file.
6. **Hugging Face (hf CLI default).** A `/blob/` URL in a gated repo without license acceptance gives a clear error with a link. An LFS file downloads through `hf` and is verified via `X-Linked-ETag`. A Xet-backed file downloads and publishes. Pausing shows "will restart from 0%", and resume restarts and completes. Logs, the DB and diagnostics contain no token. With the `hf` tool deleted, the job falls back to the native engine and resumes after a network cut.
7. **Google Drive.** A >100 MB public file goes through the confirmation page and downloads. A file over quota gives the quota error.
8. **Rename/delete while ComfyUI is running** with the model loaded (Windows) → a locked-file error, then *Stop ComfyUI and retry* succeeds. Deleted files appear in the recycle bin.
9. **Path attacks.** A rename to `..\..\evil.dll`, `CON.safetensors` or a name with `:` is rejected. A crafted `node.zip` with traversal, symlink or oversize entries is rejected with nothing written outside staging.
10. **Registry node** install shows a consent prompt, records id@version, and installs deps with torch unchanged. A requirement that asks for a different torch is blocked by the constraint and reported.
11. **Git node with local edits** → update refuses and shows `git status`. Enable/disable round-trips through `.disabled/`.
12. **Secret redaction.** After a CivitAI download, grep logs, the DB and exports for the token and the `X-Amz-Signature` value → zero matches.
13. **Unsupported hardware.** No NVIDIA GPU, or a driver too old for cu130 → CUDA profiles are disabled with the reason, and the cu126 profile is offered where eligible. The CPU testing profiles stay available.
14. **Crash recovery.** Kill the app during a download and a node install → on restart the download is `paused` and resumable, and the node staging dir is cleaned up with `custom_nodes/` unchanged.
15. **Architecture guard.** Adding `serde`, `tokio` or `tauri` to `domain` or `application`, or importing `features/downloads` internals from `features/custom-nodes`, fails CI.
16. **Maintenance race.** While a node install holds the maintenance gate, *Start ComfyUI* waits or is refused. A model rename during a running ComfyUI succeeds if the file is not locked.
17. **Failed dependency install.** A node whose `requirements.txt` fails leaves the env `repair_required`. Repair restores a verifiable env with torch unchanged.

---

## 9. Open items
Status as of 2026-10-20. Evidence IDs refer to [RELEASE_ACCEPTANCE.md](RELEASE_ACCEPTANCE.md).

Resolved:
- **Xet-backed files through the hf CLI engine (FR-DL-15, first half).** Verified natively on Windows: a live `hf` 2.2.0 download without user-profile variables (N23) and an in-app download of a public Xet-backed file, 334,641,190 bytes, SHA-256 verified (N24). Before the D4 fix every `hf` download failed on Windows.
- **Windows default stack.** `win-py312-torch2100-cu130` (Python 3.12.15, torch 2.10.0 cu130, torchaudio 2.10.0) verified natively on an RTX 3060 with ComfyUI 0.42.1 on `cuda:0` (N5, N11, N13, N28).

Open:
- Linux compiles and its automated tests pass in a Debian container (2026-10-20), but the app has never run on a Linux desktop or GPU: the Linux default profile, Secret Service, freedesktop trash on a real desktop, WebKitGTK and the AppImage/.deb bundles lack evidence.
- Exact accelerator wheel sources and SHA-256 per profile (SageAttention, FlashAttention 2/3, onnxruntime-gpu CUDA 13, llama-cpp-python) for Python 3.13 / torch 2.14.1 on both OSes *(to validate)*. The py312 / torch 2.10 / cu130 profile now pins five Windows wheels by SHA-256, but none has been installed or imported on real hardware, so they stay experimental and unchecked. FlashAttention 3's import module name is unverified.
- Whether the native HF engine can fetch Xet-backed files (FR-DL-15, second half). The hf CLI engine covers them.
- Whether a later `hf`/`huggingface_hub` release restores cross-run resume. If so, re-evaluate FR-DL-3a pause semantics.
- Thai font choice and rendering on WebKitGTK (DESIGN.md §3).
- WebKitGTK suitability for the ComfyUI canvas on Linux (FR-COMFY-3).
- Code signing for Windows (Authenticode) and the update-signing key custody (FR-SET-4). The built bundles are unsigned and the updater is inert.
- Provider-token acceptance: CivitAI (scenario 5), gated Hugging Face (scenario 6), Google Drive confirmation and quota (scenario 7) and the redaction check (scenario 12) need test credentials or fixtures.
- Windows installer execution (NSIS/MSI, WebView2 bootstrapper, uninstall data preservation) on a disposable machine.

## 10. Sources (observed 2026-10-15)
- crates.io API — versions of tauri, wry, tao, reqwest, tokio, rusqlite, keyring, trash, zip, sha2, nvml-wrapper, sysinfo, tauri-plugin-*.
- npm registry — @tauri-apps/cli, @tauri-apps/api, vite, react, tailwindcss, @tanstack/react-query.
- GitHub releases — tauri (v2.12.4 stable, v3.0.0-alpha.5), astral-sh/uv (0.13.1), Comfy-Org/ComfyUI (v0.40.0).
- PyPI / download.pytorch.org — torch 2.14.1; cu130 cp313 wheels for win_amd64 and manylinux_2_28_x86_64; triton 3.8.0; triton-windows 3.8.0.post29.
- python.org/ftp — 3.13.16, 3.14.8, 3.15.0.
- [docs.comfy.org system requirements](https://docs.comfy.org/installation/system_requirements) — Python 3.13 recommended, 3.12 fallback.
- [PyTorch dev-discuss](https://dev-discuss.pytorch.org/c/release-packaging/25) — CUDA 13.0 stable since 2.11; 2.14 is the last release with CUDA 12.x wheels; 2.15 drops Maxwell/Pascal/Volta.
- Comfy Registry live API — `https://api.comfy.org/nodes`, `/nodes/search`, `/nodes/{id}/install`.
- CivitAI live API — `/api/v1/model-versions/{id}`, `/api/download/models/{id}` (307 to a signed URL).
- Hugging Face live API — `/api/models/{repo}/tree/{rev}`, `/resolve/` (307, `X-Linked-ETag`).
