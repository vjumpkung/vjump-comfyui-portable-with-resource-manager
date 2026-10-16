# M6 environment-side audit

Read-only inspection only; no files changed, and no `cargo` or `pnpm` commands were run.

## 1. Profiles and accelerator evidence

The handoff identifies these as M6 gaps: all seven profiles have empty accelerator lists, profile tests explicitly defer them, setup forwards each requirement without using its `sha256`, and queries currently report no accelerators. Curated nodes are always skipped. [HANDOFF.md:122-128]

The parser’s schema is the serde `ProfileDoc` in `crates/infrastructure/src/environment/profiles.rs:13-78`. Its nested types include `AcceleratorDoc { name, requirement, sha256: Option<String>, non_fatal }` and `VerifyDoc { torch_version_prefix, require_cuda, require_triton }`. `parse_profile(json: &str) -> Result<SetupProfile, ProfileError>` deserializes that document and converts it to the domain type at `crates/infrastructure/src/environment/profiles.rs:89-150,210-212`.

The domain types are in `crates/domain/src/environment/profile.rs:74-88,114-139`: `SetupProfile` has `accelerators: Vec<AcceleratorSpec>`, `constraints: Vec<String>`, and `verify: VerifyExpectations`. Validation requires any accelerator to have a nonempty name and a 64-character SHA-256, but does not check that the hash is hexadecimal or provide dedicated URL, filename, module, no-dependencies, or per-artifact provenance fields (`crates/domain/src/environment/profile.rs:214-222`).

All seven bundled JSON files have `"accelerators": []` (`crates/infrastructure/src/environment/data/profiles/*.json`). The profile test `every_bundled_profile_parses_validates_and_has_a_unique_id` asserts `profile.accelerators.is_empty(), "accelerators are M6"` (`crates/infrastructure/src/environment/profiles/tests.rs:15-32`). The application also has `accelerators_are_best_effort_when_a_profile_defines_them`, which checks a run succeeds with an accelerator present (`crates/application/src/environment/tests/setup.rs:486-497`).

Representative bundled JSON, copied verbatim from `crates/infrastructure/src/environment/data/profiles/win-py313-torch2141-cu130.json:1-55`:

```json
{
  "schema_version": 1,
  "id": "win-py313-torch2141-cu130",
  "revision": 1,
  "label": "Python 3.13, PyTorch 2.14.1, CUDA 13.0",
  "status": "stable",
  "recommended": true,
  "os": "windows",
  "python": "3.13.16",
  "torch": {
    "torch": "2.14.1",
    "torchvision": "0.29.1",
    "torchaudio": null,
    "cuda": "cu130",
    "index_url": "https://download.pytorch.org/whl/cu130"
  },
  "min_driver_cuda": "13.0",
  "min_cc": "8.0",
  "torch_archs": [
    "7.5",
    "8.0",
    "8.6",
    "9.0",
    "10.0",
    "12.0"
  ],
  "triton": {
    "package": "triton-windows",
    "specifier": ">=3.8,<3.9"
  },
  "accelerators": [],
  "constraints": [],
  "verify": {
    "torch_version_prefix": "2.14.1+cu130",
    "require_cuda": true,
    "require_triton": true
  },
  "disk": {
    "env_bytes": 5368709120,
    "other_bytes": 7516192768,
    "margin_bytes": 4294967296,
    "calibrated": true
  },
  "provenance": [
    "download.pytorch.org/whl/cu130: torch 2.14.1, torchvision 0.29.1 cp313 wheels for win_amd64 and manylinux_2_28_x86_64 exist, and a uv pip compile of the pinned set resolved from that index alone with --only-binary (observed 2026-10-16)",
    "Disk budget measured on a real install on 2026-10-16: venv 4.3 GiB, uv cache 4.4 GiB (hardlinked to the venv on one volume), tools 0.14 GiB, ComfyUI 0.14 GiB, plus the 1.9 GiB torch wheel in flight",
    "No torchaudio 2.14.x wheel is published on download.pytorch.org (latest cu130 is 2.11.0, latest cu126 is 2.9.1) or PyPI (2.11.0), and each torchaudio pins its exact torch, so this profile installs torch and torchvision only. ComfyUI v0.40.0 requirements.txt does not list torchaudio (observed 2026-10-16).",
    "PyTorch v2.14.1 .ci/pytorch/windows/build_env_setup.py TORCH_CUDA_ARCH_LIST_TABLE (observed 2026-10-16)",
    "Triton 3.6 and 3.8 README: NVIDIA GPUs with compute capability 8.0 or newer; triton-windows dropped Turing after Triton 3.2 (woct0rdho/triton-windows README, section GPU)",
    "triton-windows 3.8.0.post29 (PyPI, observed 2026-10-16) matches PyTorch 2.14.1 .ci/docker/triton_version.txt 3.8.0",
    "ComfyUI v0.40.0 requirements.txt resolves with --only-binary on Python 3.13 for this platform with torch pinned (uv pip compile, observed 2026-10-16)",
    "Python 3.13.16: uv 0.13.1 embedded python-build-standalone 20261013 metadata (SHA-256 pinned in python-downloads.json)",
    "Driver floor: CUDA 13.0 wheels need a driver that supports CUDA 13.0 (profile minimum, spec 2.4)"
  ]
}
```

The profile requirement is an optional accelerator list with a pinned SHA-256 and non-fatal flag; NFR-SEC-7 requires pinned hashes for direct wheel URLs and HTTPS downloads (`docs/REQUIREMENTS.md:445-459,477`). The current profile-level `provenance` does not record provenance per accelerator artifact.

## 2. Setup execution, package installs, and UI selections

The ordered `StepId::ALL` list is `Preflight, Tools, PythonVenv, Seed, CloneComfyUi, InstallTorch, VerifyTorch, Triton, Accelerators, ComfyRequirements, Manager, CuratedNodes, FinalVerify, CacheClean`. `StepPolicy` is `Optional` for `Manager` and `CuratedNodes`, `BestEffort` for `Accelerators` and `CacheClean`, and `Required` otherwise (`crates/application/src/environment/model.rs:39-137`). `Execution::drive` loops over that order and treats best-effort errors as warnings (`crates/application/src/environment/execute.rs:194-225`).

The dispatcher calls `self.accelerators()` but returns a hard-coded skipped result for curated nodes: `"Curated custom nodes arrive in a later milestone."` (`crates/application/src/environment/execute.rs:281-299`). The accelerator implementation skips empty profiles; otherwise it passes each `accelerator.requirement` as an `InstallSpec` requirement, with the constraints file and `allow_source_builds: false`. It never reads `accelerator.sha256` or `non_fatal`; install errors become warnings (`crates/application/src/environment/execute.rs:690-724`).

`InstallSpec` has `requirements`, `requirement_files`, `index_url`, `constraint_file`, `upgrade`, and `allow_source_builds`; it has no hash or `no_deps` field. `PackageInstaller::install` is:

```rust
async fn install(
    &self,
    root: &Path,
    env_path: &Path,
    spec: &InstallSpec,
    ctx: &StepContext,
) -> Result<(), ProvisionError>;
```

Evidence: `crates/application/src/environment/ports.rs:315-339`. `UvPackages::install_args` appends requirements as positional arguments after the `uv pip install` flags, and its install method invokes uv with those arguments (`crates/infrastructure/src/environment/packages.rs:43-68,131-149`). Direct wheel URLs could be passed as requirement strings, but this path does not hash-check them.

The pin guard records installed protected package versions, writes exact `name==version` constraints plus profile extras, and rechecks for drift after successful installs (`crates/application/src/environment/execute.rs:561-605`; `crates/infrastructure/src/environment/packages.rs:115-129`). Protected packages are torch, torchvision, optional torchaudio, and the profile’s selected Triton distribution (`crates/domain/src/environment/profile.rs:48-54,141-147`). The accelerator loop reasserts pins after a successful install, but records a failed install as a warning (`crates/application/src/environment/execute.rs:709-722`).

Final verification imports `sys, torch, triton, comfy`; when CUDA is available it runs a tensor operation. It checks import-spec presence for only `sageattention`, `flash_attn`, and `llama_cpp`; those results are logged by the setup execution. It does not report `onnxruntime` or FlashAttention 3 as modules (`crates/infrastructure/src/environment/verify.rs:30-54,180-213`; `crates/application/src/environment/execute.rs:817-845`). The persisted verification summary contains Python, torch/CUDA, and Triton, not the accelerator result list (`crates/application/src/environment/ports.rs:377-403`).

`SetupRequest` has `install_manager: bool` and `clean_cache: bool`, but no accelerator or curated-node selections (`crates/application/src/environment/model.rs:316-330`). SQLite run JSON persists those Manager/cache booleans and the profile (`crates/infrastructure/src/environment/sqlite.rs:314-385,417-435`). `install.json` records profile ID/revision and ComfyUI reference/commit, but no selected-component fields (`crates/application/src/environment/model.rs:460-477`; `crates/application/src/environment/activation.rs:26-45`).

In the wizard, Manager is a checkbox defaulting off, appears in the review summary, and is submitted in the setup request. The wizard’s current alert says accelerators and curated packs are deferred (`ui/src/features/setup/setup-wizard.tsx:88-91,677-693,750-758,832-846,706-712`). The typed wizard draft has Manager and cache fields only (`ui/src/features/setup/model.ts:81-88`).

The supplied reference BAT is the older Python 3.12 / PyTorch 2.10 stack (filename and torch install at `install_comfyui_3_12_uv_nvidia_torch_2100_portable.bat:182-189`). It lists an ORT CUDA 13 nightly index with `--no-deps`, direct llama-cpp and SageAttention wheels, FlashAttention 3 via a find-links page, and a FlashAttention 2 wheel filename/URL (`:215-242`). The FlashAttention 2 helper downloads and moves the file, then installs it without invoking `:verify_file_hash` (`:231-236,575-600`). This BAT therefore supplies reference URLs, not verified SHA-256 values for those wheels.

The reference’s 22 ordered curated-node URLs are at `install_comfyui_3_12_uv_nvidia_torch_2100_portable.bat:260-281`. Its helper uses `git clone` on the URL without selecting a commit, then may run `install.py` and install `requirements.txt` (`:533-569`). The exact URLs are:

```text
01 ComfyUI-MultiGPU — https://github.com/pollockjj/ComfyUI-MultiGPU.git
02 ComfyUI-GGUF — https://github.com/molbal/ComfyUI-GGUF.git
03 ComfyUI-KJNodes — https://github.com/kijai/ComfyUI-KJNodes.git
04 ComfyUI-H3-Motion-Context-MultiRef — https://github.com/seitanism/ComfyUI-H3-Motion-Context-MultiRef.git
05 ComfyUI-VideoHelperSuite — https://github.com/Kosinkadink/ComfyUI-VideoHelperSuite.git
06 rgthree-comfy — https://github.com/rgthree/rgthree-comfy.git
07 ComfyUI-Crystools — https://github.com/crystian/ComfyUI-Crystools.git
08 ComfyUI-WanVideoWrapper — https://github.com/kijai/ComfyUI-WanVideoWrapper.git
09 ComfyUI-Custom-Scripts — https://github.com/pythongosssss/ComfyUI-Custom-Scripts.git
10 was-node-suite-comfyui — https://github.com/WASasquatch/was-node-suite-comfyui.git
11 ComfyUI-QwenVL — https://github.com/1038lab/ComfyUI-QwenVL.git
12 ComfyUI-MelBandRoFormer — https://github.com/kijai/ComfyUI-MelBandRoFormer.git
13 RES4LYF — https://github.com/ClownsharkBatwing/RES4LYF.git
14 ComfyUI-SolAttn_triton — https://github.com/kijai/ComfyUI-SolAttn_triton.git
15 comfyui-infinitetalk-native-sampler — https://github.com/vjumpkung/comfyui-infinitetalk-native-sampler.git
16 comfyui-mosaic-blur — https://github.com/vjumpkung/comfyui-mosaic-blur.git
17 ComfyUI-WanAnimatePreprocess — https://github.com/kijai/ComfyUI-WanAnimatePreprocess.git
18 comfyui-wan-animate-2-loop-sampler — https://github.com/vjumpkung/comfyui-wan-animate-2-loop-sampler.git
19 comfyui-scail-2-loop-sampler — https://github.com/vjumpkung/comfyui-scail-2-loop-sampler.git
20 ComfyUI-PlagueKind-Nodes — https://github.com/PlagueKind/ComfyUI-PlagueKind-Nodes.git
21 ComfyUI-mxToolkit — https://github.com/Smirnov75/ComfyUI-mxToolkit.git
22 ComfyUI-MAINodes — https://github.com/matlowai/ComfyUI-MAINodes.git
```

## 3. HTTPS and SHA-256 utilities

`Downloader::fetch` in `crates/infrastructure/src/shared/download.rs` has this signature:

```rust
pub async fn fetch(
    &self,
    url: &str,
    destination: &Path,
    sha256: &str,
    max_bytes: u64,
    cancel: &CancelToken,
    progress: &(dyn Fn(u64, Option<u64>) + Send + Sync),
) -> Result<(), DownloadError>;
```

It accepts HTTPS URLs, writes to a `.part` file, checks the downloaded bytes against SHA-256, then renames the file to the destination; HTTPS redirect policy is enforced (`crates/infrastructure/src/shared/download.rs:59-79,100-134,137-195`). Helpers include `sha256_hex(bytes: &[u8]) -> String` and `sha256_file(path: &Path) -> std::io::Result<String>` (`:222-240`).

`Tools::fetch_and_extract` uses that downloader and `sha256_file` to reuse only correctly hashed uv/MinGit archives (`crates/infrastructure/src/environment/tools.rs:198-231`). Managed Python is handled separately: `python-downloads.json` contains pinned metadata, and uv receives it using `--python-downloads-json-url` (`crates/infrastructure/src/environment/pins.rs:1-12,51`; `crates/infrastructure/src/environment/python.rs:236-268`). The shared downloader is infrastructure code; `EnvironmentDeps` exposes a `ToolProvisioner`, not a direct downloader port (`crates/application/src/environment/deps.rs:85-98`).

## 4. Custom-node integration and setup-held lease

`NodeService::prepare_held` and `commit_held` accept `&mut dyn MaintenanceLease`:

```rust
pub async fn prepare_held(
    &self, request: PrepareRequest, ctx: &NodeContext,
    _lease: &mut dyn MaintenanceLease,
) -> Result<PlanReview, NodeAppError>;

pub async fn commit_held(
    &self, id: &str, decision: CommitDecision, ctx: &NodeContext,
    _lease: &mut dyn MaintenanceLease,
) -> Result<NodeOutcome, NodeAppError>;
```

Evidence: `crates/application/src/custom_nodes/workflow.rs:39-44,369-375`. `PrepareRequest` already supports registry and Git installs, as well as update, dependency retry, enable/disable, and uninstall (`crates/application/src/custom_nodes/model.rs:117-141`).

Currently, `selected()` reads `envs.active()` and accepts only `Verified`, except repair-like operations can accept `RepairRequired` (`workflow.rs:24-37`). Both held methods get the filesystem root from `d.root.current().root`; prepare uses that root for runtime-stop checks, snapshots, and staging (`workflow.rs:57-69,103-104`). A prepared plan binds `PlanReview.environment` to that selected environment and stores the root in `PreparedPlan` (`workflow.rs:232-275`; `model.rs:210-230,295-300`). Commit reselects the active environment and rejects a plan if the current root, environment ID, or pins differ from the plan (`workflow.rs:385-402`).

A first-run setup target is `run.target_env` and `run.python_target.env_path`, while setup’s root/path helpers use `run.request.install_root` and `run.python_target.env_path` (`crates/application/src/environment/execute.rs:120-127`; run construction at `setup.rs:122-139`). Before setup activation, that target is still provisioning and not the active verified environment, so the current `selected()` path rejects it. Setup-target support needs an explicit, validated root/environment target passed to both held operations and bound into the prepared plan and commit checks; it cannot rely on `root.current()` plus `envs.active()`.

`ExecuteSetup::execute` acquires the maintenance gate, awaits the full `run_locked` workflow, then drops the lease (`crates/application/src/environment/execute.rs:52-73`). The held node methods are the existing integration point for work under that lease; ordinary node `prepare` and `commit` acquire their own lease (`crates/application/src/custom_nodes/use_cases.rs:59-86`). `NodeService` is not currently a field in `EnvironmentDeps` (`crates/application/src/environment/deps.rs:82-99`).

## 5. Moved install and path persistence

The stated requirement is: if `install.json` has a different install root or the venv interpreter is broken, offer a guided rebuild using the same profile, reinstall node requirements, and keep user data (`docs/REQUIREMENTS.md:286`). Acceptance scenario 4 requires copying the root, detecting it, offering rebuild, then starting with models and nodes present (`docs/REQUIREMENTS.md:542-547`).

`InstallManifest` contains `install_root`, `env_path`, `profile_id`, and `profile_revision`, along with environment identity, ownership, Python version, ComfyUI reference/commit, run ID, and activation time (`crates/application/src/environment/model.rs:460-477`). `ManifestDoc` serializes those fields to `install.json`; it is read from `<root>/install.json` (`crates/infrastructure/src/environment/manifest.rs:22-35,110-127`; `crates/infrastructure/src/environment/layout.rs:63-65`).

`GetSetupStatus` reports `recovery_required` for corrupt/newer manifests, a missing manifest with an active env, a valid manifest with no active env, or a manifest/active-env ID mismatch. It does **not** compare `manifest.install_root` with the current root or probe the venv interpreter (`crates/application/src/environment/queries.rs:99-129,150-187`). The desktop DTO maps `SetupPhase::Recovery` to `recovery_required` (`crates/desktop/src/ipc/env_dto.rs:408-436`), and `App.tsx` says automatic moved-install rebuild is deferred to M6 (`ui/src/app/App.tsx:52-69`).

Startup resolves the current root from the debug override, bootstrap pointer, or default root (`crates/infrastructure/src/shared/paths.rs:132-164`). `crates/desktop/src/lib.rs:49-81` calls `paths.ensure_dirs()` before composing the app; `AppState::compose` does so again before opening `data/app.db` (`crates/desktop/src/state.rs:642-650`). Thus the current startup flow opens the root selected by the bootstrap pointer; it does not search for a moved copy before creating directories. `SelectInstallRoot` also refuses a root change once an active environment exists (`crates/application/src/environment/queries.rs:316-346`). `app_lifecycle.rs` handles window and runtime close behavior, not root migration (`crates/desktop/src/app_lifecycle.rs:1-4`).

`pyvenv.cfg` parsing reads its version and `home`. `venv_info` uses the venv’s own Python for an exact version when needed and reports the base interpreter only if `home/python.exe` (Windows) or `home/python3` exists; it does not validate containment against the new install root (`crates/infrastructure/src/environment/python.rs:174-200,382-423`). The startup setup-status query does not invoke that inspection. Runtime launch does use `PythonLaunchPreflight::capabilities`, which calls final verification and `main.py --help` when starting ComfyUI; that is a launch check, not a guided rebuild offer (`crates/infrastructure/src/comfy_runtime/process.rs:739-788`).

Paths relevant to a moved-root database migration:

- `envs.path`, optional `base_interpreter`, and `authorized_location` are stored as text paths; `env_from_row` restores them as `PathBuf` (`crates/infrastructure/migrations/002_environment.sql:3-22`; `crates/infrastructure/src/environment/sqlite.rs:51-88,112-139,218-256`).
- Provisioning-run JSON stores `request.install_root`, custom Python source path, `python_target.env_path`, and optional interpreter path as strings (`crates/infrastructure/src/environment/sqlite.rs:302-335,405-445,580-609`).
- Runtime session JSON stores absolute `interpreter` and `checkout` paths, and rejects non-absolute values on load (`crates/infrastructure/migrations/003_comfy_runtime.sql:6-10`; `crates/infrastructure/src/comfy_runtime/sqlite.rs:96-139`).
- `model_roots.path` exists in the schema, but the current library scan upsert writes an empty string for it. Model entries use `root_id` and relative `path`; extra roots are read from `extra_model_paths.yaml` and canonicalized in memory (`crates/infrastructure/migrations/005_library.sql:18-26`; `crates/infrastructure/src/models/library/sqlite.rs:240-253`; `crates/infrastructure/src/models/library/roots.rs:149-183`).
- Download jobs store `root_id` and `relative_path`, not an absolute destination path. The default model root and staging directory are derived from the current root (`crates/infrastructure/migrations/004_downloads.sql:1-17`; `crates/infrastructure/src/models/acquisition/stack.rs:28-60`).
- The hf tool venv is `<root>/tools/hf`, derived from `Layout`; `ensure_hf` creates it with uv and syncs a hash-locked requirements file. Its lock marker is a file under the tool directory, not a SQLite path (`crates/infrastructure/src/environment/layout.rs:23-41,76-81`; `crates/infrastructure/src/environment/tools.rs:348-405`).

## Open uncertainties

- Artifact-level wheel hashes, filenames, module names, provenance, and profile compatibility still need validation; the provided BAT’s wheel steps do not provide hashes. The handoff explicitly says not to invent hashes and to leave the default Python 3.13/3.14 accelerator lists empty until real import verification (`HANDOFF.md:126`).
- Setup-target node consent, plan binding, and failure/repair behavior need a design that works with the setup-held lease while preserving the node workflow’s review and commit guarantees.
- Relocation policy needs to distinguish app-managed paths from adopted/custom external venvs and external model roots; the current database also retains absolute runtime-session paths that a copy may leave stale.