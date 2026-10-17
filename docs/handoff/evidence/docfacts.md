# Repository inventory for documentation

**Read-only snapshot:** `git status --short` shows in-progress edits in the runtime, custom-node, and desktop IPC/lifecycle code, plus an untracked `crates/application/src/custom_nodes/runtime.rs`. Claims below describe the current working copy; reconfirm claims involving those areas before publishing. `HANDOFF.md:3` also calls the main copy unverified partial M5 work. I did not run builds, tests, installs, formatting, or other write operations.

## 1. Product and release scope

- The project describes a native Rust/Tauri desktop app for installing, running, and maintaining a local ComfyUI, with a web UI rendered in the operating system webview. The documented targets are Windows 10/11 x64 and Linux x64, NVIDIA GPUs only; the spec says WebView2 on Windows and WebKitGTK on Linux, and says the app does not use Electron. These are **documented release targets**, not proof of acceptance on both platforms. (`docs/REQUIREMENTS.md:8,22-28`)
- Tauri identity: product name `vjumpkung-comfyui-desktop`, version `0.1.0`, identifier `dev.vjump.comfyui-desktop`. The manager window title is `vjumpkung ComfyUI`. (`crates/desktop/tauri.conf.json:3-5,14-22`)
- The bundle metadata describes the app as installing and managing a local NVIDIA ComfyUI environment. (`crates/desktop/tauri.conf.json:30-43`)

## 2. User-facing pages, navigation, and setup wizard

Navigation labels are **Overview**, **ComfyUI**, **Downloads**, **Models**, **Custom Nodes**, **Environment**, and **Settings**. The shell routes each label to its page; the top bar contains runtime controls and a theme toggle. (`ui/src/app/shell/nav.ts:27-35`; `ui/src/app/shell/app-shell.tsx:77-100`; `ui/src/app/shell/top-bar.tsx:5-10`)

| Page | What it lets the user do |
|---|---|
| **Overview** | See an app summary and application information, including version, data directory, and credential backend. (`ui/src/features/overview/overview-page.tsx:25-44`) |
| **ComfyUI** | Start, stop, or restart the server; open it in the app’s separate ComfyUI window or the system browser; view live redacted logs, clear the log view, copy diagnostics, and inspect read-only extra model paths. A port-conflict dialog can offer a suggested port. (`ui/src/features/comfy-runtime/comfy-page.tsx:181-260,265-295,304-344,350-379`) |
| **Downloads** | Add a provider/direct URL, import a pack, export a resource list, and manage the persistent queue. Queue actions include pause, resume, retry, cancel, and resolving a filename conflict. (`ui/src/features/downloads/downloads-page.tsx:28-52,75-91`; `ui/src/features/downloads/queue-table.tsx:147-210,319`) |
| **Models** | Rescan; search and filter by category/root; sort by name, category, size, or modified time; select files; inspect model details and preview; identify, rename, move, or delete. The delete dialog confirms moving to the OS trash and offers a separately confirmed permanent-delete path if trash fails. (`ui/src/features/library/library-page.tsx:145-224,226-285,322-387`; `ui/src/features/library/details-pane.tsx:43-159`; `ui/src/features/library/delete-dialog.tsx:139-224,288-353`) |
| **Custom Nodes** | Browse the Comfy Registry; install a registry release or Git repository; search/filter installed nodes; update selected nodes or all; enable, disable, or uninstall; inspect operations and recovery state. Install/update plans have review and consent dialogs. (`ui/src/features/custom-nodes/custom-nodes-page.tsx:167-205,325-380`; `ui/src/features/custom-nodes/registry-browser.tsx:150-249`; `ui/src/features/custom-nodes/install-dialog.tsx:54-136`; `ui/src/features/custom-nodes/review-dialog.tsx:69-130`; `ui/src/features/custom-nodes/confirm-dialog.tsx:46-51`) |
| **Environment** | Inspect the active environment, Python, profile, Torch/CUDA, Triton, packages, and verification; preview/apply package operations; re-run setup; and remove inactive environments. (`ui/src/features/environment/environment-page.tsx:122-234,261-359,370-473`) |
| **Settings** | Change preferences, manage CivitAI and Hugging Face tokens, test or clear saved tokens, and start an environment setup mode. (`ui/src/features/settings/settings-form.tsx:132-205`; `ui/src/features/settings/credentials-panel.tsx:68-85,165-190`; `ui/src/features/settings/settings-page.tsx:32-70`) |

**Setup wizard:**

- The six displayed stages are **Install root → Hardware → Python source → Profile → Options → Review**. First run starts at Install root; other modes start at Hardware. (`ui/src/features/setup/setup-wizard.tsx:42-49,64-66`)
- Modes are **Set up ComfyUI**, **Repair environment**, **Change torch / CUDA**, **Change Python**, and **Switch to custom venv**. (`ui/src/features/setup/model.ts:10-16`)
- Python source options are **App-managed**, **Custom venv**, and **Custom interpreter**. The wizard says an empty custom folder becomes app-owned; an existing venv is adopted and never deleted; a custom interpreter creates an app-owned venv under `envs/`. The chosen Python is validated before setup. (`ui/src/features/setup/setup-wizard.tsx:403-442,445-504`)
- Repair and Change torch/CUDA require the same Python minor as the active environment; Change Python requires a different minor. A custom interpreter/venv must match the selected profile’s Python minor. (`ui/src/features/setup/model.ts:22-64`)
- First run can select the latest ComfyUI release or enter an exact tag; tag browsing is unavailable. Re-setup retains the current ComfyUI checkout and commit. ComfyUI-Manager is optional and off by default; selecting it requires source review/consent. Cache cleaning is also optional. (`ui/src/features/setup/setup-wizard.tsx:630-671,674-700,721-769`)
- Settings also exposes direct buttons for Repair, Change torch/CUDA, Change Python, and Switch to custom venv. (`ui/src/features/settings/settings-page.tsx:32-70`)

## 3. IPC commands, events, and channels

The authoritative registered command collection is `crates/desktop/src/ipc/bindings.rs:7-89`. Handler references below give each command’s context and purpose.

**App, settings, and credentials** — handlers in `crates/desktop/src/ipc/commands.rs:16-104`; token test in `crates/desktop/src/ipc/download_commands.rs:363-375`.

- `app_info` — return app version, data directory, and credential backend.
- `settings_get` — read preferences.
- `settings_set` — validate and save preferences; apply the new download concurrency.
- `secret_set` — save a CivitAI or Hugging Face token.
- `secret_clear` — remove a saved token.
- `secret_status` — report whether a token is set and its test status.
- `secret_test` — test a saved token with its provider.

**Setup and environment** — handlers in `crates/desktop/src/ipc/env_commands.rs:51-244`.

- `setup_status` — report setup state or recovery state.
- `setup_detect_hardware` — detect hardware for a candidate root/environment.
- `setup_list_profiles` — list profiles and eligibility.
- `setup_validate_python` — validate a Python source.
- `setup_select_root` — select a new install root and save the wizard draft.
- `setup_run` — start setup and stream progress.
- `setup_cancel` — cancel a setup run.
- `env_info` — return active environment information.
- `env_packages` — list installed packages.
- `env_pip_preview` — preview a package operation.
- `env_pip_apply` — apply a package operation.
- `env_delete` — delete an inactive environment.
- `pick_folder` — open the app’s folder picker.
- `pick_file` — open the app’s file picker.

**ComfyUI runtime and app close** — handlers in `crates/desktop/src/ipc/comfy_commands.rs:44-273`.

- `comfy_status` — read runtime status.
- `comfy_start` — start ComfyUI.
- `comfy_stop` — stop ComfyUI.
- `comfy_restart` — restart ComfyUI.
- `comfy_subscribe` — subscribe to runtime state/log/close-request events.
- `comfy_unsubscribe` — release that subscription.
- `comfy_get_launch_options` — read saved launch options.
- `comfy_set_launch_options` — validate and save launch options.
- `comfy_list_versions` — list available ComfyUI versions and pending update state.
- `comfy_update` — update to the requested target, optionally stopping first or retrying.
- `comfy_open_window` — open the running local ComfyUI in its separate webview window.
- `comfy_open_browser` — open the running local ComfyUI in the system browser.
- `comfy_diagnostics` — gather diagnostics.
- `comfy_read_extra_model_paths` — read configured extra model paths.
- `app_close_decide` — apply the user’s close-dialog choice.

**Downloads** — handlers in `crates/desktop/src/ipc/download_commands.rs:51-357`.

- `dl_resolve` — resolve a provider/direct URL to downloadable file choices.
- `dl_enqueue` — queue selected resolved files.
- `dl_pause` — pause a job.
- `dl_resume` — resume a paused job or resolve a pending conflict.
- `dl_cancel` — cancel a job.
- `dl_retry` — retry a failed job.
- `dl_list` — list jobs.
- `dl_subscribe` — subscribe to queue state/progress events.
- `dl_unsubscribe` — release that subscription.
- `dl_import_pack` — parse and resolve an imported model pack.
- `dl_export` — export selected or completed jobs as a resource list.

**Model library** — handlers in `crates/desktop/src/ipc/library_commands.rs:20-212`.

- `model_list` — list catalogued models and library status.
- `model_rescan` — recover interrupted operations, then rescan roots.
- `model_details` — read model details/sidecars/preview.
- `model_rename` — rename a model.
- `model_move` — move a model to another folder/root.
- `model_delete_plan` — create a delete confirmation plan.
- `model_delete` — move planned files to OS trash.
- `model_delete_permanently` — permanently delete a confirmed plan.
- `model_identify` — identify a model on CivitAI by hash.
- `model_operation_status` — read rename/move/delete operation status.
- `model_subscribe` — subscribe to library changes.
- `model_unsubscribe` — release that subscription.

**Custom nodes** — handlers in `crates/desktop/src/ipc/node_commands.rs:116-435`.

- `node_open_repository` — open a validated repository URL in the system browser.
- `node_registry_search` — search/browse the Comfy Registry.
- `node_list` — list installed nodes, pending reviews, recent/unresolved operations, and repair status.
- `node_git_status` — inspect a Git-installed node’s repository state.
- `node_rescan` — recover interrupted operations and rescan node folders.
- `node_prepare_registry` — prepare a registry install for review.
- `node_prepare_git` — prepare a Git install for review.
- `node_prepare_update` — prepare an update for review.
- `node_prepare_retry_deps` — prepare a dependency retry for review.
- `node_commit` — apply a reviewed plan with consent.
- `node_set_enabled` — enable or disable a node.
- `node_uninstall` — uninstall a node.
- `node_update_all` — apply reviewed updates sequentially as a batch.
- `node_cancel` — cancel an operation or discard a prepared plan.
- `node_operation_status` — read a node operation’s status.
- `node_subscribe` — subscribe to node snapshot/change/log/gap events.
- `node_unsubscribe` — release that subscription.

**Channels/events:** Setup uses the `setup_run` channel for setup events and a final result; closing the UI channel does not stop setup. (`env_commands.rs:29-45,133-158`; `env_dto.rs:851-927`) Runtime channel events are Snapshot, State, Log, Gap, and CloseRequested. (`comfy_commands.rs:73-115`; `comfy_dto.rs:327-368`) Download events are Snapshot, State, Progress, and Gap. (`download_commands.rs:255-305`; `download_dto.rs:498-541`) Model events are Snapshot and Changed. (`library_commands.rs:170-205`; `library_dto.rs:238-247`) Node events are Snapshot, Changed, Log, and Gap. A gap means the UI should refetch. (`node_commands.rs:370-425`; `node_dto.rs:453-466`)

## 4. Settings, defaults, and secrets

- Domain settings and defaults: `download_concurrency=3`; `hf_engine=hf_cli`; `close_behavior=ask`; `theme=system`; `registry_cache_ttl=6` hours; `bandwidth_cap_kib_s=None` (unlimited). Concurrency and TTL must be positive; a configured bandwidth cap must be positive. (`crates/domain/src/settings/mod.rs:21-19,21-60,63-88,91-150,159-187,195-203`)
- Persisted SQLite keys are `download_concurrency`, `hf_engine`, `close_behavior`, `theme`, `registry_cache_ttl`, and `bandwidth_cap_kib_s`; unlimited is stored as `unlimited`. (`crates/infrastructure/src/settings/sqlite.rs:25-60,90-106`)
- The scheduler clamps concurrency to 1–32. Lowering the limit affects new admissions; already running transfers continue. (`crates/infrastructure/src/models/acquisition/supervisor.rs:39-60`; `crates/desktop/src/ipc/commands.rs:51-58`)
- Hugging Face engine choices are `hf_cli` and `native`; hf CLI is the default. Non-HF sources use the native engine. (`crates/domain/src/settings/mod.rs:63-88`; `crates/application/src/models/acquisition/use_cases.rs:93-105`)
- Close choices are ask, stop ComfyUI and quit, or keep ComfyUI running headlessly. Theme choices are system, light, and dark. (`crates/domain/src/settings/mod.rs:91-150`; `ui/src/features/settings/settings-form.tsx:19-34`)
- The bandwidth-cap key exists, but the UI field is disabled and says downloads currently run unthrottled. (`ui/src/features/settings/settings-form.tsx:185-197`; `crates/infrastructure/src/settings/sqlite.rs:94-100`)
- CivitAI and Hugging Face credentials use the OS keyring where available, under service `vjumpkung-comfyui` with separate provider accounts. The UI says tokens are never written to the database or logs; without keyring access they remain in memory for that session only. (`crates/infrastructure/src/settings/secrets.rs:101-110,123-163`; `ui/src/features/settings/credentials-panel.tsx:165-184`)

## 5. On-disk layout

- The code chooses `dirs::data_local_dir()/vjumpkung-comfyui` as the default install root. The requirements doc spells this out as `%LOCALAPPDATA%\vjumpkung-comfyui` on Windows and `~/.local/share/vjumpkung-comfyui` on Linux. (`crates/infrastructure/src/shared/paths.rs:19-31`; `docs/REQUIREMENTS.md:158-170`)
- The active install root contains `ComfyUI/`, `envs/`, `tools/uv/`, `tools/git/` (Windows MinGit), `tools/hf/`, `data/`, and `install.json`. (`crates/infrastructure/src/environment/layout.rs:23-78,63-85`)
- `data/` contains `app.db`, `cache/`, `downloads/`, and `logs/`. `AppPaths` maps these to `data/app.db` and `data/logs`. Cache directories actually ensured by the code are `uv`, `uv-python`, `pip`, `hf`, `torch`, and `triton`. (`crates/infrastructure/src/shared/paths.rs:33-49`; `crates/infrastructure/src/environment/layout.rs:47-64,193-225`)
- `tools/hf/` is an isolated Python tool environment; HF downloads also use `data/tmp/hf`. (`crates/infrastructure/src/environment/layout.rs:39-41,76-78`; `crates/infrastructure/src/models/acquisition/hf_engine.rs:41-68`)
- The bootstrap pointer is `<OS config dir>/vjumpkung-comfyui/bootstrap.json`. Root resolution order is debug override, bootstrap pointer, then default root. `VJUMPKUNG_COMFYUI_HOME` is honored only in debug builds. (`crates/infrastructure/src/shared/paths.rs:79-84,132-163`)
- **Inferred from the current code:** there is no separate `data/cache/registry` directory in the cache directory creation list; the registry client uses its SQLite cache. (`layout.rs:219-225`; `crates/infrastructure/src/custom_nodes/registry.rs:218-237,421-469`)

## 6. Setup profiles and pinned tools

Seven profile JSON files are bundled via the profile catalog. Eligibility checks OS, NVIDIA GPU presence, every detected GPU’s compute capability and wheel architecture support, driver CUDA level, and available disk budget. (`crates/infrastructure/src/environment/profiles.rs:215-223`; `crates/domain/src/environment/eligibility.rs:71-145`)

| Profile | Python / Torch | CUDA / Triton | Status |
|---|---|---|---|
| `win-py313-torch2141-cu130` | 3.13.16 / 2.14.1; torchvision 0.29.1; torchaudio unset | cu130; `triton-windows >=3.8,<3.9` | Stable, recommended. (`profiles/win-py313-torch2141-cu130.json:3-36`) |
| `win-py313-torch2141-cu126` | 3.13.16 / 2.14.1; torchvision 0.29.1; torchaudio unset | cu126; `triton-windows >=3.8,<3.9` | Stable. (`profiles/win-py313-torch2141-cu126.json:3-38`) |
| `win-py314-torch2141-cu130` | 3.14.8 / 2.14.1; torchvision 0.29.1; torchaudio unset | cu130; `triton-windows >=3.8,<3.9` | Experimental. (`profiles/win-py314-torch2141-cu130.json:3-36`) |
| `win-py312-torch2100-cu130` | 3.12.15 / 2.10.0; torchvision 0.25.0; torchaudio 2.10.0 | cu130; `triton-windows >=3.6,<3.7` | Stable compatibility profile. (`profiles/win-py312-torch2100-cu130.json:3-36`) |
| `linux-py313-torch2141-cu130` | 3.13.16 / 2.14.1; torchvision 0.29.1; torchaudio unset | cu130; `triton==3.8.0` | Stable, recommended. (`profiles/linux-py313-torch2141-cu130.json:3-36`) |
| `linux-py313-torch2141-cu126` | 3.13.16 / 2.14.1; torchvision 0.29.1; torchaudio unset | cu126; `triton==3.8.0` | Stable. (`profiles/linux-py313-torch2141-cu126.json:3-37`) |
| `linux-py314-torch2141-cu130` | 3.14.8 / 2.14.1; torchvision 0.29.1; torchaudio unset | cu130; `triton==3.8.0` | Experimental. (`profiles/linux-py314-torch2141-cu130.json:3-36`) |

CUDA 12.6 profiles specify minimum driver CUDA 12.6; CUDA 13.0 profiles specify 13.0. All tabled profiles specify `min_cc=8.0`. (`profiles/win-py313-torch2141-cu126.json:17-18`; `profiles/linux-py313-torch2141-cu130.json:17-18`)

Pinned tools and hash locations:

- uv `0.13.1`; Windows/Linux archive URLs and SHA-256 values are in `crates/infrastructure/src/environment/pins.rs:14-40`.
- Windows MinGit `2.55.0.3` / Git `2.55.0.windows.3`, URL and SHA-256 in `pins.rs:43-46`; Linux requires system Git ≥2.40. (`pins.rs:48-49`)
- HF CLI `2.2.0` with separately managed Python `3.13.16`; Windows/Linux package locks include hashes for `hf`, `hf-xet`, and dependencies. (`pins.rs:51-64`; `data/hf-requirements-windows.txt:1-8`; `data/hf-requirements-linux.txt:1-8`)
- Managed CPython download hashes are in `data/python-downloads.json`, which `pins.rs` embeds. (`pins.rs:9-12,51`; `python-downloads.json:1-18`)
- Windows app-local MSVC runtime pin: `msvc-runtime==14.44.35112`. (`pins.rs:68-69`)

## 7. Download providers, engines, and model packs

- **CivitAI:** accepts `civitai.com`, `civitai.green`, and `civitai.red` model pages (`/models/{id}` with optional `modelVersionId`), `/api/model-versions/{version}`, `/api/download/models/{version}`, and `/api/v1/model-versions/{version}`; query filters include `fileId`, `type`, `format`, `size`, and `fp`. (`crates/infrastructure/src/models/acquisition/sources.rs:78-121`)
- **Hugging Face:** accepts `huggingface.co` model, dataset, and space repo URLs, plus `resolve`, `blob`, or `tree` URL forms with revision/path; a repo URL resolves to a file listing. The transfer is pinned to a resolved commit. (`sources.rs:123-171`; `huggingface.rs:20-42,87-140`)
- **Google Drive:** accepts `/file/d/{id}` and `?id={id}` forms on `drive.google.com` or `drive.usercontent.google.com`; folders are rejected. (`sources.rs:173-193`)
- **Direct links:** other supported sources are treated as direct URLs. HTTPS is accepted; plain HTTP requires explicit confirmation. Direct links may use a `Content-Disposition` filename, otherwise URL filename. (`sources.rs:195-218`; `http.rs:122-131`; `direct.rs:15-84`)
- The settings-selected engine applies to Hugging Face. `hf_cli` is the default; native HTTP is the alternative. If the HF CLI cannot launch, the supervisor falls back to native; other failures are not the launch-failure fallback condition. Non-HF downloads use native. (`crates/domain/src/settings/mod.rs:63-88`; `crates/application/src/models/acquisition/use_cases.rs:93-105`; `crates/infrastructure/src/models/acquisition/supervisor.rs:168-185`)
- Native resume uses Range/If-Range with a **strong ETag**; without one, the partial is cleared and the transfer starts at zero. HF CLI pause stops the process and resume starts from zero; the queue UI labels that behavior. (`native_engine.rs:23-30,68-80,103-115`; `supervisor.rs:239-260,333-346`; `ui/src/features/downloads/queue-table.tsx:140-185`)
- Model packs import JSON resource lists containing `{name,url,type}` (also accepts `model_type`), or an index whose entries point to HTTPS resource lists. Remote JSON is bounded to 8 MiB. Export produces a `resource_list` JSON array; links requiring an ephemeral signed query are excluded. (`crates/infrastructure/src/models/acquisition/pack.rs:21-40,42-79,83-123,124-160`; `ui/src/features/downloads/import-pack-dialog.tsx:110-162,171-224`; `export-dialog.tsx:58-99`)
- The UI previews pack contents, lets users choose provider file variants and deselect items; export can copy or save JSON. (`import-pack-dialog.tsx:171-224`; `export-dialog.tsx:71-99`)

## 8. Model library

- Scans the default `ComfyUI/models` root and roots discovered from `ComfyUI/extra_model_paths.yaml`; unavailable roots are reported. The scanner does not follow links and does not hash files during ordinary scans. (`crates/infrastructure/src/models/library/roots.rs:80-97,120-215`; `scan.rs:1-3,89-124`)
- The UI supports search, category/root filters, sorting, rescan, and file selection. Watcher degradation is surfaced; polling checks every 30 seconds, and users can rescan. (`ui/src/features/library/library-page.tsx:167-199,226-285`)
- Rename is within the same folder; extension changes require acknowledgement. Move can target another folder/root. (`ui/src/features/library/rename-dialog.tsx:31-32,61-92`; `move-dialog.tsx:29-30,61-63`)
- Delete first plans and reports per-file results, then moves files to OS trash. Permanent deletion is offered only after trash fails and needs separate confirmation. (`crates/desktop/src/ipc/library_commands.rs:88-134`; `crates/infrastructure/src/models/library/file_ops.rs:191-220`; `ui/src/features/library/delete-dialog.tsx:139-224,288-353`)
- **Identify on CivitAI** computes SHA-256, queries by hash, and can show CivitAI details and a preview. (`ui/src/features/library/details-pane.tsx:89-108,110-159`; `crates/infrastructure/src/models/library/identify.rs:1-3,161-198`)
- `extra_model_paths.yaml` is read-only in the UI. The parser expands `~`, `$VAR`, `${VAR}`, and Windows `%VAR%`, resolves relative paths against the ComfyUI directory, and reads configured category paths. (`ui/src/features/comfy-runtime/comfy-page.tsx:304-344`; `crates/infrastructure/src/comfy_runtime/extra_paths.rs:11-35,41-105,106-169`)

## 9. Custom nodes

- The Registry browser searches/paginates, refreshes, shows compatibility and stale-cache status, and offers registry installs. Cache lifetime comes from the settings TTL. (`ui/src/features/custom-nodes/registry-browser.tsx:150-249`; `crates/desktop/src/ipc/node_commands.rs:125-151`; `crates/infrastructure/src/custom_nodes/registry.rs:32-36,410-469`)
- Registry installs can select an optional version. Git installs accept an HTTPS repository and optional branch/tag/commit reference. The app prepares an exact plan first; install/update does not execute until review and consent. (`ui/src/features/custom-nodes/install-dialog.tsx:54-61,91-135`; `crates/infrastructure/src/custom_nodes/provenance.rs:26-59`)
- Git sources are resolved to immutable commits, cloned to staging, and updates are fast-forward-only; the Git adapter says it does not reset, clean, or discard edits. (`crates/infrastructure/src/custom_nodes/git.rs:1-2,186-195,290-333`)
- Review shows compatibility/source warnings and protected-package changes. Consent is per reviewed plan; the user can separately consent to third-party code and the root `install.py`. The UI explicitly warns that a venv is not a sandbox and dependency changes cannot be rolled back. (`ui/src/features/custom-nodes/review-dialog.tsx:69-93,107-130`; `crates/infrastructure/src/custom_nodes/hook.rs:1-2,32-55`)
- Enable/disable and uninstall are direct confirmed operations. Update selected/all prepares reviewed plans; batch execution stops if a dependency failure damages the environment. (`crates/application/src/custom_nodes/use_cases.rs:146-182,206-230`; `crates/desktop/src/ipc/node_commands.rs:263-335`)
- Recovery UI exposes **Rescan / finish recovery**, unresolved operations that block startup, and dependency retry plans. Failed dependency mutation can require environment repair; interrupted retained-backup cleanup is retried on rescan. (`ui/src/features/custom-nodes/operations-panel.tsx:131-182`; `crates/application/src/custom_nodes/use_cases.rs:95-115`; `ui/src/features/custom-nodes/custom-nodes-page.tsx:205-285`)

## 10. ComfyUI runtime

- The runtime supports start, stop, restart, status, live logs, and session diagnostics. Start checks node state, process ownership/recovery, pending updates, selected environment, port availability, and launch options. (`crates/application/src/comfy_runtime/use_cases.rs:59-132,134-177,179-195`; `ui/src/features/comfy-runtime/comfy-page.tsx:181-260,265-295`)
- Saved launch options include port (default 8188), VRAM mode, attention backend, preview method, built-in Manager, LAN listen, and extra arguments. Extra arguments are tokens, not a shell command; reserved options such as port, listen address, and model paths are rejected. Options apply on the next start. (`crates/domain/src/comfy_runtime/launch.rs:17-30,36-83,138-160`; `ui/src/features/comfy-runtime/launch-options.tsx:33-54,163-202,204-275`)
- LAN listen is opt-in and requires acknowledgement; the UI warns ComfyUI has no login and network users may run workflows/read served files. (`ui/src/features/comfy-runtime/launch-options.tsx:216-251`)
- The ComfyUI webview is a separate top-level window for `http://127.0.0.1`; same-origin navigation stays there, external HTTP(S) opens in the system browser, and unsafe URLs are denied. (`crates/desktop/src/comfy_window.rs:19-33,47-100`)
- The version picker offers latest release, fast-forward of the current branch, and available tags. Updates may stop a running server, reinstall requirements under protected pins, and verify the environment; the UI warns switching to an older tag does not restore the previous package set. (`ui/src/features/comfy-runtime/versions-panel.tsx:165-208,219-260`)
- Close settings are Ask, Stop ComfyUI and quit, or keep it running headlessly. The Ask dialog offers Cancel, Keep running in background, or Stop ComfyUI and exit. Headless mode destroys visible windows; a later app launch restores the manager controls. (`ui/src/features/comfy-runtime/close-request-dialog.tsx:34-64`; `crates/desktop/src/app_lifecycle.rs:137-195,198-226`)

## 11. Security model highlights

- The Tauri manager window is the configured capability target. The capability enumerates `allow-*` permissions; the CSP restricts scripts and connections to self/Tauri IPC (with Vite endpoints in dev config). (`crates/desktop/tauri.conf.json:12-27`; `crates/desktop/capabilities/manager.json:3-85`)
- The ComfyUI window has no capability. ACL integration tests verify every registered command is represented in the app manifest, generated bindings, and manager capability, allow manager-local invocation, and deny ComfyUI-window and remote-origin invocation. (`crates/desktop/src/comfy_window.rs:6-8`; `crates/desktop/tests/acl.rs:419-464,466-488,624-667`)
- Saved credentials are attached only to the exact HTTPS provider origin (`civitai.com` or `huggingface.co`) and stripped on cross-origin redirects. (`crates/infrastructure/src/models/acquisition/http.rs:35-96,113-131`)
- Token-like query keys are rejected. Signed direct URLs with query strings are staged only in process memory; their query is removed from the persisted locator and the original is registered with the log redactor. (`crates/infrastructure/src/models/acquisition/sources.rs:35-45,198-218`)
- Logs redact registered secrets, authorization/bearer strings, token/API-key query values, AWS `x-amz-*` values, HF token patterns, and URL userinfo. (`crates/infrastructure/src/shared/logging.rs:20-32,34-115,173-219`)
- The common archive extractor rejects traversal, absolute/UNC paths, symlinks/reparse points and special files; it checks case-insensitive collisions and applies entry-count, expansion-size, and compression-ratio limits. Output stays within a fresh staging directory. (`crates/infrastructure/src/shared/archive.rs:23-30,70-116,125-149,188-224,271-285,305-359,384-421`)
- Model file operations use authorized roots and no-follow/reparse-point checks during traversal/open. (`crates/infrastructure/src/shared/model_paths.rs:16-55,88-109,111-190,216-`)
- General subprocess environments are allow-listed; the HF CLI adapter explicitly adds `HF_TOKEN` to that child environment. (`crates/infrastructure/src/environment/layout.rs:136-164,193-215`; `crates/infrastructure/src/models/acquisition/hf_engine.rs:49-84`)

## 12. Developer workflow and packaging

- Rust toolchain pin: `1.99.0`, with rustfmt and clippy. (`rust-toolchain.toml:1-3`)
- pnpm pin: `pnpm@10.24.0`. UI scripts are `dev`, `build` (`tsc -b && vite build`), `lint`, `typecheck`, `test` (Vitest), and `tauri`. (`ui/package.json:6-13`)
- No active Node version pin was found in the inspected project files. **Inferred:** Node 24 is the workflow’s intended version, but those workflow lines are commented out. (`.github/workflows/ci.yml:55-60`; `.github/workflows/release.yml:38-42`)
- The Tauri wrapper resolves the CLI installed under `ui/node_modules` and sets `TAURI_APP_PATH` and `TAURI_FRONTEND_PATH`. (`scripts/tauri.mjs:2-4,10-29`)
- Documented commands include `pnpm -C ui install --frozen-lockfile`, `pnpm -C ui dev`, `pnpm -C ui tauri dev`, `pnpm -C ui build`, `pnpm -C ui tauri build`, `pnpm -C ui lint`, `pnpm -C ui typecheck`, `pnpm -C ui test`, `cargo test --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `node scripts/check-arch-deps.mjs`. (`AGENTS.md:18-28`)
- The architecture check runs locked `cargo metadata` and checks both resolved and declared dependency edges against allowed crate-layer dependencies. (`scripts/check-arch-deps.mjs:5-15,44-60,63-88`)
- Bindings come from the Rust `bindings.rs` builder. The normal integration test exports to a temporary file and compares with `ui/src/shared/ipc/bindings.ts`; the ignored `export_bindings` test writes the checked-in file. Regeneration command: `cargo test -p desktop --test bindings -- --ignored export_bindings`. (`crates/desktop/tests/bindings.rs:11-20,27-50`)
- ACL tests use a test app and real compiled manifest/capability; they test command coverage and origin/window permissions. (`crates/desktop/tests/acl.rs:1-4,419-488,624-667`)
- Ignored real setup test requires an empty/nonexistent `VJ_REAL_ROOT`; optional `VJ_REAL_PROFILE` selects a profile. It downloads several GB and requires an NVIDIA GPU. An ignored interpreter test requires `VJ_TEST_PYTHON` pointing to CPython 3.12–3.14. (`crates/infrastructure/tests/real_setup.rs:1-8,102-129`; `crates/infrastructure/src/environment/python/tests.rs:376-380`)
- Packaging config: general bundle target `all`; Windows NSIS/MSI, silent WebView2 bootstrapper, NSIS current-user install; Linux AppImage/deb with WebKitGTK/GTK dependencies. A separate updater config enables updater artifacts. (`crates/desktop/tauri.conf.json:30-43`; `tauri.windows.conf.json:1-8`; `tauri.linux.conf.json:1-8`; `tauri.updater.conf.json:1-3`)
- Both CI and release workflow files are commented out, so those workflows are not active. The release workflow shows updater build variables `VJUMP_UPDATER_PUBKEY`, `VJUMP_UPDATER_ENDPOINT`, `TAURI_SIGNING_PRIVATE_KEY`, and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`; it conditionally passes updater config when signing is configured. (`.github/workflows/ci.yml:1-101`; `.github/workflows/release.yml:1-90`)
- Implementation status says updater signing is inactive pending public key/endpoint, placeholder icons remain, and no license is selected. (`docs/IMPLEMENTATION_STATUS.md:48-52`)

## 13. Documented limitations and status conflicts

- `IMPLEMENTATION_STATUS.md` says PyTorch 2.14.1 profiles omit torchaudio because no matching artifact was available; the Windows Python 3.12 compatibility profile includes torchaudio 2.10.0. It also says Linux GPU, Secret Service, trash, and WebKitGTK acceptance are not established by Windows tests. (`docs/IMPLEMENTATION_STATUS.md:37-46`)
- Requirements §9 lists accelerator wheel sources/hashes still to validate for Python 3.13/Torch 2.14.1 on both OSes; native HF Xet support and later HF cross-run resume remain open; WebKitGTK canvas suitability, Thai fonts, Windows code signing, and updater key custody remain open. (`docs/REQUIREMENTS.md:564-570`)
- `HANDOFF.md` says README is absent, the copy is unverified partial M5, and Linux/NVIDIA acceptance and Windows installer execution remain incomplete. (`HANDOFF.md:3,51-55`)
- **Inferred status conflict:** `IMPLEMENTATION_STATUS.md:10-17` still describes M2–M5 implementation/acceptance as remaining, while the current tree contains runtime, downloads, library, and custom-node pages/handlers. The current working tree is dirty in those same areas, so use the status docs as acceptance notes and the source as a snapshot of current features, not as evidence of release readiness.
- `IMPLEMENTATION_STATUS.md:21-28` records a Windows NVIDIA real-setup and repair pass dated 2026-10-16; it does not establish Linux acceptance. I did not independently verify that run.

## Uncertain / could not verify

- I did not run the application or any tests, builds, or packaging commands; runtime behavior and platform acceptance remain unverified in this read-only review.
- The current dirty runtime/custom-node/IPC files may change before the working copy is finalized.
- Node 24 is only present in commented workflows; no active Node version constraint was found in the inspected files.