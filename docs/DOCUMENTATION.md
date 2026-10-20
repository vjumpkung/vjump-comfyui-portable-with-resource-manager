# vjumpkung-comfyui-desktop manual

This manual has two parts: a user guide (Part 1) and a developer guide (Part 2). For the normative requirements, see [REQUIREMENTS.md](REQUIREMENTS.md). For the project front page, see the [README](../README.md).

## How to read the status markers

This project is under active development. Most of this manual describes behaviour that exists in the code. A few passages describe work that is **approved in a plan but not yet implemented or verified**. Those passages are preceded by an HTML comment starting with `VERIFY:` (invisible when rendered) so that a final pass can confirm or delete them. In the rendered text, such features are introduced with the words "planned" or "in progress".

Nothing in this manual states that the application has been accepted on NVIDIA hardware on Linux, or that a release is ready. See [Known limitations and open validation items](#known-limitations-and-open-validation-items).

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

Release 1 targets **Windows 10/11 x64** and **Linux x64**, with **NVIDIA GPUs only**. There is no published release yet, so these instructions describe the intended bundles.

### Windows

1. Run the NSIS installer (`vjumpkung-comfyui-desktop_<version>_x64-setup.exe`) or the MSI (`vjumpkung-comfyui-desktop_<version>_x64_en-US.msi`). The NSIS installer installs for the current user only.
2. If the WebView2 runtime is missing, the bundle downloads and installs it silently through the WebView2 bootstrapper.
3. Optional but recommended: enable Windows long paths (`LongPathsEnabled`) and pick a short install root. The wizard shows whether long paths are enabled.

A current NVIDIA driver is required. The driver's CUDA level decides which profiles are available (see [Profiles](#profiles)).

The locally built Windows installers are unsigned and have not been run end to end. Expect an "unknown publisher" warning from Windows.

### Linux

1. Install the runtime dependencies. The `.deb` declares `libwebkit2gtk-4.1-0` and `libgtk-3-0`. The AppImage needs the same libraries on the host.
2. Install **Git 2.40 or newer** from your distribution. The application does not bundle Git on Linux. Without it, node and ComfyUI operations that need Git report that Git is unavailable.
3. For stored tokens to persist, a Secret Service provider (for example GNOME Keyring or KWallet) must be running. Without one, tokens are kept in memory for the current session only.
4. Install the `.deb` with your package manager, or mark the AppImage executable and run it.

Linux bundles have not been built or tested yet.

### Single instance

Only one copy of the application runs at a time. Starting it a second time focuses the existing window.

## 2. First-run setup wizard

When there is no valid installation, the application opens the setup wizard. It has six stages: **Install root, Hardware, Python source, Profile, Options, Review**. After the review, the installation runs and shows live progress.

### Install root

The install root is the folder that will hold ComfyUI, the Python environments, the bundled tools and the application's data. The default is `%LOCALAPPDATA%\vjumpkung-comfyui` on Windows and `~/.local/share/vjumpkung-comfyui` on Linux. Choose a short path with no unusual permissions. You can change the install root only here, or later by running setup again.

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

Seven profiles are bundled. All use PyTorch from `download.pytorch.org`, a Triton build and a verification step.

| Profile | Python | PyTorch | CUDA | Status |
|---|---|---|---|---|
| `win-py313-torch2141-cu130` | 3.13.16 | 2.14.1 | 13.0 | Stable, recommended (default on Windows) |
| `win-py313-torch2141-cu126` | 3.13.16 | 2.14.1 | 12.6 | Stable, for drivers without CUDA 13 |
| `win-py314-torch2141-cu130` | 3.14.8 | 2.14.1 | 13.0 | Experimental |
| `win-py312-torch2100-cu130` | 3.12.15 | 2.10.0 | 13.0 | Stable compatibility profile |
| `linux-py313-torch2141-cu130` | 3.13.16 | 2.14.1 | 13.0 | Stable, recommended (default on Linux) |
| `linux-py313-torch2141-cu126` | 3.13.16 | 2.14.1 | 12.6 | Stable |
| `linux-py314-torch2141-cu130` | 3.14.8 | 2.14.1 | 13.0 | Experimental |

"Stable" describes the profile's status label in the bundle, not a validation result on real hardware. Only `win-py313-torch2141-cu130` has been run on a real NVIDIA machine (Windows, 2026-10-16).

Notes:

- The PyTorch 2.14.1 profiles install `torch` and `torchvision` only. No matching `torchaudio` wheel was published for 2.14.x when the profiles were prepared. ComfyUI v0.40.0 does not require it. The 3.12 compatibility profile installs `torchaudio` 2.10.0.
- Triton is `triton-windows` on Windows and `triton` on Linux, at the version the profile pins.
- Profiles with CUDA 12.6 require a driver that supports CUDA 12.6. Profiles with CUDA 13.0 require 13.0.

### Options

- **ComfyUI version.** Choose the latest release or enter an exact tag. Browsing tags is not offered in the wizard. After setup, the ComfyUI page has a version picker.
- **ComfyUI-Manager.** Optional and off by default. Selecting it shows the source and the exact commit, and you must approve running its code. When it is installed, you can launch ComfyUI with its built-in Manager enabled.
- **Clean caches.** Optional final step that removes download caches to save disk space.

<!-- VERIFY: M6 plan E1 (m6-approved-plan.md "E1 - Hashed accelerators"): the accelerator selection list in the Options stage, the "reset to profile defaults on profile change" behaviour, and the per-component results are not yet implemented -->
- **Accelerators** (planned). For the Python 3.12 compatibility profile only, a list of optional accelerator wheels (SageAttention, FlashAttention 2 and 3, onnxruntime-gpu, llama-cpp-python) and a "compatibility packages" component (`transformers==5.5.0`, `onnx`, `numpy<2.3`, `pillow<12`). Each wheel is downloaded from its publisher and checked against a pinned SHA-256 before it is installed. A matching hash proves the file is the expected artifact. It does **not** prove the wheel works with your Python, torch, CUDA and GPU, so these wheels are marked **experimental** and start unchecked. Failure to install or import an accelerator is a warning, not a setup failure. The Python 3.13 and 3.14 profiles have no accelerators.

<!-- VERIFY: M6 plan E2 (m6-approved-plan.md "E2 - Curated nodes through M5, before activation") and m6-reference-data.md: the curated-nodes list in the Options stage, the 22-entry catalog, the batch review dialog and the "install.py never runs during setup" rule are not yet implemented -->
- **Curated custom nodes** (planned, first-run only). A list of 22 recommended nodes, each pinned to an exact Git commit, mirroring the reference installer. 21 of the 22 are ticked by default; ComfyUI-PlagueKind-Nodes is listed as unavailable at its pinned commit (it ships `__pycache__`). Ticking a node does not authorise running its code. Setup pauses on the same review and consent dialog as the Custom Nodes page. During setup, `install.py` scripts are never run and source trust is never remembered. After setup, use the Custom Nodes page to retry a node's script step.

### Review and install

The Review stage lists your choices. When you start, the installation runs these steps in order, each idempotent on retry:

1. Preflight checks.
2. Fetch and verify the pinned tools: uv, MinGit (Windows only) and the isolated `hf` tool environment.
3. Create the Python environment.
4. Seed `pip`, `setuptools` and `wheel` (plus an app-local MSVC runtime on Windows).
5. Clone ComfyUI at the selected tag.
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

If ComfyUI-Manager needs consent, the Retry button is held until you approve its source.

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

<!-- VERIFY: M5 backend plan WP5 (m5-backend-approved-plan.md): node requirements are restored during Repair/replacement and any node requirement failure blocks activation in every replacement mode -->
**Custom nodes during re-setup** (in progress). The new environment reinstalls the dependency requirements of installed custom nodes, enabled and disabled. If a node's requirements fail to install, the new environment is **not** activated and the old one stays active. A repair-required environment always gets a full replacement.

## 4. Moved install and guided rebuild

<!-- VERIFY: M6 plan E3 (m6-approved-plan.md "E3 - Missing-root recovery and guided rebuild") and REQUIREMENTS FR-SETUP-12: detection of a moved install root or broken interpreter, the Locate/New actions, the Rebuild mode and its database path rebasing are not yet implemented -->
This section describes **planned** behaviour (FR-SETUP-12).

A Python venv records absolute paths, so it stops working when you copy or move the install folder. Release 1 does not try to make a moved folder run unchanged. Instead:

- On launch, if the recorded install root differs from the current one, or the active venv's interpreter no longer works, the application offers a **guided rebuild** and normal commands return `rebuild_required` until you finish it.
- If the install location the application remembers no longer exists, you can **locate an existing installation** (you point to an existing install, and the application restarts using it) or **start a new install** in an empty folder. Neither option overwrites an existing installation.
- A rebuild creates a **new app-owned venv** in the current root using the exact profile revision and component choices that were recorded at the original setup. It never mutates or deletes an adopted venv.
- The ComfyUI checkout is reused as is (no fetch, checkout or reset), and local edits are preserved. Requirements of enabled and disabled custom nodes are reinstalled. No `install.py` scripts run, and no newer node versions are fetched.
- The `hf` tool environment is recreated, because it also records absolute paths.
- Models, nodes, `user/`, `input/`, `output/`, external model roots and launch options are preserved. Paths stored in the database that point into the old root are rebased to the new root. Paths outside the old root are left alone.
- If the recorded profile revision is no longer available, the rebuild is refused with `profile_revision_unavailable`.
- If an earlier activation, ComfyUI update or node operation was left half-finished, the application preserves it and reports `relocation_recovery_required` instead of rebuilding.

## 5. Environment page

The Environment page shows:

- the active environment: Python version and path, profile, torch and CUDA build, Triton, ownership (app-owned or adopted) and the verification report;
- the **protected pins**: the exact `torch`, `torchvision`, `torchaudio` and Triton versions that the pin guard defends;
- installed packages, with search;
- package operations;
- re-setup actions;
- deletion of inactive environments.

<!-- VERIFY: M6 plan E1 and "I" (m6-approved-plan.md): the Environment page showing stored accelerator install/import results is not yet implemented -->
In a planned update, the page also shows the stored accelerator results for the environment (installed, failed or skipped, and whether the import test passed).

### Installing, upgrading and uninstalling packages

Choose **Install**, **Upgrade** or **Uninstall**, enter package names and **Preview**. The preview shows what would be installed, upgraded or removed. The pin guard resolves the request with the protected packages held fixed.

If the resolver needs to change a protected package, the preview says so and you must confirm those exact changes. Without confirmation the change is not applied. Package operations require ComfyUI to be stopped. The application offers to stop it.

Package changes are not rolled back. If an operation fails partway, the environment is marked **repair required** (see [Troubleshooting](#repair_required)). The application records that intent before it starts a mutating install, and returns the environment to the verified state only after a successful post-install verification.

### Deleting an inactive environment

Under "Delete an inactive environment", type the environment's recorded ID and confirm by typing it again. Only inactive environments can be deleted. Adopted environments are never deleted by the application.

## 6. ComfyUI page

### Start, stop and restart

The page and the top bar have Start, Stop and Restart. ComfyUI runs as a child process that the application owns, started as `python -s main.py --listen 127.0.0.1 --port <port>` plus your options. The application waits for ComfyUI to become ready before it reports Running.

Start first checks: custom-node recovery state, a leftover ComfyUI process, a pending update, the selected environment and the port. Start, stop, update and package operations are serialised, so only one runs at a time.

<!-- VERIFY: M5 backend plan WP6 (m5-backend-approved-plan.md "WP6"): Start/Restart/Stop checks and node rescans under one maintenance lease, and the node_recovery_required / node_rescan_failed errors -->
Start is refused with `node_recovery_required` while an interrupted custom-node operation needs resolving, and Stop reports `node_rescan_failed` if the node folders could not be re-scanned after ComfyUI stopped (in progress).

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

The versions panel offers the latest release, a fast-forward of the current branch, and available tags. An update may stop a running ComfyUI, reinstall `requirements.txt` under the pin guard, and then verify the environment. Updates use fast-forward only. If the checkout has local changes or has diverged, the update is refused, shows the Git status, and changes nothing. The application never resets or force-checks-out your files.

Switching to an older tag does **not** restore the older package set. The panel warns about this. A pending update must be completed or retried before ComfyUI can start.

### extra_model_paths.yaml

The page shows the model paths read from `ComfyUI/extra_model_paths.yaml`. It is read-only in the application. The parser expands `~`, `$VAR`, `${VAR}` and Windows `%VAR%`, and resolves relative paths against the ComfyUI folder. The model library scans these folders too.

<!-- VERIFY: M6 plan P (m6-approved-plan.md "Diagnostics"): the Copy diagnostics button on this page moving to the new app_diagnostics/app_copy_diagnostics commands, the extended report, and the textarea fallback -->
**Copy diagnostics** copies a report to the clipboard. In the planned version the report covers the application version, OS, GPU and driver, the active environment and its pins, accelerator outcomes, the ComfyUI revision, and the last 200 log lines, all redacted. If the clipboard is unavailable, the text is shown in a selectable box instead.

## 7. Downloads

The Downloads page has an **Add download** dialog, **Import pack**, **Export** and the persistent queue. The queue is stored in the database and survives restarts. Jobs that were running when the application stopped come back as paused.

### Supported links

| Provider | Accepted forms |
|---|---|
| **CivitAI** | On `civitai.com`, `civitai.green` and `civitai.red`: model pages (`/models/{id}`, with optional `?modelVersionId=`), `/api/model-versions/{version}`, `/api/v1/model-versions/{version}`, and `/api/download/models/{version}`. Query filters `fileId`, `type`, `format`, `size` and `fp` select among a version's files |
| **Hugging Face** | `huggingface.co` model, dataset and space repo URLs, and `resolve`, `blob` or `tree` URLs with a revision and path. A repository URL opens a file list where you choose files, and one job is created per file. Transfers are pinned to the resolved commit |
| **Google Drive** | `/file/d/{id}` and `?id={id}` forms on `drive.google.com` or `drive.usercontent.google.com`. Folders are rejected |
| **Direct link** | Any other HTTPS URL. A plain `http://` link needs explicit confirmation. The filename comes from the server's `Content-Disposition` header, then from the URL |

You choose a target category (a `ComfyUI/models/` subfolder) and can set the filename. When a provider lists several files, you pick the variant. A token is needed for some CivitAI and Hugging Face downloads (see [Settings](#10-settings)). Clear errors distinguish a missing token, a gated repository, early-access content and not-found.

### Hugging Face: `hf` CLI versus native engine

Hugging Face downloads use one of two engines, set in **Settings > Hugging Face engine**:

| Engine | How it works | Pause and resume |
|---|---|---|
| **hf CLI** (default) | The pinned official `hf` tool, in its own isolated environment, supports Xet-backed files. Progress is an estimate, measured from bytes written to disk | `hf` does not resume across runs. **Pause stops the download, and resume, an application restart and network-failure retries all start the file again from 0%.** The queue labels these actions "will restart from 0%" |
| **Native** | The application's own HTTP downloader | Resumes using HTTP Range requests when the server sends a strong `ETag`. Without one, the partial file is discarded and the download restarts from 0 |

If the `hf` tool is missing or cannot be launched, the job falls back to the native engine. Other `hf` errors (authentication, gated, not found, hash mismatch) do not trigger the fallback. Non-Hugging Face sources always use the native engine. CivitAI links re-resolve on resume, because their signed URLs expire. Whether the native engine can fetch Xet-backed files has not been validated. If it cannot, it fails with a clear error suggesting the `hf` engine.

### States, retries and safety

Job states are queued, resolving, downloading, verifying, paused, completed, failed and cancelled. Per job you can pause, resume, retry, cancel, or resolve a filename conflict.

- **Retries.** Network errors and server (5xx) errors are retried automatically up to three times with increasing delay. Client (4xx) errors are not, except a 403 on an expired signed link, which triggers a fresh lookup.
- **Duplicates.** A job with the same provider identity as an active job is rejected as a duplicate. If the library already holds a file with the expected SHA-256, you can skip or download again.
- **Name conflicts.** If the target file exists, you choose: overwrite (the old file goes to the Recycle Bin or Trash), keep both (`name (1).ext`), or skip.
- **Integrity.** Data is written to a temporary file, synced, verified (SHA-256 when the provider supplies one, otherwise marked unverified) and only then renamed into the models folder. A failed hash leaves nothing in the models folder.
- **Concurrency.** The number of simultaneous downloads is a setting (default 3, range 1 to 32). Lowering it affects new downloads only. Running transfers continue.

<!-- VERIFY: M6 plan B (m6-approved-plan.md "B - One live bandwidth budget"; HANDOFF 5.3): the bandwidth cap setting, shared limiter, and the HF relay on every Hugging Face attempt are partly written in a worktree and never compiled or tested -->
### Bandwidth cap (in progress)

Settings has a **Bandwidth cap (KiB/s)** field. Leave it empty for no limit. When set, one shared budget applies to native downloads and to Hugging Face `hf` CLI downloads together, and a change takes effect for downloads already running. To do this, every Hugging Face `hf` download is routed through a local relay on `127.0.0.1` that only forwards connections to `huggingface.co`, its subdomains and `hf.co`, even when no cap is set. If the relay cannot start, the job fails with `bandwidth_proxy_unavailable` rather than silently running uncapped. Your system proxy settings are passed through to the relay's upstream connection.

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

The Models page lists the files under `ComfyUI/models` and under every folder in `extra_model_paths.yaml`. An unavailable root is reported. The scan does not follow links and does not hash files.

- **Find.** Search by name, filter by category and root, and sort by name, category, size or modified time. The page rescans when files change, polls every 30 seconds, and has a Rescan button. If file watching is degraded, the page says so.
- **Details.** Select a model to see its details, sidecar files and preview.
- **Rename.** A rename stays in the same folder. Changing the extension needs your acknowledgement. Sidecar files with the same stem (for example `.preview.png`, `.civitai.info`, `.json`) are renamed too. A case-only rename works on Windows.
- **Move.** Move a model to another category, subfolder or root.
- **Delete.** Select one or more files. The dialog shows what will be moved to the **Recycle Bin or Trash**. If moving to the trash fails, the application reports it and offers **permanent delete** as a separate action that you must confirm. It never falls back to permanent deletion on its own.
- **Locked files.** On Windows, a model loaded by a running ComfyUI is locked. The operation fails without changing anything and offers Retry or "Stop ComfyUI and retry". Rename, move and delete are allowed while ComfyUI runs.
- **Identify on CivitAI.** Computes the file's SHA-256 and looks it up on CivitAI to show the model's details and a preview. Identification hashes the file on demand, which can take a while for large files.

Interrupted operations are recovered the next time the library rescans.

## 9. Custom nodes

The Custom Nodes page has a **Registry browser** and a list of **installed nodes**.

### Browsing the Comfy Registry

The browser searches and pages through the Comfy Registry. For each node it shows name, publisher, description, downloads, GitHub stars, latest version, supported OS, accelerators and ComfyUI versions, and warns when a node is flagged, deprecated or incompatible with your system. Results are cached; the cache lifetime is **Settings > Registry cache lifetime** (default 6 hours). **Refresh** forces a fresh request. When the Registry is unreachable, the page shows the last cached results and says they are stale.

### Installing

- **From the Registry.** Choose a node and optionally an exact version. Empty means the latest version.
- **From Git.** Enter an HTTPS repository URL and optionally a branch, tag or commit.

Either way the application first **prepares** the install and shows a review. Git sources are resolved to an exact commit and cloned to a staging folder. Registry archives are downloaded and extracted with archive hardening. Nothing is published into `custom_nodes/` until you consent.

### The review and consent

The review shows the exact resolved source (Registry id and version, or Git URL and commit), the previous source for updates, the node's dependency requirements, compatibility and source warnings, and any change to protected packages.

You must tick **execute** consent for the exact plan. The review warns that:

- installing a node runs third-party code;
- a Python venv is not a sandbox;
- dependency changes cannot be rolled back.

Other checkboxes:

- If the node has a root `install.py`, a separate checkbox lets you run it. It is unchecked by default and is never implied by anything else.
- Warnings and protected-package changes need their own explicit confirmations.
- "Remember this source" lets the application remember a trusted source (a Git repository, or a Registry node with a publisher and repository). A remembered source never replaces the per-install execute consent.

Closing an idle review discards the prepared plan.

<!-- VERIFY: M5 backend plan WP3 (m5-backend-approved-plan.md "WP3"): node dependency previews refuse protected-package changes outright (no unconstrained retry) -->
**Protected packages** (in progress). Installing a node must not change `torch`, `torchvision`, `torchaudio` or Triton. If a node's requirements conflict with the pins, the install fails with a resolution error and nothing changes. To deliberately change those packages, use the Environment page's package workflow.

### Updating, enabling, disabling, uninstalling

- **Update.** Registry nodes are installed from the newer version in a staging folder and swapped in, and the old folder goes to the trash. Git nodes use fast-forward only. A repository with local changes, an unfinished operation or a diverged history is **refused**, and your files are left alone.
- **Update all** prepares reviewed plans one node at a time, then applies them sequentially. Nodes that need an explicit target (a detached HEAD, or no upstream) are skipped with a message. The batch stops if a dependency failure damages the environment.
- **Enable / Disable.** Disabling moves the node folder to `custom_nodes/.disabled/<name>`, the same convention ComfyUI-Manager uses. Enabling moves it back.
- **Uninstall.** The node folder moves to the Recycle Bin or Trash. Python packages stay installed, because other nodes may share them. There is no permanent-delete fallback.

<!-- VERIFY: M5 frontend plan WP5 (m5-frontend-approved-plan.md "Update all") : groups of at most 128 plans reviewed group by group; Git status inspected before Update -->
In progress: "Update all" works in reviewed groups of at most 128 plans, and **Update** on a Git node inspects and displays the real `git status` first.

All node operations need ComfyUI stopped and run one at a time. After a successful operation the page offers **Restart ComfyUI**.

### Dependencies and repair

After a node is published, the application installs its `requirements.txt` into the active environment under the pin guard. A node's own Python dependencies can fail in ways the application cannot undo. When that happens:

- the node is marked **dependencies failed**;
- the environment is marked **repair required**;
- you can **Retry dependencies** (which prepares and reviews a plan again, and never runs a script merely because an earlier install allowed it) or **Repair** the environment from the Environment page or Settings.

Retry dependencies is also offered for nodes whose dependency state is "not run", including manual directories.

### Nodes the application did not install

Nodes already in `custom_nodes/` (including ones installed by ComfyUI-Manager) are listed as **manual** or unmanaged. They can be disabled and uninstalled. They cannot be updated by the application. If ComfyUI-Manager is present, the application re-scans after ComfyUI stops, so changes made through the Manager appear.

### Recovery after interruptions

Node operations are journaled. If the application or system stops mid-operation, the next scan reconciles it: unpublished staging is removed, and copies that cannot be proven safe are **preserved** rather than deleted. Use **Rescan / finish recovery** on the page to run reconciliation. The operations panel shows unresolved operations, recent results and per-operation logs.

<!-- VERIFY: M5 backend plan WP4 (m5-backend-approved-plan.md "WP4"): identity-based recovery, final tree digest, and NodeOperation::blocks_start making ambiguous recovery block ComfyUI launch -->
In progress: ambiguous recovery states (for example a changed or foreign copy) block ComfyUI from starting until resolved, while ordinary cleanup that failed only because the trash was unavailable does not. The recovery panel marks which operations block launch.

<!-- VERIFY: M5 frontend plan WP1 (m5-frontend-approved-plan.md): pendingReviews/recent in node_list so a reloaded UI can "Resume review" and see recent failures, and node_git_status -->
In progress: a reloaded page can offer **Resume review** for prepared plans (consent is never restored) and keeps recent failed operations visible.

### Git requirements

Git-based operations need Git. On Windows the application uses its pinned MinGit. On Linux it uses system Git 2.40 or newer. If Git is missing, the page reports that Git is unavailable and tells you to complete tool setup (Windows) or install Git (Linux).

## 10. Settings

### Preferences

| Setting | Default | Valid values | Notes |
|---|---|---|---|
| Download concurrency | 3 | 1 to 32 | Applies to newly started downloads |
| Hugging Face engine | hf CLI | hf CLI, Native | See [Downloads](#hugging-face-hf-cli-versus-native-engine) |
| Close behaviour | Ask every time | Ask, Stop ComfyUI and quit, Keep ComfyUI running in the background | |
| Theme | System | System, Light, Dark | Also toggled from the top bar |
| Registry cache lifetime | 6 hours | 1 or more | How long Registry results are cached |
| Bandwidth cap (KiB/s) | Unlimited | 1 or more, or empty | See the bandwidth cap section |

<!-- VERIFY: M6 plan B and "I" (m6-approved-plan.md): the Bandwidth cap field is enabled in the UI and takes effect; in the committed UI the field is disabled and says downloads run unthrottled -->
The bandwidth cap is part of in-progress work. In the last committed version the field is disabled and downloads run unthrottled.

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

<!-- VERIFY: M6 plan P (m6-approved-plan.md "Updater"): Updates panel with app_update_status/app_update_check/app_update_install; shown only when the build has VJUMP_UPDATER_PUBKEY and VJUMP_UPDATER_ENDPOINT; no check-on-launch setting -->
### App updates (planned)

When the application was built with an update public key and endpoint, Settings shows an **Updates** panel. **Check for updates** looks for a newer signed release and shows its version, date and notes as plain text. Installing it requires ComfyUI to be stopped, verifies the update's signature and installs exactly the release you reviewed, then restarts the application. There is no automatic check on launch. Builds without the key and endpoint have no update action, and the application never updates itself in that case.

<!-- VERIFY: M6 plan P (m6-approved-plan.md "Diagnostics"): Settings diagnostics panel -->
### Diagnostics (planned)

A **Copy diagnostics** action copies a redacted support report. See [ComfyUI page](#6-comfyui-page).

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

<!-- VERIFY: M5 backend plan WP6 and WP4: the node_recovery_required error on Start and when ambiguous states block launch -->
(In progress.) A custom-node operation was interrupted and the application could not prove which copy of a node is safe. ComfyUI will not start until you run **Rescan / finish recovery** on the Custom Nodes page and resolve what it reports. Preserved copies stay on disk.

### rebuild_required

<!-- VERIFY: M6 plan E3 (m6-approved-plan.md): rebuild_required / root_missing / root_invalid / relocation_recovery_required error codes and the guided rebuild flow -->
(Planned.) The install folder was moved or copied, or the venv's interpreter no longer works. See [section 4](#4-moved-install-and-guided-rebuild).

### Why does pausing a Hugging Face download restart from 0%?

The default `hf` CLI engine cannot resume across runs. Switch **Hugging Face engine** to Native to get resumable downloads, at the cost of not using the CLI's Xet support. See [Downloads](#7-downloads).

### Why is there no torchaudio?

The PyTorch 2.14.1 profiles omit it because no matching 2.14.x wheel existed. See [Profiles](#profiles).

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
| `<root>/install.json` | Marker recording the install root, active environment and profile |
| `<root>/data/app.db` | SQLite database: settings, environment records, launch options, download queue, model catalogue, node records and the Registry cache. Contains no tokens or signed URLs |
| `<root>/data/cache/` | `uv`, `uv-python`, `pip`, `hf`, `torch` and `triton` caches |
| `<root>/data/downloads/` | In-progress downloads (temporary files) |
| `<root>/data/tmp/hf` | Temporary area for `hf` CLI downloads |
| `<root>/data/logs/` | Log files |

<!-- VERIFY: M6 plan E1 (m6-approved-plan.md): wheel cache location data/cache/wheels/<sha256>/<filename> -->
In planned accelerator support, verified wheels are also cached under `<root>/data/cache/wheels/<sha256>/`.

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
    migrations/              001_settings.sql ... 006_custom_nodes.sql
    src/                     comfy_runtime, custom_nodes, environment, models, settings, shared
    tests/                   Integration tests (workflow, real_setup)
  desktop/
    src/ipc/                 Commands, DTOs, bindings.rs (command list)
    capabilities/manager.json
    permissions/autogenerated/   Generated, do not edit
    tests/                   acl.rs, bindings.rs, navigation.rs
    build.rs                 App manifest: the command list for the ACL
    tauri.conf.json (+ .windows, .linux, .updater overlays), icons/
ui/
  src/{app,features,shared}  React/TypeScript frontend
  package.json               pnpm 10.24.0 and scripts
docs/                        REQUIREMENTS, DESIGN, ADRs, IMPLEMENTATION_STATUS, handoff/
scripts/                     tauri.mjs, check-arch-deps.mjs (+ test and fixtures)
.github/workflows/           ci.yml, release.yml (both disabled)
```

## 15. Toolchain setup

### Common

- Rust: install `rustup`. The toolchain in `rust-toolchain.toml` (`1.99.0`, with rustfmt and clippy) is selected automatically.
- Node.js and pnpm: pnpm `10.24.0` is pinned by `packageManager` in `ui/package.json`. No Node version is pinned (the disabled workflows reference Node 24). Enable Corepack or install the pinned pnpm.
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

Install the Tauri build prerequisites (as in the disabled release workflow):

```bash
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
  librsvg2-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev \
  build-essential patchelf
```

Also install Git 2.40 or newer. The project has not yet been built or tested on Linux.

### Memory

The primary development machine has limited RAM. Run at most two Rust builds at once and use `-j 4`.

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
| Production-protocol build | `cargo build -p desktop --locked --features tauri/custom-protocol` |

The full check list, as used for handoff:

```bash
pnpm -C ui install --frozen-lockfile && pnpm -C ui lint && pnpm -C ui typecheck && pnpm -C ui test && pnpm -C ui build
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -j 4 -- -D warnings
cargo test --workspace --locked -j 4 --no-fail-fast
node --test scripts/check-arch-deps.test.mjs && node scripts/check-arch-deps.mjs
cargo test -p desktop --test bindings --locked
cargo build -p desktop --locked --features tauri/custom-protocol
pnpm -C ui tauri build
```

Notes:

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

<!-- VERIFY: M6 plan "I" (m6-approved-plan.md): eight new commands setup_list_curated_nodes, setup_open_install, setup_new_install, app_update_status, app_update_check, app_update_install, app_diagnostics, app_copy_diagnostics, with valid-body ACL cases and recovery-mode handlers; no updater/clipboard plugin IPC permissions granted to webviews -->
Eight further commands are planned for M6 (curated node list, open and new install for recovery, updater status, check and install, and two diagnostics commands). Updater and clipboard plugin permissions are not granted to any webview. Recovery-mode commands must work when the normal services are unavailable.

## 19. Database migrations

The application database is SQLite (`rusqlite`, bundled) at `data/app.db`. The single ordered migration list lives in `crates/infrastructure/src/shared/sqlite.rs` (`migrations()`), using `rusqlite_migration`. Contexts append to this list. They never keep their own `user_version` sequences.

To add a migration:

1. Create `crates/infrastructure/migrations/NNN_name.sql` with the next number (currently the last is `006_custom_nodes.sql`). Migrations are additive.
2. Add `M::up(include_str!("../../migrations/NNN_name.sql"))` to `migrations()` in order.
3. Update any test helper that builds a partial migration list (for example in the model library tests).
4. Test the migration in the owning context's tests.

<!-- VERIFY: M6 plan E1 and E3 (m6-approved-plan.md): migrations 007_environment_components.sql (components_json column) and 008_install_relocation.sql; M5 backend plan WP5 says no migration 007 is needed for node dependency state -->
Two further migrations are planned for M6: `007_environment_components.sql` (stored accelerator and component results) and `008_install_relocation.sql` (relocation records). Their numbers may change if the order of work changes.

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
| `accelerators` | Optional accelerator list. Empty for all bundled profiles in the committed version |
| `constraints` | Extra resolver constraints |
| `verify` | Expected torch version prefix, require CUDA, require Triton |
| `disk` | Disk budget (`env_bytes`, `other_bytes`, `margin_bytes`, `calibrated`) |
| `provenance` | Notes on how the values were observed |

Eligibility checks the OS, an NVIDIA GPU, every detected GPU's compute capability and wheel architecture, the driver's CUDA level, and the disk budget. Pinned tool versions and SHA-256 values (uv 0.13.1, MinGit, `hf` 2.2.0 and its hash-locked requirements, managed CPython hashes, the app-local MSVC runtime) are in `crates/infrastructure/src/environment/pins.rs` and `data/`.

<!-- VERIFY: M6 plan E1 (m6-approved-plan.md "Contract"): schema 2 accelerator records (id, name, modules, source Wheel{url,filename,sha256,size}|Packages, no_deps, default_selected, non_fatal, provenance); schema 1 stays readable only with an empty accelerator list; the win-py312-torch2100-cu130 profile gets revision 2 with five experimental wheels -->
**Planned (M6, schema 2).** Accelerators become structured records with an id, an importable module list, and a source that is either a hashed direct wheel (HTTPS URL, safe `.whl` filename, SHA-256 and optional size) or package requirements, plus `no_deps`, `default_selected`, `non_fatal` and provenance. Schema 1 profiles remain readable only with an empty accelerator list. The `win-py312-torch2100-cu130` profile is planned to move to revision 2 with five experimental wheels and a "compatibility packages" component. The other profiles keep empty accelerator lists. FlashAttention 3's installed module name is unverified.

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
| Interpreter validation | `VJ_TEST_PYTHON=<path to CPython 3.12-3.14>` with the ignored Python test in `infrastructure/src/environment/python/tests.rs` | A suitable interpreter |
| Real `hf` download | Uses `VJ_REAL_HF_EXE`, `VJ_REAL_HF_REPO`, `VJ_REAL_HF_REVISION`, `VJ_REAL_HF_FILE` (in the Hugging Face proxy tests) | A built `hf` tool and network |

<!-- VERIFY: M6 plan B (m6-approved-plan.md tests): the opt-in test real_hf_xet_obeys_relay_with_direct_egress_denied and exact use of VJ_REAL_HF_* variables, which live in uncommitted worktree files -->
The `VJ_REAL_HF_*` variables appear in the uncommitted bandwidth work and may change.

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

The Windows bundles build locally and are unsigned. The installers have **not** been run. Linux bundles need a Linux host and have not been built.

**Updater.** The updater is inactive unless both `VJUMP_UPDATER_PUBKEY` and `VJUMP_UPDATER_ENDPOINT` are supplied at build time. The release workflow (disabled) also reads `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` from secrets, and passes the updater overlay only when a signing key exists. It fails early if a key is present but the public key or endpoint is missing. Key custody belongs to the owner. Never substitute a generated development key in a release.

<!-- VERIFY: M6 plan P (m6-approved-plan.md "Updater"): updater plugin pin =2.14.0 and clipboard plugin =2.5.0 (not yet in Cargo.lock), plugin registered only with complete build-time config, install bound to the reviewed offer via an opaque id, maintenance gate and runtime-stop before install, build-script rerun tracking, no updater/clipboard IPC permissions to webviews -->
**Planned updater design.** `tauri-plugin-updater` `=2.14.0` and `tauri-plugin-clipboard-manager` `=2.5.0` are to be added. The updater plugin is registered only when both variables are present and valid. The command set stays registered, and absent configuration returns a controlled refusal. Installing takes the maintenance gate, verifies that ComfyUI (including a headless instance) is stopped, and installs only the offer the user reviewed, identified by an opaque id kept in process memory. Cleanup happens before the Windows installer can exit the process. Compatibility of these plugin versions with the workspace is not yet proven by a build.

**Remaining packaging work** (plan section R): verify the overlays are picked up by the wrapper, that executable, Cargo and Tauri versions agree, and install, launch and uninstall all four bundle formats, including user-data preservation across update and uninstall.

## 25. Continuous integration status

Both workflow files, `.github/workflows/ci.yml` and `.github/workflows/release.yml`, are **commented out** (owner commit `chore: disable ci (temp)`). Do not re-enable them without the owner's decision. All checks are run locally with the commands in [section 16](#16-build-run-test-and-lint). The commented workflows document the intended CI: lint, test and bundle on Windows and Ubuntu.

## 26. Coding conventions

- Rust: rustfmt, four-space indentation, `snake_case` modules and functions, clippy with warnings denied.
- TypeScript: two-space indentation, single quotes, no semicolons. PascalCase components and types, kebab-case file names for features.
- Dependencies point inward. Keep serialisation (serde, specta) out of `domain` and `application`.
- Access Tauri from the UI only through `ui/src/shared/ipc/gateway.ts`.
- Pin dependency versions exactly. Both `Cargo.lock` and `pnpm-lock.yaml` are committed, and CI-style commands use `--locked` and `--frozen-lockfile`.
- Follow the design system in [DESIGN.md](DESIGN.md) for UI. New tokens or patterns update DESIGN.md first. Copy is English only.
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
| Consent | Installing a node, running `install.py` and building from source execute third-party code. The UI states that a venv is not a sandbox, shows the exact source, and requires explicit confirmation per reviewed plan. Source builds are never authorised by node IPC |
| Webview isolation | A strict CSP (`default-src 'self'`; scripts and connections limited to self and Tauri IPC). The ComfyUI window has no capability. Commands are allow-listed to the manager window only. External links open in the system browser |
| Tool integrity | uv, MinGit, managed Python archives and every `hf` tool artifact have pinned SHA-256 values, installed with hash checking from HTTPS only |
| LAN exposure | ComfyUI listens on `127.0.0.1` by default. LAN listening needs a settings change and an acknowledgement |
| Git safety | Git operations never reset, clean or discard local changes. Updates are fast-forward only |

<!-- VERIFY: M5 backend plan WP1 and m5-wp1-gap-decision.md: operation-time pinned-directory containment for all node mutations (staging, publication, backup, enable/disable, restore, recycling) and guarded subprocess inputs for requirements and install.py; registry archive download through an already-open create-new file. Linux guarantees are explicitly not yet established -->
**In progress: pinned containment for custom nodes (M5 WP1).** The plan is to perform every node mutation through retained directory handles for the install root, `ComfyUI`, `custom_nodes`, `.disabled` and staging, so a parent swapped for a link between the check and the use cannot redirect the operation, and to give dependency and `install.py` subprocesses guarded inputs rather than plain paths. Registry archives are to be written through an already-open, create-new file and extracted from the same handle. On Linux the guarantee is limited: pinned directories keep their identity but do not freeze names or contents, and the rename and unlink helpers check identity and then act by name. Native Windows and Linux verification (junction, symlink and substitution tests) has not been done.

<!-- VERIFY: M5 backend plan WP2 (m5-backend-approved-plan.md): Git node tree identity as a content digest instead of git:<commit>, full staged checkout for updates, post-apply validation before dependencies or hooks, GitRefused { status } -->
**In progress: Git content identity (M5 WP2).** Git node reviews are to be bound to a content digest of the whole staged tree, not just the commit, so edited, untracked or ignored files change the identity. Updates stage a full checkout of the frozen target commit, and the live fast-forward is validated against the reviewed digest before any dependency or script runs. A refused update keeps a bounded, redacted `git status`.

## Known limitations and open validation items

This list is the honest state of validation, not a roadmap.

**Not established by any test result**

- Linux: no end-to-end run, including Secret Service, freedesktop trash, WebKitGTK behaviour and Linux packaging. Windows results do not carry over.
- NVIDIA: the only real-hardware evidence is the Windows run on 2026-10-16 (RTX 3060, profile `win-py313-torch2141-cu130`: Python 3.13.16, torch 2.14.1+cu130, torchvision 0.29.1+cu130, triton-windows 3.8.0.post29, repair of a healthy environment returned "nothing to repair"). That run is not acceptance of every profile. The main development machine has an AMD GPU only.
- Windows installers (NSIS and MSI) build but have not been installed and run. They are unsigned (no Authenticode certificate) and the application uses placeholder icons.
- Accelerator wheel sources and compatibility for Python 3.13 and torch 2.14.1 on both operating systems are unvalidated (REQUIREMENTS section 9). The Python 3.12 wheels planned for M6 prove artifact identity only and are experimental. FlashAttention 3's import module name is unverified.
- Whether the native Hugging Face engine can fetch Xet-backed files, and whether a later `hf` release restores cross-run resume.
- WebKitGTK suitability for the ComfyUI canvas, and Thai font rendering on Linux.
- Updater key custody and the signed-update flow.
- The M5 custom-node hardening, M6 features, and the 17 acceptance scenarios in REQUIREMENTS section 8 are not accepted. A `docs/RELEASE_ACCEPTANCE.md` ledger is planned to map each requirement to evidence or an explicit blocker. It does not exist yet.

**Known differences and limits**

- The PyTorch 2.14.1 profiles omit `torchaudio` (no matching wheel).
- In-place dependency changes are not rolled back. A failed `uv pip` or `install.py` step marks the environment repair-required, and the application does not snapshot venvs.
- Only one ComfyUI install is managed at a time.
- `docs/IMPLEMENTATION_STATUS.md` still describes M2 to M5 as remaining work in places, while the code contains their pages and handlers. Treat the status files as acceptance notes and the source as the snapshot of features.
- No license has been selected. The application does not add one on the owner's behalf.
- AMD, Intel, macOS, CPU-only, localisation and multi-install support are out of scope for release 1.
