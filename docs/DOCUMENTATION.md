# vjumpkung-comfyui-desktop manual

This manual has two parts: a user guide (Part 1) and a developer guide (Part 2). For the normative requirements, see [REQUIREMENTS.md](REQUIREMENTS.md). For the project front page, see the [README](../README.md).

## How to read the status markers

This manual describes the behaviour of the code at the final session-6 checkpoint (2026-10-20). Every feature described here is implemented. Where it matters, the text says how far a feature has been checked:

- **Verified natively on Windows** means the real application ran it on Windows 11 with an NVIDIA GeForce RTX 3060 (driver 610.88). The evidence IDs (for example N24) refer to [docs/RELEASE_ACCEPTANCE.md](RELEASE_ACCEPTANCE.md).
- **Covered by automated tests** means Rust or Vitest tests exercise the behaviour, but no native run has been recorded.
- Nothing has been compiled or run on Linux. Linux statements describe the code and configuration only.

The release verdict is **NO-GO**: Linux, installer execution, provider-token scenarios, signing and other gates are open. [RELEASE_ACCEPTANCE.md](RELEASE_ACCEPTANCE.md) is the authoritative ledger, and [Known limitations and open validation items](#known-limitations-and-open-validation-items) summarises it.

## Table of contents

**[Part 1: User guide](#part-1-user-guide)**

1. [Installation](#1-installation)
2. [First-run setup wizard](#2-first-run-setup-wizard)
3. [Changing the environment later (re-setup)](#3-changing-the-environment-later-re-setup)
4. [Moved install and guided rebuild](#4-moved-install-and-guided-rebuild)
5. [Environment page](#5-environment-page)
6. [ComfyUI page](#6-comfyui-page)
7. [Downloads](#7-downloads)
8. [Model library](#8-model-library)
9. [Custom nodes](#9-custom-nodes)
10. [Settings](#10-settings)
11. [Troubleshooting and FAQ](#11-troubleshooting-and-faq)
12. [Data and privacy](#12-data-and-privacy)

**[Part 2: Developer guide](#part-2-developer-guide)**

13. [Architecture](#13-architecture)
14. [Project layout](#14-project-layout)
15. [Toolchain setup](#15-toolchain-setup)
16. [Build, run, test and lint](#16-build-run-test-and-lint)
17. [IPC and generated bindings](#17-ipc-and-generated-bindings)
18. [Capabilities, ACL and adding a command](#18-capabilities-acl-and-adding-a-command)
19. [Database migrations](#19-database-migrations)
20. [Setup profiles](#20-setup-profiles)
21. [Adding a download provider](#21-adding-a-download-provider)
22. [Testing strategy and coverage](#22-testing-strategy-and-coverage)
23. [Opt-in real-system tests](#23-opt-in-real-system-tests)
24. [Packaging and updater](#24-packaging-and-updater)
25. [Continuous integration status](#25-continuous-integration-status)
26. [Coding conventions](#26-coding-conventions)
27. [Security model](#27-security-model)
28. [Known limitations and open validation items](#known-limitations-and-open-validation-items)

---

# Part 1: User guide

The application window is titled **vjumpkung ComfyUI**. The left navigation has seven pages: **Overview**, **ComfyUI**, **Downloads**, **Models**, **Custom Nodes**, **Environment** and **Settings**. The top bar holds the ComfyUI runtime controls and a theme toggle.

| Page | Purpose |
|---|---|
| Overview | App version, data directory and which credential backend is in use |
| ComfyUI | Start, stop and restart ComfyUI; logs; launch options; updates |
| Downloads | The model download queue, import and export |
| Models | The model library: search, rename, move, delete, identify |
| Custom Nodes | Browse the Comfy Registry, install, update and manage nodes |
| Environment | The active Python environment, packages, re-setup |
| Settings | Preferences, tokens, re-setup shortcuts |

## 1. Installation

Release 1 targets **Windows 10/11 x64** and **Linux x64**, with **NVIDIA GPUs only**. There is no published release yet, so these instructions describe the bundles as configured.

### Windows

1. Run the NSIS installer (`vjumpkung-comfyui-desktop_<version>_x64-setup.exe`) or the MSI (`vjumpkung-comfyui-desktop_<version>_x64_en-US.msi`). The NSIS installer installs for the current user only.
2. If the WebView2 runtime is missing, the bundle downloads and installs it silently through the WebView2 bootstrapper.
3. Optional but recommended: enable Windows long paths (`LongPathsEnabled`) and pick a short install root. The wizard shows whether long paths are enabled.

A current NVIDIA driver is required. The driver's CUDA level decides which profiles are available (see [Profiles](#profiles)).

Both Windows installers were built locally on 2026-10-20 (`vjumpkung-comfyui-desktop_0.1.0_x64-setup.exe`, 8.90 MiB, and `vjumpkung-comfyui-desktop_0.1.0_x64_en-US.msi`, 12.72 MiB; hashes in [RELEASE_ACCEPTANCE.md](RELEASE_ACCEPTANCE.md)). They are unsigned and have **not** been run, so the WebView2 bootstrapper, installation and uninstallation are unverified. Expect an "unknown publisher" warning from Windows.

### Linux

1. Install the runtime dependencies. The `.deb` declares `libwebkit2gtk-4.1-0` and `libgtk-3-0`. The AppImage needs the same libraries on the host.
2. Install **Git 2.40 or newer** from your distribution. The application does not bundle Git on Linux. Without it, node and ComfyUI operations that need Git report that Git is unavailable.
3. For stored tokens to persist, a Secret Service provider (for example GNOME Keyring or KWallet) must be running. Without one, tokens are kept in memory for the current session only.
4. Install the `.deb` with your package manager, or mark the AppImage executable and run it.

Linux bundles have not been built or tested yet.

### Single instance

Only one copy of the application runs at a time. Starting it a second time focuses the existing window.

## 2. First-run setup wizard

When there is no valid installation, the application opens the setup wizard. It has six stages: **Install root, Hardware, Python source, Profile, Options, Review**. After the review, the installation runs and shows live progress. Before setup, the application creates only its `data` folder in the install root; nothing is created inside `ComfyUI/` until setup clones it (verified natively, N8, N26). After a successful first-time setup the application restarts itself once (N11, N28).

A complete first run on the recommended Windows profile with ComfyUI-Manager and 21 curated nodes took about 8.5 minutes on the test machine, with no pause, and every node imported on `cuda:0` (N11, N13).

### Install root

At the top of this first stage, choose **English** or **ไทย** in **Language**. The default is **English**. The interface switches immediately and saves your choice for later launches. If saving fails, it returns to the previous language. **Continue** is unavailable while the language is being saved. You can change the language later in Settings.

The install root is the folder that will hold ComfyUI, the Python environments, the bundled tools and the application's data. The default is `%LOCALAPPDATA%\vjumpkung-comfyui` on Windows and `~/.local/share/vjumpkung-comfyui` on Linux. Choose a short path with no unusual permissions. You can change the install root only here, or later by running setup again. If you save a different folder, the wizard offers **Restart now**, which restarts the application on the new install root with your settings and wizard choices carried over.

### Hardware check and profile gating

The Hardware stage detects:

- each NVIDIA GPU, its compute capability and its wheel architecture;
- the driver version and the highest CUDA level the driver supports;
- free disk space on the install volume;
- on Windows, whether long paths are enabled.

A profile is **disabled, with the reason shown**, when any of these hold: it is for another operating system; no NVIDIA GPU is found; any detected GPU's architecture is not supported by the profile's wheels; the driver does not support the profile's CUDA level; or there is not enough free disk for the profile's budget. The bundled profiles all require compute capability 8.0 or newer.

### Python source

| Source | Behaviour |
|---|---|
| **App-managed** (default) | The app installs a pinned CPython with uv and creates an app-owned venv under `<install root>/envs/`. |
| **Custom venv** | You pick a folder. An empty folder becomes a new app-owned venv. An existing venv is **adopted**: it is validated, never deleted by the app, and rebuilt only after you confirm. |
| **Custom interpreter** | You pick a `python` executable. The app creates an app-owned venv under `envs/` from it. |

For the last two, the chosen Python is validated before setup, and each check shows pass or fail. The interpreter must match the selected profile's Python minor version.

### Profiles

Seven profiles are bundled. All use PyTorch from `download.pytorch.org`, a Triton build and a verification step. The wizard lists the recommended profile for your OS first and preselects it (N25); the other eligible profiles follow.

| Profile | Python | PyTorch | CUDA | Status |
|---|---|---|---|---|
| `win-py312-torch2100-cu130` | 3.12.15 | 2.10.0 (+ torchvision 0.25.0, torchaudio 2.10.0) | 13.0 | Stable, recommended (default on Windows) |
| `win-py313-torch2141-cu130` | 3.13.16 | 2.14.1 | 13.0 | Stable, Windows alternative |
| `win-py313-torch2141-cu126` | 3.13.16 | 2.14.1 | 12.6 | Stable, for drivers without CUDA 13 |
| `win-py314-torch2141-cu130` | 3.14.8 | 2.14.1 | 13.0 | Experimental |
| `linux-py313-torch2141-cu130` | 3.13.16 | 2.14.1 | 13.0 | Stable, recommended (default on Linux) |
| `linux-py313-torch2141-cu126` | 3.13.16 | 2.14.1 | 12.6 | Stable |
| `linux-py314-torch2141-cu130` | 3.14.8 | 2.14.1 | 13.0 | Experimental |

"Stable" describes the profile's status label in the bundle, not a validation result on real hardware. On the RTX 3060 test machine:

- `win-py312-torch2100-cu130` (revision 2) was verified natively from the application on 2026-10-20: first-time setup, final verification and ComfyUI 0.42.1 on `cuda:0` with Python 3.12.15 and torch 2.10.0+cu130 (N5, N11, N13, N28). It is the Windows default by owner decision. Its disk estimate is not calibrated.
- `win-py313-torch2141-cu130` passed a direct setup test on 2026-10-16 and an application-driven first run in session 5 (Python 3.13.16, torch 2.14.1+cu130).
- The other profiles, and every Linux profile, have not been run.

Notes:

- The PyTorch 2.14.1 profiles install `torch` and `torchvision` only. No matching `torchaudio` wheel was published for 2.14.x when the profiles were prepared, so curated nodes that import `torchaudio` (for example ComfyUI-MelBandRoFormer and ComfyUI-WanVideoWrapper) fail to import on those profiles. The Python 3.12 profile installs `torchaudio` 2.10.0, and those nodes import on it (N13).
- Triton is `triton-windows` on Windows and `triton` on Linux, at the version the profile pins.
- Profiles with CUDA 12.6 require a driver that supports CUDA 12.6. Profiles with CUDA 13.0 require 13.0.

### Options

- **ComfyUI version.** Choose **Latest release** (the default), **Latest commit (master)** or **Specific tag** (N26). Latest commit (master) installs the newest commit on ComfyUI's `master` branch. This is development code and may be less stable than a release. The checkout tracks `origin/master`, so `git pull` and the app's **Fast-forward** update it. A release or tag is placed on the branch that contains it (`master`, or the `release/vX.Y` branch for a patch release; see [Updating ComfyUI](#updating-comfyui-and-the-version-picker)). For a specific tag, enter the exact tag; browsing tags is not offered in the wizard. After setup, the ComfyUI page has a version picker.
- **ComfyUI-Manager.** Optional. It is ticked by default in first-run setup (other setup modes leave it unticked). It is installed from the latest commit of its default branch. The Review stage states that this runs third-party code and lists the source; clicking **Start setup** is your confirmation, so first-run setup does not pause for consent and logs the approved commit. (Repair with ComfyUI-Manager still asks for consent.) When it is installed, you can launch ComfyUI with its built-in Manager enabled.
- **Clean package cache after setup.** Optional, off by default. The final step runs `uv cache clean` on the application's uv cache to save disk space.
- **Accelerators.** Each profile offers its own list. The Python 3.12 profile (revision 3) offers five optional accelerator wheels (SageAttention, llama-cpp-python, FlashAttention 2, FlashAttention 3 and onnxruntime-gpu) and a "Compatibility packages" component (`transformers`, `onnx`, `numpy`, `pillow` constraints). Every option shows its provenance and is labelled **experimental**. None is selected by default, and changing the profile resets the selection to the new profile's defaults. Each wheel is downloaded from its publisher and checked against a pinned SHA-256 before it is installed. A matching hash proves the file is the expected artifact. It does **not** prove the wheel works with your Python, torch, CUDA and GPU. Failure to install or import an accelerator is a warning, not a setup failure, and the result of each component is stored and shown on the Environment page. The other bundled profiles show "This profile offers no optional accelerators." Limitation: no accelerator has been installed on real hardware yet, and FlashAttention 3's import module name is unverified, so it has no import check.

- **Curated custom nodes** (first-run only). A list of 22 recommended nodes, mirroring the reference installer. Each is cloned at the latest commit of its repository's default branch (there is no commit lock) and tracks that branch afterwards. All 22, including ComfyUI-PlagueKind-Nodes, are selectable; none is ticked by default. The Review stage lists every selected repository and states that third-party code will run; clicking **Start setup** is your confirmation, so setup does not pause for a per-node review. Each full repository URL and resolved commit is logged before it is installed, and warnings are logged as acknowledged. Selected curated Git nodes may ship Python cache files (`__pycache__`) and top-level native programs or libraries; containment, provenance, Git restrictions and resource limits still apply. Setup runs a selected node's `install.py` if present, then installs its reviewed requirements. Source trust and script trust are never remembered, and app-managed installation remains wheel-only. These exceptions apply only to persisted consent for selected curated Git sources in FirstRun; non-consented setup and the Custom Nodes page retain their content refusals. Manager uses its separate source/requirements path.

### Review and install

The Review stage lists your choices. When a first run selects ComfyUI-Manager or curated nodes, a **Third-party code** notice lists every source URL and says:

> Installing ComfyUI-Manager and selected custom nodes runs third-party code on this computer; a Python environment is not a sandbox. Setup installs the latest commit of each repository’s default branch. For selected custom nodes, setup allows shipped Python cache files and top-level native programs or libraries, runs install.py if present, then installs the reviewed requirements.txt. It does not ask again. Clicking Start setup confirms this.

The button is **Start setup**. There is no consent checkbox and no later consent pause in a first run (verified natively, N10, N11, N28). When you start, the installation runs these steps in order, each idempotent on retry:

1. Preflight checks.
2. Fetch and verify the pinned tools: uv, MinGit (Windows only) and the isolated `hf` tool environment.
3. Create the Python environment.
4. Seed `pip`, `setuptools` and `wheel` (plus an app-local MSVC runtime on Windows).
5. Clone ComfyUI at the selected source, on a branch that tracks `origin` where possible.
6. Install `torch` and `torchvision` (and `torchaudio` where the profile lists it).
7. Verify the torch version and CUDA availability.
8. Install Triton.
9. Optional accelerators.
10. Install ComfyUI's `requirements.txt`.
11. Optional ComfyUI-Manager.
12. Optional curated nodes.
13. Final verification: import `torch`, `triton` and `comfy`, and run a CUDA operation.
14. Optional cache clean.

From step 6 onward, a **pin guard** passes a constraints file so that later installs cannot change `torch`, `torchvision`, `torchaudio` or Triton. The installed versions are re-checked after every dependency operation.

Setup does not stop if you close the window's progress channel. Cancelling ("Abort setup") stops the run and cleans up.

### When a step fails

A failed step shows its name, message, the failing command, its exit code and the last log lines. The previous working environment, if any, stays active. You can choose:

- **Retry same run**: re-run the failed step.
- **Skip optional step**: only offered when the failed step is optional (for example an accelerator).
- **Abort setup**: cancel and clean up.

A first-run setup carries your consent from the Review stage, so ComfyUI-Manager and the curated nodes install without a pause and Retry never waits for approval. In other modes, for example Repair with a recorded ComfyUI-Manager, the Retry button is held until you approve its source.

## 3. Changing the environment later (re-setup)

From **Settings** (and the Environment page) you can run setup again, with ComfyUI stopped. The modes are:

| Mode | What it does | Python rule |
|---|---|---|
| **Set up ComfyUI** | First run only | n/a |
| **Repair environment** | Re-verify and reinstall missing pieces | Same Python minor as the active environment |
| **Change torch / CUDA** | Install a different torch/CUDA build | Same Python minor |
| **Change Python** | New Python version in a new venv | A different Python minor |
| **Switch to custom venv** | Use your own venv or interpreter | Must match the profile's minor |

Re-setup starts at the Hardware stage. It keeps the current ComfyUI checkout and commit.

**Side-by-side activation.** A new environment is built next to the old one. It becomes the active environment only after verification passes. If the build fails, the old environment stays active and ComfyUI still starts. The previous environment is kept until you delete it from the Environment page. Models, `user/`, `input/`, `output/` and custom node folders are never touched.

**Custom nodes during re-setup.** The new environment reinstalls the dependency requirements of installed custom nodes, enabled and disabled, under the pin guard. If a node's requirements fail to install, the new environment is **not** activated and the old one stays active. This holds in every replacement mode. A repair-required environment always gets a full replacement. This is covered by automated tests (`replacement_environments_receive_existing_node_requirements_under_the_pin_guard`, `replacement_node_dependency_failure_never_activates`, `repair_required_always_builds_a_replacement`); no re-setup has been run natively yet.

## 4. Moved install and guided rebuild

This behaviour (FR-SETUP-12) is implemented and was verified natively on Windows on a `robocopy /E` copy of a working installation (11.2 GB): the copy was detected as moved, the rebuild succeeded in about 2 minutes, the application restarted itself and ComfyUI ran with all nodes on `cuda:0` (N19, N20). The variant where the original folder no longer exists, and the Linux symbolic-link case, have not been run natively.

A Python venv records absolute paths, so it stops working when you copy or move the install folder. Release 1 does not try to make a moved folder run unchanged. Instead:

- On launch, if the recorded install root differs from the current one, or the active venv's interpreter no longer works, the application shows a **Rebuild environment** screen (recorded root, profile, Python version and preserved data) and normal commands return `rebuild_required` until you finish it. A failed rebuild stays inspectable: **Retry same run** resumes it, and **Start fresh rebuild** starts again. Locating the original install from this screen is not offered.
- If the install location the application remembers is missing (`root_missing`) or cannot be accessed (`root_inaccessible`), the application shows **Install recovery required** with the recorded root and three actions: **Locate existing install** (you point to an existing install, and the application restarts using it), **Create new install** (an empty folder; the application restarts using it, and an empty root opens the setup wizard) and **Recheck installation**. Neither selection overwrites an existing installation. A folder that cannot be used is refused with `root_invalid`.
- A rebuild creates a **new app-owned venv** in the current root using the exact profile revision and component choices that were recorded at the original setup. It never mutates or deletes an adopted venv.
- The ComfyUI checkout is reused as is (no fetch, checkout or reset), and local edits are preserved. Requirements of enabled and disabled custom nodes are reinstalled. No `install.py` scripts run, and no newer node versions are fetched.
- The `hf` tool environment is recreated, because it also records absolute paths.
- Models, nodes, `user/`, `input/`, `output/`, external model roots and launch options are preserved. Paths stored in the database that point into the old root are rebased to the new root. Paths outside the old root are left alone.
- If the recorded profile revision is no longer available, the rebuild is refused with `profile_revision_unavailable`.
- If an earlier activation, ComfyUI update or node operation was left half-finished, the application preserves it and reports `relocation_recovery_required` instead of rebuilding.

Copy tools that follow links turn uv's Python version links (junctions on Windows, symbolic links on Linux) into real folders. uv recreates the link of every managed Python version on each install, so the tools step deletes every such copied folder inside the app's Python cache, `data/cache/uv-python` (for example both `cpython-3.12-…` and `cpython-3.13-…`), and uv creates the links again. Links are never followed or deleted by the app. On Windows this was verified on the copy above (N19 found a remaining copied 3.12 link, fixed; N20 passed). The managed Python is installed with `--no-registry`, so neither the original nor the rebuilt installation registers itself under `HKCU\Software\Python\Astral` (N20).

Other limits (residual R21 in the ledger): move detection needs a valid `install.json`; a copy whose manifest is missing or corrupt starts normally while its database still points at the old root. User-owned paths into the old root (for example a launch `--output-directory` or entries in `extra_model_paths.yaml`) are kept as they are.

## 5. Environment page

The Environment page shows:

- the active environment: Python version and path, profile, torch and CUDA build, Triton, ownership (app-owned or adopted) and the verification report;
- the **protected pins**: the exact `torch`, `torchvision`, `torchaudio` and Triton versions that the pin guard defends;
- installed packages, with search;
- package operations;
- re-setup actions;
- deletion of inactive environments.

A **Stored accelerator results** card shows the recorded outcome of each accelerator component for the environment (installed, failed or skipped, and whether the import test passed). When nothing was recorded it says so: older metadata may not record optional components, and an empty card does not establish which accelerators are installed.

The app's private Python interpreters are not registered with Windows, so the `py` launcher and IDEs do not list them. Development builds before this change registered them under `HKCU\Software\Python\Astral\CPython<version>`. If such an entry's `InstallPath` points into an install root's `data\cache\uv-python`, you can remove it with `reg delete "HKCU\Software\Python\Astral\CPython<version>" /f`.

### Installing, upgrading and uninstalling packages

Choose **Install**, **Upgrade** or **Uninstall**, enter package names and **Preview**. The preview shows what would be installed, upgraded or removed. The pin guard resolves the request with the protected packages held fixed.

If the resolver needs to change a protected package, the preview says so and you must confirm those exact changes. Without confirmation the change is not applied. Package operations require ComfyUI to be stopped. The application offers to stop it.

Package changes are not rolled back. If an operation fails partway, the environment is marked **repair required** (see [Troubleshooting](#repair_required)). The application records that intent before it starts a mutating install, and returns the environment to the verified state only after a successful post-install verification.

### Deleting an inactive environment

Under "Delete an inactive environment", type the environment's recorded ID and confirm by typing it again. Only inactive environments can be deleted. Adopted environments are never deleted by the application.

## 6. ComfyUI page

### Start, stop and restart

The page and the top bar have Start, Stop and Restart. ComfyUI runs as a child process that the application owns, started as `python -s main.py --listen 127.0.0.1 --port <port>` plus your options. The application waits for ComfyUI to become ready before it reports Running. Verified natively on Windows: ComfyUI 0.42.1 reported `cuda:0 NVIDIA GeForce RTX 3060` through `/system_stats`, and Stop freed the port and left no Python process (N5, N13; session 5).

Start first checks: custom-node recovery state, a leftover ComfyUI process, a pending update, the selected environment and the port. Start, Restart and Stop run their checks and the custom-node rescan under one maintenance lease, and start, stop, update and package operations are serialised, so only one runs at a time.

Start is refused with `node_recovery_required` ("interrupted custom-node work must be resolved before launch.") while an interrupted custom-node operation needs resolving. Stop reports `node_rescan_failed` ("ComfyUI stopped, but custom nodes could not be rescanned; retry Rescan.") if the node folders could not be re-scanned after ComfyUI stopped. While that post-stop rescan runs, it holds the maintenance lease, so closing the application right after Stop can be refused for a moment (see [Troubleshooting](#closing-is-refused-with-maintenance-is-in-progress)).

**Port conflicts.** If the port is in use, a dialog tells you which port is taken and offers a free port. If you accept, the new port is saved as your launch port.

### Logs

Live logs stream into the page with secrets redacted. You can clear the view. Each ComfyUI session also writes a log file under `data/logs/` (see [Data and privacy](#12-data-and-privacy)).

### Launch options

| Option | Notes |
|---|---|
| Port | Default 8188 |
| VRAM mode | e.g. low or high VRAM |
| Attention backend | e.g. SageAttention, where installed |
| Preview method | |
| Built-in Manager | Starts ComfyUI with its Manager enabled |
| Listen on LAN | Off by default. Turning it on requires acknowledging that ComfyUI has no login and that other machines on the network can run workflows and read served files |
| Extra arguments | Passed as separate tokens, not through a shell. Reserved options such as the port, listen address and model paths are rejected |

Options are validated when saved and apply the next time ComfyUI starts.

### Opening the ComfyUI interface

- **Open window** opens ComfyUI in a separate application window. That window can only show the local ComfyUI address (`http://127.0.0.1`). Same-site navigation stays in it, external web links open in your system browser, and unsafe addresses are blocked. The window has no access to the application's commands.
- **Open in browser** opens ComfyUI in your system browser. On Linux this is the fallback if the embedded WebKitGTK view is too slow for the ComfyUI canvas (not yet evaluated).

### Closing the application

The **Settings > Close behaviour** choice decides what happens when you close the window while ComfyUI runs:

| Choice | Result |
|---|---|
| Ask every time | A dialog offers Cancel, "Keep running in background" and "Stop ComfyUI and exit" |
| Stop ComfyUI and quit | ComfyUI's whole process tree is stopped |
| Keep ComfyUI running in the background | The application's windows are closed and ComfyUI keeps running headless. Launching the application again restores the manager and its controls |

### Updating ComfyUI and the version picker

The versions panel offers **Update to latest commit (git pull)** to switch to the latest commit on origin’s default branch, normally `master`. This is development code and may be less stable than a release. **Update to latest release** and **Switch to &lt;tag&gt;** select a release tag for checkout. **Fast-forward &lt;current branch&gt;** updates only the current branch from its upstream using fast-forward only. Source switching checks out the frozen commit with safe branch placement; it is not restricted to fast-forward updates. An update may stop a running ComfyUI, reinstall `requirements.txt` under the pin guard, and then verify the environment. If the checkout has local changes or has diverged, the update is refused, shows the Git status, and changes nothing. The application never resets or force-checks-out your files.

Switching to an older tag does **not** restore the older package set. The panel warns about this. A pending update must be completed or retried before ComfyUI can start.

ComfyUI is a normal git checkout that tracks `origin`, so `git pull` works. A `vX.Y.0` release is on `master`, so `git pull` brings the latest development code. A patch release such as v0.42.1 exists only on ComfyUI's `release/v0.42` branch, so `git pull` brings later v0.42.x patches only; use the app's version switch to move to a newer minor version. An install that is still detached (created by an older version of the app) is attached by switching to its current version in the Version card. Prefer the app's Fast-forward button: it reinstalls requirements with the torch pin guard. After a manual `git pull`, run Repair. Fresh installs: the app creates nothing inside the install folder's `ComfyUI` before setup clones it, and restarts once after first-time setup.

Verified natively on Windows: switching a detached v0.42.1 install to v0.42.1 attached it to `release/v0.42` and `git pull --ff-only` reported "Already up to date." (N18); **Update to latest commit (git pull)** moved it to `master` tracking `origin/master` and ComfyUI ran (N21); switching back to v0.42.1 returned it to `release/v0.42` and kept the local `master` (N22). The refusal of a checkout with local changes is covered by automated tests only.

### extra_model_paths.yaml

The page shows the model paths read from `ComfyUI/extra_model_paths.yaml`. The parser expands `~`, `$VAR`, `${VAR}` and Windows `%VAR%`, and resolves relative paths against the ComfyUI folder. The model library scans these folders too.

**Generate from a base folder.** Enter a model base folder (or use Browse) and press **Generate**. The app writes one section, `vjumpkung_desktop`, to `ComfyUI/extra_model_paths.yaml`:

- `base_path` is the folder you chose, and `is_default` is `false`, so `ComfyUI/models` stays ComfyUI's default location.
- One key per model folder, each pointing at `<name>/` inside the base folder: the standard categories (`checkpoints`, `diffusion_models`, `text_encoders`, `vae`, `loras`, `controlnet`, ...), `configs`, and every folder that already exists in `ComfyUI/models`. `text_encoders` also lists `clip/`, and `diffusion_models` also lists `unet/`, but only when those folders already exist in the base folder.
- Missing standard folders are created inside the base folder. Existing folders and links are left as they are, and nothing is ever deleted. A file with the name of a standard folder stops the operation.

Other sections of an existing file are kept, but the file is re-written, so its comments are lost. When the file was not written by the app (its first line is not the `# Managed by vjumpkung ComfyUI desktop` header), a copy is first saved as `data/backups/extra_model_paths-<YYYYMMDDTHHMMSSZ>.yaml` in the install folder. Backups are never placed beside the file, because that would make the ComfyUI checkout dirty and block app updates. Press **Generate** again to change the base folder; repeating it with the same folder does not make another backup. **Remove generated section** deletes only the `vjumpkung_desktop` section, and the file itself when no other section remains. Both actions rescan the model library.

The app refuses a file it cannot rewrite safely and shows the reason: a link, larger than 1 MiB, not UTF-8, invalid YAML, a top level that is not a mapping, or a section that uses a `<<` merge key. Edit or move the file, then retry. It also refuses a base folder that is not an absolute local or network path, that contains `$` or `%` or starts with `~` (ComfyUI expands them), that is the install folder or inside its `ComfyUI`, `envs`, `data` or `tools` folders, or that sits behind a link or junction.

Limitations: downloads started in the app still go to `ComfyUI/models`, not to the base folder. ComfyUI reads the file when it starts, so restart ComfyUI after a change; the page reminds you while ComfyUI is running.

Verified natively on Windows with the base folder `D:\vjc-s6\models ทดสอบ` (a space and Thai characters): the file got the header and the `vjumpkung_desktop` section with 27 keys, 18 folders were created and the ComfyUI checkout stayed clean (N15); ComfyUI logged each extra search path, `GET /models/loras` listed a test file in the base folder and the Models page showed it (N16); removing the section deleted the file and kept the folders (N17).

**Copy diagnostics** copies a redacted support report to the clipboard. The same button is in Settings. The report covers the application version, OS, GPU, NVIDIA driver and driver CUDA level, the active environment (profile, Python, torch, Triton, protected pins and stored accelerator outcomes), the install manifest, the ComfyUI revision, tool versions, availability warnings, and the last 200 lines of the application log and of the ComfyUI runtime log. Tokens, signed links and the install folder path are removed. If the clipboard is unavailable, the report is shown in a selectable text box instead. This is covered by automated tests; a native copy has not been recorded.

## 7. Downloads

The Downloads page has an **Add download** dialog, **Import pack**, **Export** and the persistent queue. The queue is stored in the database and survives restarts. Jobs that were running when the application stopped come back as paused.

### Supported links

| Provider | Accepted forms |
|---|---|
| **CivitAI** | On `civitai.com`, `civitai.green` and `civitai.red`: model pages (`/models/{id}`, with optional `?modelVersionId=`), `/api/model-versions/{version}`, `/api/v1/model-versions/{version}`, and `/api/download/models/{version}`. Query filters `fileId`, `type`, `format`, `size` and `fp` select among a version's files |
| **Hugging Face** | `huggingface.co` model, dataset and space repo URLs, and `resolve`, `blob` or `tree` URLs with a revision and path. A repository URL opens a file list where you choose files, and one job is created per file. Transfers are pinned to the resolved commit |
| **Google Drive** | The forms the `gdown` library accepts: `/file/d/{id}/view` or `/edit` (also `/file/u/{n}/d/{id}`), `open?id={id}` and `uc?id={id}` on `drive.google.com` or `docs.google.com`, and `download?id={id}` on `drive.usercontent.google.com`. Folders and Docs, Sheets or Slides editor links are rejected |
| **Direct link** | Any other HTTPS URL. A plain `http://` link needs explicit confirmation. The filename comes from the server's `Content-Disposition` header, then from the URL |

You choose a target category (a `ComfyUI/models/` subfolder) and can set the filename. When a provider lists several files, you pick the variant. A token is needed for some CivitAI and Hugging Face downloads (see [Settings](#10-settings)). Clear errors distinguish a missing token, a gated repository, early-access content and not-found.

### Hugging Face: `hf` CLI versus native engine

Hugging Face downloads use one of two engines, set in **Settings > Hugging Face engine**:

| Engine | How it works | Pause and resume |
|---|---|---|
| **hf CLI** (default) | The pinned official `hf` tool, in its own isolated environment, supports Xet-backed files. Progress is an estimate, measured from bytes written to disk | `hf` does not resume across runs. **Pause stops the download, and resume, an application restart and network-failure retries all start the file again from 0%.** The queue labels these actions "will restart from 0%" |
| **Native** | The application's own HTTP downloader | Resumes using HTTP Range requests when the server sends a strong `ETag`. Without one, the partial file is discarded and the download restarts from 0 |

If the `hf` tool is missing or cannot be launched, the job falls back to the native engine. Other `hf` errors (authentication, gated, not found, hash mismatch) do not trigger the fallback. Sources other than Hugging Face and Google Drive always use the native engine. CivitAI links re-resolve on resume, because their signed URLs expire. Whether the native engine can fetch Xet-backed files has not been validated. If it cannot, it fails with a clear error suggesting the `hf` engine.

The `hf` downloader runs with a private home folder (`data/cache/hf/home`), not your profile. It never reads profile credential files such as `.netrc` or a cached Hugging Face token; its only credential is the token stored in the OS keyring. Before this change every `hf` download failed on Windows, because `hf` 2.2.0 needs a home folder at start-up and the child environment had none; runtime failures of the tool are now logged with a redacted summary. Verified natively on Windows: a live engine test without profile variables (N23) and an in-app download of a public Xet-backed file (`stabilityai/sd-vae-ft-mse-original`, `vae-ft-mse-840000-ema-pruned.safetensors`, 334,641,190 bytes) that finished as "COMPLETED VERIFIED" with its SHA-256 checked (N24). Pause and resume, the missing-tool fallback, gated repositories and token-bearing downloads have not been run natively.

### Google Drive: `gdown`

Google Drive downloads use the pinned `gdown` 6.4.2 library, installed with the other tools into its own hash-locked environment (`tools/gdown/`, on the same pinned Python as `hf`) and run through a small driver script. The driver runs with a private home folder (`data/cache/gdown/home`), never saves cookies and reports exact byte progress. Its traffic goes through the same bandwidth relay as `hf`, limited to Google hosts, so the shared speed cap applies. The file name comes from Google Drive, and the size is shown from the first progress sample.

Like `hf`, `gdown` does not resume across runs: **pause stops the download, and resume, an application restart and retries all start the file again from 0%.** The queue labels these actions "will restart from 0%". Google Drive does not say whether a file is missing or not shared, so both give the same permission error; the file must be shared with "Anyone with the link". If the `gdown` tool is missing or cannot be launched (for example in an install root made before this version), the job falls back to the application's own Drive downloader; **Repair** installs the missing tool. This has been covered by automated tests only; a live download natively has not been run (S07).

### States, retries and safety

Job states are queued, resolving, downloading, verifying, paused, completed, failed and cancelled. Per job you can pause, resume, retry, cancel, or resolve a filename conflict.

- **Retries.** Network errors and server (5xx) errors are retried automatically up to three times with increasing delay. Client (4xx) errors are not, except a 403 on an expired signed link, which triggers a fresh lookup.
- **Duplicates.** A job with the same provider identity as an active job is rejected as a duplicate. If the library already holds a file with the expected SHA-256, you can skip or download again.
- **Name conflicts.** If the target file exists, you choose: overwrite (the old file goes to the Recycle Bin or Trash), keep both (`name (1).ext`), or skip.
- **Integrity.** Data is written to a temporary file, synced, verified (SHA-256 when the provider supplies one, otherwise marked unverified) and only then renamed into the models folder. A failed hash leaves nothing in the models folder.
- **Concurrency.** The number of simultaneous downloads is a setting (default 3, range 1 to 32). Lowering it affects new downloads only. Running transfers continue.

### Bandwidth cap

Settings has a **Bandwidth cap (KiB/s)** field ("Shared by all downloads. Leave empty for unlimited speed."). Leave it empty for no limit; 0 is rejected. When set, one shared budget applies to native downloads and to Hugging Face `hf` CLI downloads together, and a change takes effect for downloads already running. To do this, every Hugging Face `hf` download is routed through a local relay on `127.0.0.1` that only forwards connections to `huggingface.co`, `hf.co` and their subdomains, even when no cap is set. If the relay cannot start, the job fails with `bandwidth_proxy_unavailable` rather than silently running uncapped. An `HTTPS_PROXY` setting is passed through to the relay's upstream connection. The relay has no per-download authentication: while a Hugging Face download runs, another local process could use it to reach those hosts (accepted residual R03). The shared budget is covered by automated tests; the cap has not been measured natively.

### Resource lists and packs

**Import pack** accepts a `resource_list` JSON file, or a pack index that points to several resource lists (each URL must be HTTPS). Remote JSON is limited to 8 MiB. A preview lets you choose provider file variants and deselect items before anything is queued.

A `resource_list` is a JSON array of objects with `name`, `url` and `type` (the model category). The key `model_type` is accepted in place of `type`.

```json
[
  {
    "name": "flux1-dev-fp8.safetensors",
    "url": "https://huggingface.co/example/repo/resolve/main/flux1-dev-fp8.safetensors",
    "type": "checkpoints"
  },
  {
    "name": "my-lora.safetensors",
    "url": "https://civitai.com/api/download/models/123456",
    "type": "loras"
  }
]
```

A pack index maps a category to entries whose `url` points at a resource list:

```json
{
  "Checkpoints": [
    { "name": "Starter checkpoints", "url": "https://example.com/packs/checkpoints.json" }
  ]
}
```

**Export** writes the selected or completed jobs as a `resource_list`, as a file or copied to the clipboard. Links that rely on a temporary signed query string are excluded, because they would expire.

## 8. Model library

The Models page lists the files under `ComfyUI/models` and under every folder in `extra_model_paths.yaml`. An unavailable root is reported. The scan does not follow links and does not hash files. The ComfyUI page can generate these extra folders from one base folder (see `extra_model_paths.yaml` in the ComfyUI section); downloads still go to `ComfyUI/models`.

- **Find.** Search by name, filter by category and root, and sort by name, category, size or modified time. The page rescans when files change, polls every 30 seconds, and has a Rescan button. If file watching is degraded, the page says so.
- **Details.** Select a model to see its details, sidecar files and preview.
- **Rename.** A rename stays in the same folder. Changing the extension needs your acknowledgement. Sidecar files with the same stem (for example `.preview.png`, `.civitai.info`, `.json`) are renamed too. A case-only rename works on Windows.
- **Move.** Move a model to another category, subfolder or root.
- **Delete.** Select one or more files. The dialog shows what will be moved to the **Recycle Bin or Trash**. If moving to the trash fails, the application reports it and offers **permanent delete** as a separate action that you must confirm. It never falls back to permanent deletion on its own.
- **Locked files.** On Windows, a model loaded by a running ComfyUI is locked. The operation fails without changing anything and offers Retry or "Stop ComfyUI and retry". Rename, move and delete are allowed while ComfyUI runs.
- **Identify on CivitAI.** Computes the file's SHA-256 and looks it up on CivitAI to show the model's details and a preview. Identification hashes the file on demand, which can take a while for large files.

Interrupted operations are recovered the next time the library rescans.

Verified natively on Windows (session 5): renaming a model to a Thai file name while ComfyUI ran; refusal of a delete while another program held the file open ("Another program is using the file. Close it, or stop ComfyUI, then retry."), then a successful move to the Recycle Bin after the handle was released. A lock held by a model that ComfyUI has loaded, a non-recyclable volume, a disabled Recycle Bin and network shares have not been exercised.

## 9. Custom nodes

The Custom Nodes page has a **Registry browser** and a list of **installed nodes**.

### Browsing the Comfy Registry

The browser searches and pages through the Comfy Registry. For each node it shows name, publisher, description, downloads, GitHub stars, latest version, supported OS, accelerators and ComfyUI versions, and warns when a node is flagged, deprecated or incompatible with your system. Results are cached; the cache lifetime is **Settings > Registry cache lifetime** (default 6 hours). **Refresh** forces a fresh request. When the Registry is unreachable, the page shows the last cached results and says they are stale.

### Installing

- **From the Registry.** Choose a node and optionally an exact version. Empty means the latest version.
- **From Git.** Enter an HTTPS repository URL and optionally a branch, tag or commit.

Either way the application first **prepares** the install and shows a review. Git sources are resolved to an exact commit and cloned to a staging folder. Registry archives are downloaded and extracted with archive hardening. Nothing is published into `custom_nodes/` until you consent.

**Content checks before consent.** The page refuses new node content that it cannot safely stage or review:

- Registry archives that contain `.git` or `__pycache__`, and Git sources that track `__pycache__` content.
- On Windows, a node whose top-level folder contains a program or library: any file or folder whose name ends in `.dll`, `.exe`, `.com`, `.cpl`, `.drv`, `.ocx`, `.scr` or `.sys`. Tools such as Git, uv or Python could load such a file before you approve the node. For a Git source, the exact commit's file list (`git ls-tree`) is checked **before** anything is checked out; for a Registry archive, the extracted staging folder is checked before the review. The refusal reads: "This node has programs or libraries (.dll, .exe and similar) in its top-level folder. Tools could load them before you approve the node, so it cannot be installed."
- Native files in sub-folders are allowed. Nodes that are already installed, or that you placed in `custom_nodes/` yourself, are not re-checked.

Tools that the application starts without an explicit working folder start in their own executable's folder, and uv's interpreter query imports nothing from the node folder (checked with pinned uv 0.13.1, N27). The Git and native-image refusals are covered by Windows tests with real Git; no public repository with a top-level `.dll` was available for a native run. These refusals are relaxed only for selected curated nodes in a consented first-run setup (see [Options](#options)).

### The review and consent

The review shows the exact resolved source (Registry id and version, or Git URL and commit), the previous source for updates, the node's dependency requirements, compatibility and source warnings, and any change to protected packages.

You must tick **execute** consent for the exact plan. The review warns that:

- installing a node runs third-party code;
- a Python venv is not a sandbox;
- dependency changes cannot be rolled back.

Other checkboxes:

- If the node has a root `install.py`, a separate checkbox, "Run the root install.py script with the selected Python environment before requirements.txt", lets you run it. It is unchecked by default and is never implied by anything else.
- Warnings and protected-package changes need their own explicit confirmations.
- "Remember this source" lets the application remember a trusted source (a Git repository, or a Registry node with a publisher and repository). A remembered source never replaces the per-install execute consent.

Closing an idle review discards the prepared plan.

**Protected packages.** Installing a node must not change `torch`, `torchvision`, `torchaudio` or Triton. Node dependency previews are resolved only with the pins held fixed, with no unconstrained retry, so if a node's requirements conflict with the pins, preparation fails with a resolution error that names the protected constraints and nothing changes. Node operations can never authorise a protected-package change. To deliberately change those packages, use the Environment page's package workflow.

### Updating, enabling, disabling, uninstalling

- **Update.** Registry nodes are installed from the newer version in a staging folder and swapped in, and the old folder goes to the trash. Git nodes use fast-forward only. A repository with local changes, an unfinished operation or a diverged history is **refused**, and your files are left alone.
- **Update all** prepares reviewed plans one node at a time, then applies them sequentially. More than 128 targets run as successive reviewed groups of at most 128 plans. Nodes that need an explicit target (a detached HEAD, or no upstream) are skipped with a message. A partially executed batch is never replayed: failed and unrun targets are prepared again. The batch stops if a dependency failure damages the environment.
- **Enable / Disable.** Disabling moves the node folder to `custom_nodes/.disabled/<name>`, the same convention ComfyUI-Manager uses. Enabling moves it back.
- **Uninstall.** The node folder moves to the Recycle Bin or Trash. Python packages stay installed, because other nodes may share them. There is no permanent-delete fallback.

**Update** on a Git node, and Update all, first read and display the node's real `git status`.

Verified natively on Windows: installing `https://github.com/ltdrdata/ComfyUI-Impact-Subpack` from Git with `install.py` approved. The operation log showed "running approved install.py before node requirements.", the script's own output, a pin re-check and "installing reviewed node requirements."; the node ended enabled with dependencies OK on `main` tracking `origin/main`, and `torch`, `torchvision` and `torchaudio` were unchanged (N29). Registry installs, updates, enable and disable, uninstall and the refusal of an edited repository are covered by automated tests only.

All node operations need ComfyUI stopped and run one at a time. After a successful operation the page offers **Restart ComfyUI**.

### Dependencies and repair

After a node is published, the application runs an approved root `install.py` first, then installs its reviewed requirements into the active environment. This order also applies to updates, dependency retries and consented curated setup installs. Protected pins are restored after the hook even when it fails or is cancelled; a hook or pin-restoration failure stops before requirements installation. Before installing requirements, the application restores profile constraints and validates the same guarded requirements bytes and frozen exact package versions against the post-hook environment. Failed validation stops installation. Pins are restored again after requirements installation, followed by final verification and containment/provenance checks. Requirements created by a hook are not discovered when the reviewed source had none.

App-managed resolution and installation are wheel-only. An approved arbitrary hook can invoke its own installers or builds; that policy does not sandbox scripts. A node's own Python dependencies can fail in ways the application cannot undo. When that happens:

- the node is marked **dependencies failed**;
- the environment is marked **repair required**;
- you can **Retry dependencies** (which prepares and reviews a plan again, and never runs a script merely because an earlier install allowed it) or **Repair** the environment from the Environment page or Settings.

Retry dependencies is also offered for nodes whose dependency state is "not run", including manual directories.

### Nodes the application did not install

Nodes already in `custom_nodes/` (including ones installed by ComfyUI-Manager) are listed as **manual** or unmanaged. They can be disabled and uninstalled. They cannot be updated by the application. If ComfyUI-Manager is present, the application re-scans after ComfyUI stops, so changes made through the Manager appear.

Known limitation: ComfyUI-Manager installed by first-run setup is listed with origin **Observed** and dependencies **NOT RUN**, although setup installed its requirements. Setup does not record it as a node operation.

### Recovery after interruptions

Node operations are journaled. If the application or system stops mid-operation, the next scan reconciles it: unpublished staging is removed, and copies that cannot be proven safe are **preserved** rather than deleted. Use **Rescan / finish recovery** on the page to run reconciliation. The operations panel shows unresolved operations, recent results and per-operation logs.

Recovery is identity-based: a copy is matched by its source, commit and content digest, and a node's final content digest must equal the reviewed one unless an approved `install.py` ran (then the new digest is recorded). Ambiguous recovery states (for example a changed or foreign copy) block ComfyUI from starting until resolved, while ordinary cleanup that failed only because the trash was unavailable does not. The operations panel marks such operations with a **Blocks start** chip, and Restart is unavailable while one exists.

After a reload, the page lists prepared plans that are waiting for review ("Nothing runs until you confirm. Resuming starts with every confirmation unchecked.") with **Resume review** and **Discard**, and keeps recent failed operations visible. Consent is never restored.

These recovery paths are covered by automated tests; an interrupted node install has not been run natively (scenario 14).

### Git requirements

Git-based operations need Git. On Windows the application uses its pinned MinGit. On Linux it uses system Git 2.40 or newer. If Git is missing, the page reports that Git is unavailable and tells you to complete tool setup (Windows) or install Git (Linux).

## 10. Settings

### Preferences

Choose **English** or **ไทย** in **Language**, then click **Save**. The interface changes only after a successful save; if saving fails, the current language stays active. The saved choice persists across launches.

| Setting | Default | Valid values | Notes |
|---|---|---|---|
| Download concurrency | 3 | 1 to 32 | Applies to newly started downloads |
| Hugging Face engine | hf CLI | hf CLI, Native | See [Downloads](#hugging-face-hf-cli-versus-native-engine) |
| Close behaviour | Ask every time | Ask, Stop ComfyUI and quit, Keep ComfyUI running in the background | |
| Theme | System | System, Light, Dark | Also toggled from the top bar |
| Language | English | English, ไทย | Also selectable at the top of the first setup stage |
| Registry cache lifetime | 6 hours | 1 or more | How long Registry results are cached |
| Bandwidth cap (KiB/s) | Unlimited | 1 or more, or empty | See the bandwidth cap section |

The bandwidth cap field is enabled and applies to native and Hugging Face `hf` downloads together, including running ones (see [Bandwidth cap](#bandwidth-cap)).

The install root is shown but is changed only by running setup again.

### Access tokens

| Token | Used for |
|---|---|
| CivitAI | Gated, early-access and API-restricted downloads, and identify |
| Hugging Face | Gated and private repositories |

Tokens are stored in the OS keyring (service `vjumpkung-comfyui`, one account per provider): Windows Credential Manager, or Secret Service on Linux. Where no keyring is available, tokens are kept in memory for the current session only and you are warned. Tokens are never written to the database or to logs and are never put in a URL.

Each token has **Test** and **Clear**. The page shows only whether a token is set and when it was last tested. It never shows the token back. A CivitAI token is sent only to CivitAI and a Hugging Face token only to Hugging Face.

### Re-setup buttons

The Settings page also has buttons for Repair, Change torch / CUDA, Change Python and Switch to custom venv (see [section 3](#3-changing-the-environment-later-re-setup)).

### App updates

Settings has an **App updates** panel that shows the installed version. Updates are possible only when the application was built with both an update public key (`VJUMP_UPDATER_PUBKEY`) and an endpoint (`VJUMP_UPDATER_ENDPOINT`). Every build made so far has neither, and the panel then says "This build has no update source configured, so it cannot update itself."; the application never updates itself in that case.

In a configured build, **Check for updates** looks for a newer signed release and shows its version, date and notes as plain text. **Install and restart** requires ComfyUI and other maintenance work to be stopped, verifies the update's signature, installs exactly the release you reviewed and restarts the application. There is no automatic check on launch. This flow is covered by automated tests only; no signed update has been published or installed.

### Diagnostics

The **Diagnostics** panel has **Copy diagnostics**, the same redacted support report as on the ComfyUI page (its contents are described in [ComfyUI page](#6-comfyui-page)), with the same selectable-text fallback.

## 11. Troubleshooting and FAQ

### repair_required

The environment is marked **repair required** when a package install or a node's dependency step failed partway, or was interrupted. The Environment and Custom Nodes pages show a banner. **Retry** (for example Retry dependencies on the node), or **Repair** from Settings. Repair builds a replacement environment from the profile and the node requirements, then activates it only after verification. The application cannot undo what an arbitrary script did outside its managed state.

### Pin drift

If a protected package (`torch`, `torchvision`, `torchaudio`, Triton) is found at an unexpected version, the pin guard reports `pin_drift`. Use Repair to restore the profile's versions. The application does not accept a drifted version as the new baseline.

### The port is already in use

Start shows a dialog with a free port. Accept it to save it as your launch port, or free the original port and press Start again. Another program, or a ComfyUI that was left running, may hold the port.

### A file is locked

On Windows, a model loaded by ComfyUI cannot be renamed, moved or deleted. Use "Stop ComfyUI and retry". Nothing was changed by the failed attempt.

### Long paths on Windows

Deep Python and Git paths can exceed Windows' default path limit. Enable long paths (`LongPathsEnabled`) and use a short install root, such as `C:\vjc`. Git's `core.longpaths` is set for clones, but it does not cover Python.

### Git is missing on Linux

Install Git 2.40 or newer (for example `sudo apt install git`) and retry. The application reports "Git is unavailable" when it cannot find it.

### The Registry is offline

The Custom Nodes page falls back to the cached Registry data and labels it stale. You can browse but not refresh. Installing a Registry node needs the Registry reachable, because the download link is requested for each install.

### maintenance_busy

Another maintenance operation (setup, an environment change, a node operation, a ComfyUI update, or a ComfyUI start) holds the application-wide lock. Wait for it to finish and try again. The application never stops another operation on your behalf.

### node_recovery_required

A custom-node operation was interrupted and the application could not prove which copy of a node is safe. Start is refused with "interrupted custom-node work must be resolved before launch." until you run **Rescan / finish recovery** on the Custom Nodes page and resolve what it reports. The operations panel marks the responsible operation with **Blocks start**. Preserved copies stay on disk.

### node_rescan_failed

ComfyUI stopped, but the custom-node folders could not be re-scanned. Run **Rescan / finish recovery** on the Custom Nodes page.

### rebuild_required and install recovery codes

The install folder was moved or copied, or the venv's interpreter no longer works. See [section 4](#4-moved-install-and-guided-rebuild). Related codes:

| Code | Meaning |
|---|---|
| `rebuild_required` | Normal commands are refused until the guided rebuild finishes |
| `root_missing`, `root_inaccessible` | The remembered install root is missing or cannot be accessed; locate it or create a new install |
| `root_invalid` | The folder you selected cannot be used (for example it is not empty, is a file, or is inside the current root) |
| `relocation_recovery_required` | An interrupted activation, ComfyUI update or node operation must be recovered before relocation |
| `profile_revision_unavailable` | The recorded setup profile revision is not bundled with this version |
| `rebuild_checkout_missing`, `rebuild_checkout_invalid` | The existing ComfyUI checkout is missing, unsafe or not a Git checkout |

### Closing is refused with "Maintenance is in progress"

The dialog "Maintenance is in progress. Wait for it to finish or cancel it before closing." appears when you close the application while setup, a node operation, an update or a post-stop custom-node rescan holds the maintenance lease. Right after **Stop**, the rescan can hold it for a moment; close again once it finishes (seen natively, N17).

### ComfyUI fails to start on the Python 3.12 profile with a `pwd` error

Older builds did not pass `USERNAME` to Python child processes. Python 3.12's `getpass.getuser()` then falls back to the Unix-only `pwd` module, which made `torch._dynamo` and ComfyUI fail on Windows. Current builds pass `USERNAME` (verified natively, N5). Update the application.

### A development build closes immediately with exit code 127

Debug builds before the session-6 fix overflowed the 1 MiB Windows main-thread stack during start-up once custom nodes were installed ("thread 'main' has overflowed its stack"). Current builds reserve 8 MiB for the main thread and for the async worker threads, and a test checks the built executable's stack reserve (N1, N6, N7; the release executable also reserves 8 MiB). Rebuild from the current source.

### Relaunching right after a forced exit fails with a WebView2 error

If the application is killed (for example with Task Manager), its WebView2 helper processes (`msedgewebview2.exe`) stay alive for a few seconds. Launching again immediately can fail with `failed to create webview: WebView2 error … 0x800700AA (The requested resource is in use)`. Wait a few seconds and launch again. Closing the window normally avoids this.

### Why does pausing a Hugging Face download restart from 0%?

The default `hf` CLI engine cannot resume across runs. Switch **Hugging Face engine** to Native to get resumable downloads, at the cost of not using the CLI's Xet support. See [Downloads](#7-downloads).

### Why is there no torchaudio?

The PyTorch 2.14.1 profiles omit it because no matching 2.14.x wheel existed. Nodes that import `torchaudio` fail on those profiles. The recommended Windows profile (Python 3.12, PyTorch 2.10.0) includes `torchaudio` 2.10.0. See [Profiles](#profiles).

### Why can't Repair change my Python version?

Repair and Change torch / CUDA keep the Python minor version. Use **Change Python** for a different minor.

### Where do I find logs?

See [Data and privacy](#12-data-and-privacy).

## 12. Data and privacy

### Where data lives

Everything lives in the install root you chose, apart from one small pointer file.

| Path | Contents |
|---|---|
| `<root>/ComfyUI/` | The ComfyUI Git checkout: `models/`, `custom_nodes/`, `user/`, `input/`, `output/` |
| `<root>/ComfyUI/custom_nodes/.disabled/` | Disabled custom nodes |
| `<root>/envs/<env-id>/` | App-owned Python environments (several can exist side by side) |
| `<root>/tools/uv/` | Pinned uv |
| `<root>/tools/git/` | Pinned MinGit (Windows only) |
| `<root>/tools/hf/` | Isolated environment for the `hf` CLI |
| `<root>/tools/gdown/` | Isolated environment for the `gdown` Google Drive downloader |
| `<root>/install.json` | Marker recording the install root, active environment and profile |
| `<root>/data/app.db` | SQLite database: settings, environment records, launch options, download queue, model catalogue, node records and the Registry cache. Contains no tokens or signed URLs |
| `<root>/data/cache/` | `uv`, `uv-python`, `pip`, `hf`, `torch` and `triton` caches |
| `<root>/data/cache/hf/home` | Private home folder for the `hf` CLI child process |
| `<root>/data/cache/wheels/<sha256>/` | Verified accelerator wheels, one folder per pinned SHA-256 |
| `<root>/data/downloads/` | In-progress downloads (temporary files) |
| `<root>/data/tmp/hf` | Temporary area for `hf` CLI downloads |
| `<root>/data/backups/` | Copies of an `extra_model_paths.yaml` that the application replaced |
| `<root>/data/logs/` | Log files |

**Bootstrap pointer.** The only file outside the install root is `bootstrap.json`, in the OS configuration folder under `vjumpkung-comfyui`. It records which install root to use. If it is deleted, the application falls back to the default root.

### Logs

| File | Contents |
|---|---|
| `data/logs/app.log` | The application's log, rotated |
| `data/logs/comfyui-<time>-<session>.log` | One file per ComfyUI session |
| `data/logs/setup-<run-id>.log` | One file per setup run |

### What is redacted

Logs, error messages and the copied diagnostics are redacted for:

- registered secrets such as saved tokens;
- `Authorization` and bearer strings;
- token and API-key values in query strings;
- AWS-style signed URL values (`x-amz-*`);
- Hugging Face token patterns;
- credentials embedded in URLs.

Signed direct links are held in memory only. Their query string is removed from what is saved in the queue.

### Credentials

See [Access tokens](#access-tokens). The only process that receives a token in its environment is the pinned `hf` downloader, and only the Hugging Face token. ComfyUI, node scripts, uv and Git never receive tokens.

---

# Part 2: Developer guide

## 13. Architecture

### Processes

```text
vjumpkung-comfyui-desktop (Rust, Tauri 2)
+-- Manager window (system webview, React/Vite UI)
|     typed IPC (invoke + Channels) ------------> Core services
|                                                  setup / environment
+-- ComfyUI window (system webview)                comfy runtime
|     http://127.0.0.1:<port>  (no IPC)            downloads
|        |                                         model library
|        v                                         custom nodes
|   ComfyUI child process (venv python)            settings + keyring
|     spawned and owned by the core                SQLite
+-- uv / git / python / hf subprocesses, spawned by the core with a minimal allow-listed environment
```

- The manager UI talks to the core only through Tauri IPC. There is no localhost HTTP API for the manager.
- The ComfyUI window has no Tauri capability. Content served by ComfyUI cannot call any command.
- The window labels are `manager` and the ComfyUI window. The manager capability is the only one declared.

### Clean Architecture crates

```text
domain         <-  application  <-  infrastructure  <-  desktop      +  ui/
(rules)            (use cases,       (adapters:           (Tauri
                    port traits)      SQLite, HTTP,        composition,
                                      processes, fs)       IPC, DTOs)
```

Allowed edges (everything else is forbidden and checked by `scripts/check-arch-deps.mjs`):

| Crate | May depend on |
|---|---|
| `domain` | `thiserror` only |
| `application` | `domain`, `thiserror`, `async-trait`, `futures-core` |
| `infrastructure` | `application`, `domain` (never Tauri) |
| `desktop` | `application`, `infrastructure`, `domain` |

| Layer | Contains | Must not contain |
|---|---|---|
| domain | Pure rules, validated constructors, state transitions, per-context errors | serde, Tokio, reqwest, rusqlite, Tauri, filesystem or clock access |
| application | Use cases (concrete structs with `execute(input) -> Result`), plain input/output models, port traits, the maintenance gate | serde or specta derives; reqwest, SQLite, Tauri or Tokio types |
| infrastructure | Provider and persistence DTOs, SQL, subprocesses, Tokio scheduling, filesystem containment, translation of adapter errors into port errors | Tauri types, UI DTOs |
| desktop | serde IPC DTOs, generated bindings, commands (controllers) and presenters, sanitised error codes, wiring in Tauri `run`/`setup` | Business rules; passing `AppHandle` inward; querying SQLite directly |

Concrete adapters are created and wired **only** in the desktop composition root. Use cases receive `Arc<dyn Port + Send + Sync>`. Each layer owns its errors, which are translated at each boundary and sanitised at IPC. See [ADR-0002](adr/0002-clean-architecture-ddd.md).

### Contexts

Each context is a module inside each layer crate.

| Context | Owns |
|---|---|
| `environment` | `PythonEnvironment` (owned or adopted, Python/torch/CUDA, verification state, `repair_required`), setup profiles, provisioning runs, activation |
| `comfy_runtime` | Lifecycle (`Stopped`, `Starting`, `Running`, `Stopping`, `Crashed`), launch options, updates |
| `models` | `acquisition` (download jobs and their state machine, sources) and `library` (model files, categories) |
| `custom_nodes` | `InstalledNode`, provenance (Registry, Git, Manual), consent, dependency outcomes, the operation journal |
| `settings` | Validated preferences; credential references or presence only |
| `shared` | Value objects such as `SafeFileName`, `RelativeModelPath`, `Sha256`, `ByteSize`, ids |

### Maintenance gate and operation matrix

One application-level scheduler (the maintenance gate) serialises setup and activation, environment changes, node operations, ComfyUI updates and ComfyUI start or restart. The gate is acquired **before** checking process state, and composed workflows acquire it once. It controls only work issued by this app. ComfyUI-Manager or user scripts are outside it, so the app re-scans after ComfyUI stops. There is no general event bus. Required effects use awaited orchestration; channels are for UI notification only.

| Operation | Gate | ComfyUI must be stopped | Consent | Failure state | Recovery |
|---|---|---|---|---|---|
| Setup, re-setup, activate | yes | yes | none | New env `failed`; old env stays active | Retry or discard the new env |
| Package install, upgrade, uninstall | yes | yes | confirm if a pinned package changes | Env `repair_required` | Retry or Repair |
| Install custom node | yes | yes | yes | No partial folder (staged). Dependency or `install.py` failure: node `deps-failed` and env `repair_required` | Retry deps, Uninstall, Repair |
| Update, enable, disable, uninstall node | yes | yes | update: yes if the source changed | Staged swap; old version kept until success | Retry or restore from trash |
| Update ComfyUI | yes | yes | none | Dirty or diverged: refused, no change | Resolve manually, retry |
| Start, restart ComfyUI | yes | n/a | none | `Crashed` with a log | Retry or change launch options |
| Download | no (own queue) | no | none | Job `failed` or `paused`; no file in the models folder | Retry or resume |
| Rename, move, delete model | per-path lock | no | delete: confirm | Locked: unchanged, error shown | Retry, or stop ComfyUI and retry |

### Frontend layering

`ui/src/app` -> `ui/src/features/<context>` -> `ui/src/shared` (`shared/ui` is shadcn components; `shared/ipc` is the generated bindings plus a gateway). Components never call `invoke` directly; they use feature hooks and the gateway at `ui/src/shared/ipc/gateway.ts`. One feature must not import another feature's internals. ESLint (`@boundaries/eslint-plugin`) enforces this. State comes from `@tanstack/react-query` over IPC.

## 14. Project layout

```text
Cargo.toml, Cargo.lock       Rust workspace (resolver 3, edition 2024, rust-version 1.99)
rust-toolchain.toml          Pinned toolchain (1.99.0, rustfmt, clippy)
crates/
  domain/                    Business types and invariants
  application/               Use cases and port traits
  infrastructure/
    migrations/              001_settings.sql ... 008_install_relocation.sql
    src/                     comfy_runtime, custom_nodes, environment, models, settings, shared
    tests/                   Integration tests (workflow, real_setup)
  desktop/
    src/ipc/                 Commands, DTOs, bindings.rs (command list)
    capabilities/manager.json
    permissions/autogenerated/   Generated, do not edit
    tests/                   acl.rs, bindings.rs, navigation.rs, stack_reserve.rs, startup_smoke.rs
    build.rs                 App manifest (the command list for the ACL), 8 MiB Windows stack reserve
    tauri.conf.json (+ .windows, .linux, .updater overlays), icons/
ui/
  src/{app,features,shared}  React/TypeScript frontend
  package.json               pnpm 10.24.0 and scripts
docs/                        REQUIREMENTS, DESIGN, ADRs, IMPLEMENTATION_STATUS, handoff/
scripts/                     tauri.mjs, check-arch-deps.mjs (+ test and fixtures)
.github/workflows/           ci.yml, release.yml
```

## 15. Toolchain setup

### Common

- Rust: install `rustup`. The toolchain in `rust-toolchain.toml` (`1.99.0`, with rustfmt and clippy) is selected automatically.
- Node.js and pnpm: pnpm `10.24.0` is pinned by `packageManager` in `ui/package.json`. No Node version is pinned (the workflows use Node 24). Enable Corepack or install the pinned pnpm.
- Git, for the tests that use real repositories.

### Windows (MSVC, default)

Install Visual Studio 2022 (Community is enough) with:

- `Microsoft.VisualStudio.Component.VC.Tools.x86.x64` (MSVC v143), and
- a Windows 10/11 SDK, for example `Microsoft.VisualStudio.Component.Windows11SDK.22621`.

The WebView2 runtime is needed to run the app. With the toolchain in place, cargo works from PowerShell or Git Bash without a Developer Prompt or an environment script. If the components go missing, linking fails with `LNK1181 kernel32.lib`. Note that running the VS bootstrapper's `install --add ...` on an existing instance only updates it and skips the `--add` components. Add them from an elevated shell with the Visual Studio Installer's `setup.exe modify ... --add <component>`.

Desktop test executables get a Common Controls v6 manifest from `crates/desktop/build.rs`.

### Windows (GNU fallback)

Use only when MSVC is unavailable. It needs the `1.99.0-x86_64-pc-windows-gnu` toolchain and MSYS2 UCRT64 `gcc` and `windres`. In Git Bash, run `source docs/handoff/tooling/gnu-env.sh` before cargo. The script selects the GNU toolchain, uses a separate `target-gnu` directory, places the 64-bit `WebView2Loader.dll` next to test executables, links a Common Controls v6 manifest, and reduces debug info. Without it, desktop test executables fail with `0xc0000139` or `0xc000007b`. The exports last for the whole shell, so use a separate shell for GNU work.

### Linux

Install the Tauri build prerequisites (as in the release workflow):

```bash
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
  librsvg2-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev \
  build-essential patchelf
```

Also install Git 2.40 or newer. The project has not yet been built or tested on Linux.

### Memory

Rust builds of this workspace are memory-hungry. On the 32 GB development machine, `-j 4` to `-j 6` works; run only one heavy cargo command at a time.

## 16. Build, run, test and lint

Run from the repository root.

| Task | Command |
|---|---|
| Install frontend dependencies | `pnpm -C ui install --frozen-lockfile` |
| Desktop app with hot reload | `pnpm -C ui tauri dev` |
| Frontend only | `pnpm -C ui dev` |
| Frontend production build | `pnpm -C ui build` |
| Desktop bundles | `pnpm -C ui tauri build` |
| Frontend lint | `pnpm -C ui lint` |
| Frontend types | `pnpm -C ui typecheck` |
| Frontend tests (Vitest) | `pnpm -C ui test` |
| Rust tests | `cargo test --workspace --locked` |
| Rust format check | `cargo fmt --all -- --check` |
| Rust lint | `cargo clippy --workspace --all-targets --locked -- -D warnings` |
| Architecture rules | `node scripts/check-arch-deps.mjs` |
| Architecture checker tests | `node --test scripts/check-arch-deps.test.mjs` |
| Binding drift check | `cargo test -p desktop --test bindings --locked` |
| ACL check | `cargo test -p desktop --test acl --locked` |
| Production-protocol build | `cargo build -p desktop --locked --features tauri/custom-protocol` |
| Native start-up smoke test | `cargo test -p desktop --test startup_smoke --features tauri/custom-protocol fresh_root_starts_and_closes_cleanly -- --ignored --exact --nocapture --test-threads=1` |

The full check list, as used for handoff:

```bash
pnpm -C ui install --frozen-lockfile && pnpm -C ui lint && pnpm -C ui typecheck && VITEST_MAX_WORKERS=2 pnpm -C ui test && pnpm -C ui build
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -j 4 -- -D warnings
cargo test --workspace --locked -j 4 --no-fail-fast
node --test scripts/check-arch-deps.test.mjs && node scripts/check-arch-deps.mjs
cargo test -p desktop --test bindings --locked
cargo build -p desktop --locked --features tauri/custom-protocol
pnpm -C ui tauri build
```

The last run of this list (2026-10-20, Windows, MSVC) passed: Vitest 30 files and 202 tests; Rust 802 passed, 0 failed, 12 ignored; architecture self-tests 10/10 and `OK`; bindings 1 passed, 1 ignored (the exporter); ACL 11 passed; the native start-up smoke test passed; the bundles built. See [RELEASE_ACCEPTANCE.md](RELEASE_ACCEPTANCE.md#automated-and-packaging-evidence).

Notes:

- The smoke test opens a visible window, needs WebView2 and an interactive desktop, and fails if another instance of the application is running.

- Without `ui/dist`, the desktop build fails ("`frontendDist` ... doesn't exist"). Run `pnpm -C ui build` first.
- `pnpm -C ui tauri ...` runs `scripts/tauri.mjs`, which finds the Tauri CLI in `ui/node_modules` and sets `TAURI_APP_PATH` (crates/desktop) and `TAURI_FRONTEND_PATH` (ui).
- The Rust tests include the binding drift and ACL checks.
- Generated permission TOMLs under `crates/desktop/permissions/autogenerated/` can show as modified after a build because of line-ending differences. Check `git diff` before committing them.
- A test that needs Git must really run it. Tests guarded by a "skip if no git" macro do not count as evidence when Git is missing.

## 17. IPC and generated bindings

IPC uses **tauri-specta** `2.0.0-rc.25` (a pinned prerelease, a deliberate exception to "stable only"). The command list is defined once, in `crates/desktop/src/ipc/bindings.rs` (`collect_commands!`), and is shared by `run()`, the binding export test and the ACL tests. The TypeScript output is `ui/src/shared/ipc/bindings.ts`, which is checked in.

- **Never edit `bindings.ts` by hand.**
- The normal test `cargo test -p desktop --test bindings --locked` exports to a temporary file and fails if it differs from the checked-in file.
- To regenerate, run exactly:

  ```bash
  cargo test -p desktop --test bindings --locked -- --ignored export_bindings
  ```

- Wrap new commands in `ui/src/shared/ipc/gateway.ts`. All Tauri channels stay inside the gateway.
- Channels carry progress and change events (runtime, downloads, model library, custom nodes). A `Gap` event means the UI must refetch authoritative state. Setup events use a channel too, and closing the UI channel does not stop the setup run.
- IPC errors are sanitised codes with a fixed message. No secret, signed URL or path outside an authorised root crosses IPC.

## 18. Capabilities, ACL and adding a command

Custom Tauri commands are reachable from every window by default, so each is declared as an explicit app-command permission and granted only to the manager window.

To add a command:

1. Implement the handler in the appropriate `crates/desktop/src/ipc/*_commands.rs` file. It should call a use case and return a DTO or `IpcError`.
2. Register it in `crates/desktop/src/ipc/bindings.rs` (`collect_commands!`).
3. Add its name to the command list in `crates/desktop/build.rs` (the app manifest). The build generates the `allow-<name>` permission under `permissions/autogenerated/`.
4. Add `allow-<name>` (with dashes) to `crates/desktop/capabilities/manager.json`.
5. Add a valid-body case to `crates/desktop/tests/acl.rs`, so the completeness test, local-origin allow test, ComfyUI-window denial tests and remote-origin denial test cover it.
6. Add the gateway wrapper in `ui/src/shared/ipc/gateway.ts`.
7. Regenerate the bindings (previous section).

The ACL integration tests run against a test app using the real compiled manifest and capability. They verify that every registered command is represented in the manifest, bindings and capability; that the manager window with a local origin can invoke all of them; and that the ComfyUI window and remote origins are denied.

The existing opener plugin is used only for repository links, and its registration is not to be duplicated. `node_open_repository` validates the URL (HTTPS, a non-empty authority, no user information) before opening.

The M6 commands `setup_list_curated_nodes`, `setup_open_install`, `setup_new_install`, `app_update_status`, `app_update_check`, `app_update_install` and `app_copy_diagnostics` follow the same rules: each has an `allow-…` entry in `manager.json` and an ACL case. The updater and clipboard plugins are used from Rust only; `manager.json` grants no updater or clipboard plugin permission, so no webview can call them. `setup_open_install` and `setup_new_install` work in the recovery state, when the normal services are unavailable.

## 19. Database migrations

The application database is SQLite (`rusqlite`, bundled) at `data/app.db`. The single ordered migration list lives in `crates/infrastructure/src/shared/sqlite.rs` (`migrations()`), using `rusqlite_migration`. Contexts append to this list. They never keep their own `user_version` sequences.

To add a migration:

1. Create `crates/infrastructure/migrations/NNN_name.sql` with the next number (currently the last is `008_install_relocation.sql`). Migrations are additive.
2. Add `M::up(include_str!("../../migrations/NNN_name.sql"))` to `migrations()` in order.
3. Update any test helper that builds a partial migration list (for example in the model library tests).
4. Test the migration in the owning context's tests.

The M6 migrations are `007_environment_components.sql`, which adds the `components_json` column to `envs` for stored accelerator and component results, and `008_install_relocation.sql`, which creates the `install_relocations` table of accepted root moves. Node dependency state needed no migration.

Tables are described in [REQUIREMENTS section 5.2](REQUIREMENTS.md) (non-normative) and in the migration files, which are the source of truth. Signed URLs and tokens are never stored.

## 20. Setup profiles

Profiles are versioned JSON files bundled with the application in `crates/infrastructure/src/environment/data/profiles/`. Remote profile updates are a later feature and would have to be signed. The following fields are used (see any bundled profile for a complete example):

| Field | Meaning |
|---|---|
| `schema_version`, `id`, `revision`, `label` | Identity. A changed profile gets a new `revision`; the old revision is archived so rebuilds can use the exact recorded one |
| `status` (`stable` or `experimental`), `recommended` | Presentation and defaults |
| `os` | `windows` or `linux` |
| `python` | Exact patch version, pinned for the uv-managed CPython |
| `torch` | `torch`, `torchvision`, `torchaudio` (nullable), `cuda` (`cu130`, `cu126`) and `index_url` |
| `min_driver_cuda`, `min_cc`, `torch_archs` | Eligibility gates: driver CUDA level, minimum compute capability, supported GPU architectures |
| `triton` | Package (`triton-windows` or `triton`) and version specifier |
| `accelerators` | Optional accelerator records (schema 2). Only `win-py312-torch2100-cu130` has entries |
| `constraints` | Extra resolver constraints |
| `verify` | Expected torch version prefix, require CUDA, require Triton |
| `disk` | Disk budget (`env_bytes`, `other_bytes`, `margin_bytes`, `calibrated`) |
| `provenance` | Notes on how the values were observed |

Eligibility checks the OS, an NVIDIA GPU, every detected GPU's compute capability and wheel architecture, the driver's CUDA level, and the disk budget. Pinned tool versions and SHA-256 values (uv 0.13.1, MinGit, `hf` 2.2.0 and `gdown` 6.4.2 with their hash-locked requirements, managed CPython hashes, the app-local MSVC runtime) are in `crates/infrastructure/src/environment/pins.rs` and `data/`.

**Accelerator records (schema 2).** Each record has `id`, `name`, `modules` (importable module names used for the import check), `source`, `no_deps`, `default_selected`, `non_fatal` and `provenance`. `source` is either `{"kind": "wheel"}` with an HTTPS `url`, a safe `.whl` `filename`, `sha256` and `size`, or `{"kind": "packages"}` with requirement strings. Schema 1 profiles remain readable only with an empty accelerator list. `win-py312-torch2100-cu130` is schema 2, revision 3, with five experimental wheels and a "Compatibility packages" component, none selected by default; revisions 1 and 2 are kept in `data/profiles/archive/` for rebuilds. The other six profiles are schema 1 with no accelerators. FlashAttention 3's installed module name is unverified, so its `modules` list is empty.

## 21. Adding a download provider

NFR-MAINT-2 requires that adding a provider needs no domain or UI-core change beyond a new `SourceRef` variant.

1. Add a `SourceRef` variant in the domain (`crates/domain/src/models/acquisition/`). It must hold only identity, never a token or signed URL.
2. In `infrastructure/src/models/acquisition/`, implement the resolver (the `SourceResolver` port: `resolve` returns the downloadable files and `selected` returns the chosen one) and the transfer (the `Downloader` port).
3. Add URL recognition in `sources.rs`. Reject token-like query keys.
4. Make credentials origin-scoped: attach auth only to the provider's own HTTPS origin and strip it on cross-origin redirects (`http.rs`).
5. Wire the new adapters in the desktop composition root.
6. Add the provider's identity to de-duplication, error classification and, if it supplies a hash, verification.
7. Add infrastructure integration tests with HTTP fixtures for redirects, auth stripping, resume and malformed responses.

## 22. Testing strategy and coverage

| Level | Scope | Examples |
|---|---|---|
| Domain unit | Pure, no I/O | State transitions, ownership, profile compatibility, consent, `SafeFileName` rules |
| Application | Use cases with fakes of ports | Start versus maintenance race, cancellation cleanup, torch pin guard, idempotent download finalisation |
| Infrastructure integration | Temp dirs, temp SQLite, HTTP fixtures, real Git | Migrations, redirects and auth stripping, Range resume, malicious zip corpus, case-only rename, trash failure, crash reconciliation |
| Desktop and IPC | Serialisation, binding drift, capability denial | The ComfyUI window cannot invoke commands |
| Frontend | Vitest with React Testing Library | Colocated as `*.test.ts` and `*.test.tsx` |

Rust unit tests sit beside modules or in `tests.rs`. Integration tests are in each crate's `tests/`. Domain and application crates build and test without Tauri, network, database or GPU.

Coverage targets (NFR-MAINT-4), measured with `cargo llvm-cov`: **domain at least 90% of lines, application at least 80%**. Historical coverage numbers are not evidence for the current tree.

## 23. Opt-in real-system tests

Ordinary tests never touch real credentials or user installations. These tests are `#[ignore]`d and need explicit execution:

| Test | Command and variables | Needs |
|---|---|---|
| Real first-run setup | `VJ_REAL_ROOT=<empty or nonexistent dir> cargo test -p infrastructure --locked --test real_setup -- --ignored --nocapture`. Optional `VJ_REAL_PROFILE` picks a profile | Network, several GB of downloads, an NVIDIA GPU. The root must not exist yet or be empty |
| Interpreter validation | `VJ_TEST_PYTHON=<path to CPython 3.12-3.14>` with `a_real_interpreter_is_probed` in `infrastructure/src/environment/python/tests.rs` | A suitable interpreter |
| Real tool provisioning | `real_tools_are_provisioned` in `infrastructure/src/environment/tools/tests.rs` | Network; downloads uv, MinGit and Python |
| Real keyring | `real_keyring_round_trip` in `infrastructure/src/settings/secrets/tests.rs` | An unlocked OS credential store; creates and deletes a temporary credential |
| Real `gdown` download | `VJ_REAL_GDOWN_PYTHON=<absolute path to the venv python.exe with gdown 6.4.2> cargo test -p infrastructure --locked --lib real_gdown -- --ignored --nocapture` | Network; resolves and downloads a 498,881,336-byte public Google Drive file through the relay (added in session 7) |
| Real `hf` download | `VJ_REAL_HF_EXE=<absolute path to the pinned hf> cargo test -p infrastructure --locked real_hf_download_succeeds_without_user_profile_variables -- --ignored --nocapture` | Network; downloads a 334,641,190-byte public Xet-backed file and checks its size and SHA-256. Passed on 2026-10-20 (N23) |
| `hf` relay with denied egress | `real_hf_xet_obeys_relay_with_direct_egress_denied` in `models/acquisition/hf_proxy/tests.rs`, with `VJ_REAL_HF_EXE`, `VJ_REAL_HF_REPO`, `VJ_REAL_HF_REVISION` (a pinned commit), `VJ_REAL_HF_FILE` (a public Xet-backed file) and `VJ_REAL_HF_SIZE` (its exact size, 1 to 16 MiB) | A disposable sandbox where the child's direct egress is denied externally. Not run yet |
| Native start-up smoke | See [section 16](#16-build-run-test-and-lint) | WebView2, an interactive desktop, no other running instance. Part of the check list |

Four further ignored tests are helper child processes that their parent tests start, and `export_bindings` regenerates the bindings. The full inventory of the 12 ignored tests and which ones ran in session 6 is in [RELEASE_ACCEPTANCE.md](RELEASE_ACCEPTANCE.md#ignored-test-inventory).

## 24. Packaging and updater

Packaging is configured in `crates/desktop/tauri.conf.json` with platform overlays that Tauri merges automatically.

| File | Purpose |
|---|---|
| `tauri.conf.json` | `productName` `vjumpkung-comfyui-desktop`, version `0.1.0`, identifier `dev.vjump.comfyui-desktop`, window `manager` titled `vjumpkung ComfyUI`, CSP, bundle targets `all`, placeholder icons |
| `tauri.windows.conf.json` | Targets `nsis` and `msi`; WebView2 `downloadBootstrapper` in silent mode; NSIS `currentUser` install mode |
| `tauri.linux.conf.json` | Targets `appimage` and `deb`; deb depends on `libwebkit2gtk-4.1-0` and `libgtk-3-0` |
| `tauri.updater.conf.json` | `createUpdaterArtifacts: true`. Passed as `--config tauri.updater.conf.json` only when a signing key is configured |

Build with `pnpm -C ui tauri build` (about 12 minutes on Windows). The wrapper `scripts/tauri.mjs` runs the CLI installed in `ui/node_modules`. The UI must be built before embedding. On first use the Tauri CLI downloads NSIS and WiX. Output is under `target/release/bundle/`:

- `nsis/vjumpkung-comfyui-desktop_0.1.0_x64-setup.exe`
- `msi/vjumpkung-comfyui-desktop_0.1.0_x64_en-US.msi`

The last build (2026-10-20, from the final session-6 checkpoint) exited 0 and produced the NSIS installer (9,336,453 bytes) and the MSI (13,340,672 bytes); their SHA-256 values are in [RELEASE_ACCEPTANCE.md](RELEASE_ACCEPTANCE.md#snapshot). Both installers and the release executable are unsigned (`NotSigned`), and the release executable reserves an 8 MiB main-thread stack. The installers have **not** been run, and the release executable has not been launched. Linux bundles need a Linux host and have not been built.

**Updater.** The updater is inactive unless both `VJUMP_UPDATER_PUBKEY` and `VJUMP_UPDATER_ENDPOINT` are supplied at build time. The release workflow also reads `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` from secrets, and passes the updater overlay only when a signing key exists. It fails early if a key is present but the public key or endpoint is missing. Key custody belongs to the owner. Never substitute a generated development key in a release.

**Updater design.** `tauri-plugin-updater` `=2.14.0` and `tauri-plugin-clipboard-manager` `=2.5.0` are pinned in the workspace `Cargo.toml` and `Cargo.lock`. The updater plugin is registered only when both build-time variables are present and valid; `crates/desktop/build.rs` re-runs when either changes. The commands `app_update_status`, `app_update_check` and `app_update_install` stay registered, and absent configuration returns a controlled refusal ("This build has no signed update source configured."). Installing takes the maintenance gate, verifies that ComfyUI (including a headless instance) is stopped, and installs only the offer the user reviewed, identified by the `updateId` of that offer. Downloads and the model library are quiesced and the log is flushed before the installer runs. No webview has an updater or clipboard plugin permission. These rules are covered by automated tests (for example `invalid_signature_never_invokes_installer`, `update_install_requires_the_reviewed_offer`); no signed update has been built, published or installed.

**Remaining packaging work:** install, launch and uninstall the Windows bundles on a disposable machine (including the WebView2 bootstrapper and user-data preservation across uninstall and reinstall), build and run the Linux bundles, and sign the Windows artifacts. See the release gates in [RELEASE_ACCEPTANCE.md](RELEASE_ACCEPTANCE.md#release-gates).

## 25. Continuous integration status

The owner authorized re-enabling CI on 2026-10-21; hosted validation is pending.

`.github/workflows/ci.yml` runs on every push and pull request. It runs the UI lint, typecheck, tests and build, Rust fmt, clippy and tests, the desktop build and the architecture checks on Windows and Ubuntu 24.04, plus the domain (90 %) and application (80 %) coverage thresholds.

`.github/workflows/release.yml` builds the Windows and Linux bundles and attaches them to a draft GitHub release `v<version>`. It runs only on the default branch and only when a push changes `version` in `crates/desktop/tauri.conf.json`; other pushes, and tag pushes, do not build bundles. The two platforms build one after the other so they share one draft.

To cut a release:

1. Set the same new version in `crates/desktop/tauri.conf.json`, `Cargo.toml` (`[workspace.package]`) and `ui/package.json`, and refresh `Cargo.lock` (for example with `cargo check --workspace`).
2. Push to the default branch. When the draft `v<version>` is complete, publish it on GitHub.

A version that already has a published release is skipped. A manual run (`workflow_dispatch`) on the default branch builds the current version when it has no release yet (for example the first release) or rebuilds its draft from the same commit. If a draft belongs to an older commit, re-run that commit's original workflow run instead. Linux bundles are built on Ubuntu 22.04 for glibc compatibility; GitHub retires that runner on 2027-04-17.

## 26. Coding conventions

- Rust: rustfmt, four-space indentation, `snake_case` modules and functions, clippy with warnings denied.
- TypeScript: two-space indentation, single quotes, no semicolons. PascalCase components and types, kebab-case file names for features.
- Dependencies point inward. Keep serialisation (serde, specta) out of `domain` and `application`.
- Access Tauri from the UI only through `ui/src/shared/ipc/gateway.ts`.
- Pin dependency versions exactly. Both `Cargo.lock` and `pnpm-lock.yaml` are committed, and CI-style commands use `--locked` and `--frozen-lockfile`.
- New global frontend state uses zustand stores; the existing `useSyncExternalStore` stores are unchanged.
- Follow the design system in [DESIGN.md](DESIGN.md) for UI. New tokens or patterns update DESIGN.md first. Manager UI copy uses English and Thai entries in the shared translation catalog through `t()` / `useTranslation()` at render time (DESIGN.md §1); product names and backend diagnostics stay as delivered.
- Record any architectural deviation as an ADR in `docs/adr/`.
- Commit messages use short conventional prefixes: `feat:`, `fix:`, `docs:`, `chore:`.
- Do not hand-edit generated files (bindings, autogenerated permissions).

## 27. Security model

| Control | What it does |
|---|---|
| Secret redaction | Logs redact registered secrets, authorization and bearer strings, token and API-key query values, AWS `x-amz-*` values, Hugging Face token patterns and URL user information (`infrastructure/src/shared/logging.rs`). Token-like query keys in URLs are rejected. Signed direct URLs are staged only in memory, with the query removed from the saved locator |
| Credential scoping | Auth headers are attached only to the exact HTTPS provider origin (`civitai.com`, `huggingface.co`) and stripped on cross-origin redirects. Subprocess environments are allow-listed. The only exception is `HF_TOKEN` in the pinned `hf` child's environment. It is never an argument. Tokens never reach ComfyUI, node scripts, uv or Git |
| Keyring | Tokens live in the OS keyring under service `vjumpkung-comfyui`, never in SQLite or logs. Domain and application types hold references or presence only |
| Archive hardening | The shared extractor rejects traversal, absolute and UNC paths, drive-relative paths, symlinks, reparse points, special files and alternate data streams; checks case-insensitive collisions; enforces limits (20,000 entries, 2 GiB expanded size, 200:1 compression ratio) and extracts into a fresh staging directory |
| Path containment | Names are validated by domain value objects (no separators, `..`, reserved names, trailing dot or space, control characters, over 255 bytes). Adapters check real containment at operation time against authorised roots, with no-follow and reparse-point checks. Model file operations use pinned directories |
| Consent | Installing a node, running `install.py` and building from source execute third-party code. The UI states that a venv is not a sandbox, shows the exact source, and requires explicit confirmation per reviewed plan. FirstRun's Review disclosure and Start setup click cover Manager and selected curated sources without another pause; selected curated hooks run before reviewed requirements and the two disclosed content exceptions become acknowledged warnings. App-managed resolution/install remains wheel-only; scripts may invoke their own builds. Source builds are never authorised by node IPC |
| Webview isolation | A strict CSP (`default-src 'self'`; scripts and connections limited to self and Tauri IPC). The ComfyUI window has no capability. Commands are allow-listed to the manager window only. External links open in the system browser |
| Tool integrity | uv, MinGit, managed Python archives and every `hf` and `gdown` tool artifact have pinned SHA-256 values, installed with hash checking from HTTPS only |
| LAN exposure | ComfyUI listens on `127.0.0.1` by default. LAN listening needs a settings change and an acknowledgement |
| Git safety | Git operations never reset, clean or discard local changes. Node updates are fast-forward only. ComfyUI updates fast-forward, or check out a frozen target only on a clean tree with no unpublished local commits. Git runs with an app-owned empty global configuration, `GIT_CONFIG_NOSYSTEM=1` and a non-executing `git config --local --no-includes` preflight that refuses `filter.*`, includes and worktree redirection |
| Native-image boundary (Windows) | Before consent, new node content whose top-level folder has a `.dll`, `.exe`, `.com`, `.cpl`, `.drv`, `.ocx`, `.scr` or `.sys` entry is refused: Git sources from the exact commit's NUL-delimited `git ls-tree` output before checkout (read as raw bytes, 8 MiB cap, 120 s), Registry archives after extraction into staging. Only selected curated Git sources with persisted first-run consent turn this into an acknowledged warning. Installed and user-placed nodes are not re-checked (residual R06 after consent) |
| Child processes | Children without an explicit working folder start in their executable's folder, never in the manager's launch folder. The `SetDllDirectoryW("")` policy covers the application process only and is not inherited by children |

**Pinned containment for custom nodes.** Every node mutation (staging, publication, backup, enable and disable, restore, recycling) runs through retained directory handles for the install root, `ComfyUI`, `custom_nodes`, `.disabled` and staging, so a parent swapped for a link between the check and the use cannot redirect the operation. Dependency and `install.py` subprocesses receive guarded inputs rather than plain paths. Registry archives are written through an already-open, create-new file and extracted from the same handle. All reparse points are refused in node trees, so OneDrive placeholders and deduplicated or WOF-backed entries are unsupported there, and same-user drive-letter remapping (`subst`, `net use`) is outside the boundary (residual R05). This is covered by Windows tests, including real junctions. On Linux the guarantee is limited: pinned directories keep their identity but do not freeze names or contents, and the rename and unlink helpers check identity and then act by name. The Linux-only code has never been compiled or run.

**Git content identity.** Git node reviews are bound to a content digest of the whole staged tree, not just the commit, so edited, untracked or ignored files change the identity. Updates stage a full checkout of the frozen target commit, and the live fast-forward is validated against the reviewed digest before any dependency or script runs. A refused update keeps a bounded, redacted `git status`.

## Known limitations and open validation items

This list is the honest state of validation, not a roadmap. The release verdict is **NO-GO**; [RELEASE_ACCEPTANCE.md](RELEASE_ACCEPTANCE.md) maps every requirement and scenario to evidence or an open gate.

**What has been verified natively (Windows only)**

All on one machine: Windows 11, NVIDIA GeForce RTX 3060 12 GiB, driver 610.88 (CUDA 13.3), 2026-10-20, using debug builds with the production asset protocol on fresh install roots. This is not clean-machine or installed-release acceptance.

- First-time setup on `win-py312-torch2100-cu130` with ComfyUI-Manager and curated nodes, final verification and ComfyUI 0.42.1 on `cuda:0` (N8–N13, N28).
- Wizard defaults and order: recommended profile first, Latest release, Manager ticked, curated nodes unticked (N25, N26).
- ComfyUI branch tracking, Update to latest commit and switching tags (N18, N21, N22).
- The `extra_model_paths.yaml` generator with a Thai base path (N15–N17).
- Moved-install rebuild on a copy whose original root still exists (N19, N20).
- A Hugging Face Xet download through `hf` (N23, N24).
- A Custom Nodes Git install with `install.py` before requirements and unchanged pins (N29).
- From session 5: a rename while ComfyUI ran, a locked-file refusal and the Recycle Bin.

**Not established by any test result**

- Linux: nothing has been compiled or run, including the Linux-only containment code, Secret Service, freedesktop trash, WebKitGTK behaviour, Thai fonts and the AppImage and .deb bundles. Windows results do not carry over.
- Windows installers: built but never installed or run; the release executable has never been launched. They are unsigned (no Authenticode certificate), and the application uses placeholder icons.
- Provider scenarios that need credentials or fixtures: CivitAI downloads with and without a token, gated Hugging Face repositories, Google Drive confirmation and quota, the token Test buttons, and the secret-redaction check after a real token download.
- Further Windows scenarios: custom venv, change torch, the moved install with the original root absent, Hugging Face pause/resume and missing-tool fallback, a lock held by a loaded model, Registry node install, an edited Git node, unsupported hardware, crash recovery, the maintenance race and a failed dependency install with Repair.
- Accelerator wheels: no accelerator has been installed on real hardware. The Python 3.12 wheels prove artifact identity only and are experimental; FlashAttention 3's import module name is unverified. Wheel sources for Python 3.13 and torch 2.14.1 are not defined (REQUIREMENTS section 9).
- Whether the native Hugging Face engine can fetch Xet-backed files, and whether a later `hf` release restores cross-run resume.
- Performance targets, the accessibility audit (contrast, keyboard, 200 % zoom) and coverage (`cargo llvm-cov`) have not been measured.
- Updater key custody and the signed-update flow.

**Known differences and limits**

- The PyTorch 2.14.1 profiles omit `torchaudio` (no matching wheel); nodes that need it fail to import there.
- In-place dependency changes are not rolled back. A failed `uv pip` or `install.py` step marks the environment repair-required, and the application does not snapshot venvs.
- Downloads always go to `ComfyUI/models`, not to a generated extra-model base folder.
- ComfyUI-Manager installed by setup is listed as Observed with dependencies NOT RUN.
- Non-consented setup modes still skip a curated node's `install.py`; reinstall the node in Custom Nodes and approve it if needed.
- One unreachable curated repository stops the curated step until it is skipped, and skipping skips every curated node not yet installed.
- After a failed self-update install, downloads stay stopped until the application restarts.
- Move detection needs a valid `install.json` (residual R21).
- Only one ComfyUI install is managed at a time.
- No license has been selected. The application does not add one on the owner's behalf.
- AMD, Intel, macOS, CPU-only and multi-install support are out of scope for release 1.
