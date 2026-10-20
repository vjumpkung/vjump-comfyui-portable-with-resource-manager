## 1. Go/No-go

**No-go on the draft as written. Go for implementation using the corrections below. Release acceptance remains open.**

The principal corrections are:

- Curated nodes must run **before final verification and activation**, against an explicitly bound setup candidate.
- Optional-component failures must never allow protected-package drift to become the new baseline.
- Relocation detection must precede startup reconciliation, background workers, and execution of persisted paths.
- HF jobs must already use the relay when initially uncapped, so subsequent settings changes affect running jobs.
- Wheel hashes establish artifact integrity, not Python/torch/GPU compatibility.
- Updater installation must bind to the release the user reviewed and complete application shutdown before the Windows installer exits the process.

This review was strictly read-only. No files were changed; no cargo or pnpm commands were run. Committed baseline inspected: `8310f12`; concurrent M5 edits were treated as unstable. The accepted M5 plans remain prerequisites.

## 2. Corrected plan

All paths below are relative to the repository root.

### Work allocation and merge boundaries

Use three implementation worktrees, followed by one integration owner.

| Package | Dependencies | Exclusive implementation files | Parallelism |
|---|---|---|---|
| **E1 — Accelerators** | Stable M5 environment contracts | Domain/application/infrastructure environment files listed below | Parallel with B and P |
| **E2 — Curated setup nodes** | E1, accepted M5 backend | Environment files plus application custom-node files listed below | Sequential after E1 |
| **E3 — Relocation/rebuild** | E2, M5 requirement provider | Environment, bootstrap-path and persistence files listed below | Sequential after E2 |
| **B — Bandwidth** | None beyond committed acquisition implementation | Acquisition implementation and its tests | Parallel with E and P |
| **P — Updater/diagnostics** | None for implementation; integrated M5 runtime orchestration for acceptance | New platform modules, diagnostics/logging files | Parallel with E and B |
| **I — Desktop/UI integration** | E, B, P merged | Shared composition, IPC registration, manifests, dependencies, UI | One owner |
| **R — Release audit and README** | I verified | Packaging configuration, release documentation, README | Last |

E1–E3 intentionally share one worktree and owner: they overlap substantially in `environment/{model,ports,setup,execute,queries}.rs`. Splitting them into simultaneous coders would create avoidable merge and state-machine conflicts.

**Reserve these shared files for I:** workspace/desktop/infrastructure `Cargo.toml`, `Cargo.lock`, desktop `src/{lib,state,app_lifecycle}.rs`, `src/ipc/{mod,bindings,error,commands,dto,env_commands,env_dto}.rs`, `build.rs`, `capabilities/manager.json`, desktop `tests/{acl.rs,support/mod.rs}`, module-export changes outside each package’s owned module, and all frontend files. Implementation packages provide concrete wiring changes to I.

Regenerate `ui/src/shared/ipc/bindings.ts` once after integrating the Rust contract. Do not hand-edit generated bindings or permissions.

### E1 — Hashed accelerators, selections, outcomes and pin safety

**Files**

- `crates/domain/src/environment/{profile,error,mod}.rs`
- `crates/application/src/environment/{model,ports,deps,setup,execute,queries,activation}.rs`
- `crates/application/src/environment/tests/{fakes,setup,queries,activation}.rs`
- `crates/infrastructure/src/environment/{profiles,packages,verify,sqlite,manifest}.rs`
- Corresponding existing `profiles/tests.rs`, `packages/tests.rs`, `verify/tests.rs`, `sqlite/tests.rs`, `manifest/tests.rs`
- New `crates/infrastructure/src/environment/wheels.rs`
- New `crates/infrastructure/src/environment/wheels/tests.rs`
- Existing bundled profile JSON files; archive the changed profile’s revision 1.
- New migration `007_environment_components.sql`, registered by I.
- Constructor/fake updates in existing environment integration tests.

#### Contract

Retain support for package components as required by §5.4. Use:

```rust
pub enum AcceleratorSource {
    Wheel {
        url: String,
        filename: SafeFileName,
        sha256: Sha256,
        size: Option<u64>,
    },
    Packages {
        requirements: Vec<String>,
    },
}

pub struct AcceleratorSpec {
    pub id: String,
    pub name: String,
    pub modules: Vec<String>,
    pub source: AcceleratorSource,
    pub no_deps: bool,
    pub default_selected: bool,
    pub non_fatal: bool,
    pub provenance: String,
}
```

Validate unique, stable lowercase IDs; Python module identifiers; safe wheel filenames ending in `.whl`; hashes through the existing `Sha256` value object; and nonempty provenance. Infrastructure additionally validates parsed HTTPS URLs, nonempty authority, no userinfo, and no credential-bearing query.

Schema 2 uses these fields. Schema 1 remains readable **only with its historical empty accelerator list**; reject unsupported nonempty legacy accelerator records rather than silently discarding them. Preserve unknown/newer-schema refusal.

Add:

```rust
#[async_trait]
pub trait WheelFetcher: Send + Sync {
    async fn fetch(
        &self,
        root: &Path,
        spec: &AcceleratorSpec,
        ctx: &StepContext,
    ) -> Result<PathBuf, ProvisionError>;
}
```

- `EnvironmentDeps.wheels: Arc<dyn WheelFetcher>`
- `InstallSpec.no_deps: bool`, default false.
- `SetupRequest.accelerators: Vec<String>`: explicit selected IDs; duplicates/unknown IDs are rejected. Empty means none. Defaults belong in the wizard.
- `ProvisioningRun` stores per-component results, saved after each attempt.

Use:

```rust
pub enum ComponentStatus { Installed, Failed, Skipped }

pub struct AcceleratorResult {
    pub id: String,
    pub selected: bool,
    pub status: ComponentStatus,
    pub importable: Option<bool>,
    pub message: Option<String>,
}
```

`importable: None` means untested, not success. Preserve installation and import outcomes separately.

Add `EnvironmentRepository::{load_components, save_components}` using application `InstalledComponents`, containing Manager selection/result/source commit and accelerator results. Migration 007 adds an additive `components_json` column. Persist equivalent components in manifest schema 2; read schema 1 with unknown component metadata and recover known selections from its recorded provisioning run.

`env_info` returns these stored outcomes, not the current profile’s defaults.

#### Artifact data

Use these exact wheel filenames and SHA-256 values:

| ID / module | Filename | SHA-256 |
|---|---|---|
| `sageattention` / `sageattention` | `sageattention-2.2.0+cu130torch2.9.0andhigher.post4-cp39-abi3-win_amd64.whl` | `f7261c53a1d46202059daff6b8feac5ee806274d47e3e7fb7abf8c5f6d410c92` |
| `llama-cpp-python` / `llama_cpp` | `llama_cpp_python-0.3.34+cu130.basic-cp312-cp312-win_amd64.whl` | `f418f3fe281c1d6c9e8d996e730e955a7d8b7ad447e7d09a6a1c383e64684f40` |
| `flashattention-2` / `flash_attn` | `flash_attn-2.8.3+cu130torch2.10.0cxx11abiTRUE-cp312-cp312-win_amd64.whl` | `d1e76adda81f57d9a300b9e8d71b9c467e07923e0d8e5baa5d90e2007815ec93` |
| `flashattention-3` / candidate `flash_attn_interface` | `flash_attn_3-3.0.0+20260318.cu130torch2100cxx11abitrue.8afc61-cp39-abi3-win_amd64.whl` | `36642bbbaea59e035b3c57b3fa1dded0e40598949321917cc218c71bd79b184b` |
| `onnxruntime-gpu` / `onnxruntime` | `onnxruntime_gpu-1.26.0-cp312-cp312-win_amd64.whl` | `7388afa11b005eea389e06d60c4a290494a4dd71e507a24ea21890dd9c39b547` |

URL prefixes, followed by the filename with `+` encoded as `%2B`:

```text
sageattention:
https://github.com/woct0rdho/SageAttention/releases/download/v2.2.0-windows.post4/

llama-cpp-python:
https://github.com/JamePeng/llama-cpp-python/releases/download/v0.3.34-cu130-Basic-win-20260331/

flashattention-2:
https://huggingface.co/ussoewwin/Flash-Attention-2_for_Windows/resolve/main/

flashattention-3:
https://github.com/windreamer/flash-attention3-wheels/releases/download/2026.03.19-850211f/

onnxruntime-gpu:
https://aiinfra.pkgs.visualstudio.com/2692857e-05ef-43b4-ba9c-ccf1c22c437c/_packaging/22685817-9e91-4967-bafe-94c9843c26a9/pypi/download/onnxruntime-gpu/1.26/
```

Known sizes, in the same order: `12321863`, `311186533`, `367193483`, `384220928`, unknown.

For `win-py312-torch2100-cu130` revision 2:

- Add these five wheels as **experimental, initially unchecked** components.
- `no_deps = true` only for ORT; false for the other four.
- Add optional component `compatibility-packages`, requirements `transformers==5.5.0`, `onnx`, `numpy<2.3`, `pillow<12`.
- Add profile constraints `numpy<2.3`, `pillow<12`; add `transformers==5.5.0` as the compatibility profile’s resolver constraint. A constraint alone does not install the package.
- Record publisher hash provenance and absence of app import validation.
- Keep all other profiles’ accelerator lists empty.
- Confirm FA3’s actual installed module from the hashed wheel before presenting an import claim; the supplied evidence explicitly leaves that name unverified.

Do not describe these components as release-validated until native validation passes.

#### Execution

- Cache wheels under `data/cache/wheels/<sha256>/<filename>`.
- Rehash cached files; never trust existence alone.
- Reuse shared `Downloader::fetch`, with cancellation, HTTPS redirect policy and bounded download size. For unknown size use an explicit 2 GiB limit.
- Install the verified local wheel path with constraints, `--only-binary :all:`, and optional `--no-deps`.
- Package components also remain wheel-only.
- Reassert original protected pins after **every attempted dependency mutation**, including failure/cancellation. Cleanup uses a fresh token while retaining the same lease.
- Add `ProvisionError::PinDrift`; inability to restore/prove pins is fatal even inside BestEffort/Optional steps.
- Final verification must compare against recorded pins **before** recording verification. Remove its current ability to bless newly observed protected versions.
- Optional import tests run separately with bounded subprocesses. A failed accelerator import is a warning; a missing/core CUDA failure remains fatal.
- Rebuild uses the recorded profile snapshot and component selections.

**New IPC error codes:** `accelerator_selection_invalid`, `profile_revision_unavailable`; retain `pin_drift`, `cancelled`, and existing setup failure reporting.

**Tests**

| Name | Required assertions |
|---|---|
| `accelerator_specs_reject_invalid_artifacts` | Bad hashes, filenames, URLs, module names and duplicate IDs refused. |
| `profile_v1_empty_and_v2_components_round_trip` | Legacy empty lists survive; unsupported legacy records are refused. |
| `wheel_cache_is_rehashed_before_reuse` | Modified cache never reaches installer. |
| `wheel_hash_failure_never_runs_uv` | No package mutation on digest mismatch. |
| `accelerator_selection_is_explicit_and_profile_bound` | Empty selection installs nothing; unknown/duplicate IDs fail. |
| `accelerator_failure_reasserts_original_pins` | Guard runs after failed install; original failure retained when guard succeeds. |
| `optional_pin_drift_prevents_activation` | Unrecoverable drift fails candidate; previous active selection unchanged. |
| `final_verification_cannot_rebaseline_drift` | Unexpected protected versions are rejected, not saved as new pins. |
| `accelerator_import_failure_is_reported_separately` | Successful wheel installation plus import failure remains visible and nonfatal. |
| `components_survive_manifest_and_database_round_trip` | Selections/results remain associated with the correct environment. |

### E2 — Curated nodes through M5, before activation

**Files**

- New `crates/application/src/custom_nodes/setup.rs`
- `crates/application/src/custom_nodes/{model,workflow,use_cases,tests}.rs`
- `crates/application/src/environment/{model,ports,setup,execute}.rs`
- Environment test fakes/setup tests
- New `crates/infrastructure/src/environment/curated.rs`
- New `crates/infrastructure/src/environment/data/curated_nodes.json`
- Environment run JSON codec/tests in `sqlite.rs`
- I owns exports, DTOs, composition and UI.

#### Catalog

```rust
pub struct CuratedNodeSpec {
    pub id: String,
    pub name: String,
    pub repository: String,
    pub branch: String,
    pub commit: String,
    pub provenance: String,
}

pub trait CuratedNodeCatalog: Send + Sync {
    fn list(&self) -> Vec<CuratedNodeSpec>;
}
```

Catalog URLs are `https://github.com/<repository>.git`. Preserve this order; branch is `main` except where shown:

| Repository | Commit | Other branch |
|---|---|---|
| `pollockjj/ComfyUI-MultiGPU` | `37a73feda5804045e4f39d8c640905d62a2169d1` | |
| `molbal/ComfyUI-GGUF` | `f1d050fac00b0505cba519e44ad7fb18c34318b0` | |
| `kijai/ComfyUI-KJNodes` | `17195fcfa8a05f601b766a0f09c9733284f47416` | |
| `seitanism/ComfyUI-H3-Motion-Context-MultiRef` | `361624fb406b63eb6694442eac6c895fc1533a70` | |
| `Kosinkadink/ComfyUI-VideoHelperSuite` | `4d907bee61e92c2e65af3bd6383a4e4d356126d1` | |
| `rgthree/rgthree-comfy` | `5f940ddfbb83ff9c667f32ae7d0c3ad0bb8cc9ee` | `dev` |
| `crystian/ComfyUI-Crystools` | `2f18256c5b5063937106f29a8e0a7db3ae3869b7` | |
| `kijai/ComfyUI-WanVideoWrapper` | `088128b224242e110d3906c6750e9a3a348a659b` | |
| `pythongosssss/ComfyUI-Custom-Scripts` | `609f3afaa74b2f88ef9ce8d939626065e3247469` | |
| `WASasquatch/was-node-suite-comfyui` | `49a1098630dedff9e0ef85e8e6c4d9a4ac85f8a1` | |
| `1038lab/ComfyUI-QwenVL` | `1b67b443918801f571714bab636edc1845b7002a` | |
| `kijai/ComfyUI-MelBandRoFormer` | `92c86854e6654f4aacc97484471af95c98ea16d4` | |
| `ClownsharkBatwing/RES4LYF` | `56e44755d866c87ff9cb4b6608c8e42155db6ecc` | |
| `kijai/ComfyUI-SolAttn_triton` | `26d816ebd4f1e43a2c6e4d4759be3137f10a7a73` | |
| `vjumpkung/comfyui-infinitetalk-native-sampler` | `2bcb9e7e75348044b960d7759438c97e6b4ab289` | |
| `vjumpkung/comfyui-mosaic-blur` | `ea6e4d21b81e74d1f7fbc2c18d037d23afdaf6d3` | `master` |
| `kijai/ComfyUI-WanAnimatePreprocess` | `0e0b6a2a555625acf4d4aefb780e27d06937132f` | |
| `vjumpkung/comfyui-wan-animate-2-loop-sampler` | `64f9b89c1c478000de8ec2db464d16ef4ab2954e` | |
| `vjumpkung/comfyui-scail-2-loop-sampler` | `569fa4a3c19e78a20a7dfa239955efd64fe77881` | |
| `PlagueKind/ComfyUI-PlagueKind-Nodes` | `d58d006a4ea32c25c06499f2ff104f0852a045a6` | |
| `Smirnov75/ComfyUI-mxToolkit` | `7f7a0e584f12078a1c589645d866ae96bad0cc35` | |
| `matlowai/ComfyUI-MAINodes` | `f4868b4a08e8a504ce86db54a17961d399ffa2bc` | |

Use catalog IDs derived from repository names and validate uniqueness. Store provenance: reference installer order; commits resolved at `2026-10-16T17:11Z`.

#### Target binding and construction

Add an internal target:

```rust
pub enum NodeTarget {
    Active,
    Setup(RunId),
}
```

Keep ordinary held APIs as Active wrappers. Add:

```rust
pub async fn prepare_for_setup_held(
    &self,
    run: &RunId,
    request: PrepareRequest,
    ctx: &NodeContext,
    lease: &mut dyn MaintenanceLease,
) -> Result<PlanReview, NodeAppError>;

pub async fn commit_for_setup_held(
    &self,
    run: &RunId,
    id: &str,
    decision: CommitDecision,
    ctx: &NodeContext,
    lease: &mut dyn MaintenanceLease,
) -> Result<NodeOutcome, NodeAppError>;
```

Resolve the target from the persisted run, not caller-supplied root/environment paths. Require:

- Running first-run setup at `CuratedNodes`.
- Current configured root equals the run root.
- Candidate ID/path/pins match the run and environment record.
- Candidate is Provisioning.
- ComfyUI is stopped under the already-held lease.

Bind target kind, run ID, root, candidate ID and pins into the prepared plan. Ordinary `node_commit` must refuse setup plans; ordinary pending-review listing excludes them.

Avoid a construction cycle: **do not put `NodeService` inside `EnvironmentDeps`**. Inject a separate `Arc<dyn SetupNodes>` into `ExecuteSetup::new(deps, nodes)`, implemented by the already-constructed `NodeService`:

```rust
#[async_trait]
pub trait SetupNodes: Send + Sync {
    async fn prepare(
        &self, run: &RunId, entry: &CuratedNodeSpec,
        ctx: &StepContext, lease: &mut dyn MaintenanceLease,
    ) -> Result<PlanReview, ProvisionError>;

    async fn commit(
        &self, run: &RunId, plan: &str, decision: CommitDecision,
        ctx: &StepContext, lease: &mut dyn MaintenanceLease,
    ) -> Result<NodeOutcome, ProvisionError>;
}
```

#### Consent and execution

Superseded for first-run setup by the owner decision of 2026-10-20 (`plans/s6-owner-feedback-plan.md`, F).

- `SetupRequest.curated_nodes: Vec<String>` contains catalog IDs, not arbitrary repository/commit pairs.
- Offer only for FirstRun; default all 22 selected (superseded 2026-10-20: none selected by default).
- Selection is not execution consent.
- Prepare sequentially at the exact catalog commit; require resolved source equality.
- Persist per-entry progress and one pending `PlanReview`.
- Pause for exact M5 review/consent. Extend Retry with `curated_decision: Option<CommitDecision>`.
- Expose pending review through `RunSnapshotDto.curatedReview`, using existing `PlanReviewDto`.
- Never automatically acknowledge newly discovered warnings.
- Fix all setup decisions to `run_install_py=false`, `remember_install_py=false`, `allow_source_builds=false`; do not remember source trust from setup.
- Explain skipped hooks and direct users to the existing Custom Nodes Retry review afterward.
- Save journal operation IDs before proceeding to the next entry.
- On backend restart, invalidate uncommitted process-local reviews and obtain fresh consent. Never replay an obsolete binding.
- On retry, recognize an already installed exact source with successful candidate dependency outcomes; do not clone or execute dependencies again unnecessarily.

Dependency failure after publication records Failed outcomes against the **candidate**, marks that candidate Failed, pauses setup and prevents activation. It must not mark the old active environment damaged.

Add `StepFailure.can_skip`; backend computes it. Preparation-only failures may be skipped. Published dependency damage or pin drift cannot be bypassed through Skip. Retry replays mandatory candidate/core/node requirement checks; final activation requires successful dependency outcomes.

**Commands/DTOs**

- `setup_list_curated_nodes() -> Vec<CuratedNodeDto>`
- DTO fields: `id`, `name`, `repository`, `branch`, `commit`, `provenance`.
- Extend existing setup request, Retry and run snapshot DTOs as above.
- New errors: `curated_selection_invalid`; reuse `node_plan_stale`, `node_consent_required`, `step_not_skippable`, `pin_drift`.

**Tests**

- `curated_catalog_matches_reference_order_and_commits`: exactly 22 unique valid entries.
- `setup_node_target_never_selects_the_active_environment`: old active interpreter receives no mutation.
- `setup_node_plans_bind_run_root_candidate_and_pins`: any changed binding is refused.
- `curated_commit_mismatch_never_publishes`.
- `curated_warnings_require_exact_review_acknowledgement`.
- `curated_setup_never_runs_install_py`.
- `curated_dependency_failure_prevents_activation`.
- `skip_cannot_hide_published_dependency_damage`.
- `curated_retry_is_idempotent_after_committed_entries`.
- `backend_restart_requires_fresh_curated_consent`.

### E3 — Missing-root recovery and guided rebuild

**Files**

- `crates/domain/src/environment/env.rs`
- New `crates/application/src/environment/relocation.rs`
- Application environment `{model,ports,setup,execute,queries,activation}.rs` and tests
- New `crates/infrastructure/src/environment/relocation.rs`
- Infrastructure environment `{python,tools,switcher,sqlite,manifest}.rs` and adjacent tests
- `crates/infrastructure/src/shared/paths.rs`
- New migration `008_install_relocation.sql`
- I owns startup/state/IPC/UI changes.

#### Bootstrap and health contracts

Add:

```rust
pub enum RebuildReason { Moved, InterpreterBroken }

pub enum InstallationHealth {
    Healthy,
    RebuildRequired(RebuildReason),
}

#[async_trait]
pub trait InstallationProbe: Send + Sync {
    async fn inspect(
        &self,
        current_root: &Path,
        manifest: &InstallManifest,
        active: &PythonEnvironment,
    ) -> Result<InstallationHealth, ProvisionError>;
}
```

- Compare roots using normalized component-aware paths; canonicalize existing paths, but do not require the disappeared old root to canonicalize.
- Probe all active venv ownership/source kinds, not only managed ones.
- For managed Python, validate `pyvenv.cfg.home` against the actual managed directory from `Layout`—currently under `data/cache`, not an invented `data/...uv` path.
- Run a 10-second isolated interpreter identity/version/prefix probe. A present executable is insufficient.
- Status polling must not spawn overlapping probes; reuse an in-flight/result cache and explicitly invalidate after setup.

Before `ensure_dirs`, log initialization at the install root, database creation or normal composition:

- A missing **bootstrap-selected existing root** enters `root_missing`.
- A genuinely new default root may still enter first-run creation.
- Permission/inaccessibility errors are not treated as absence.
- Preserve the recorded path in recovery state.

For a moved existing root, detect the mismatch **before** `ReconcileStartup`, runtime recovery, node/library mutation or starting download/watch workers. Compose restricted setup/recovery services. Normal commands return `rebuild_required`; setup status, diagnostics, folder picking and rebuild remain available.

#### Open-existing and new-install recovery

Add a root-independent port:

```rust
#[async_trait]
pub trait ExistingInstallOpener: Send + Sync {
    async fn open_existing(&self, path: &Path)
        -> Result<(), RootSwitchError>;

    async fn select_new(&self, path: &Path)
        -> Result<(), RootSwitchError>;
}
```

`open_existing`:

- Validates a real, contained root, supported manifest, existing readable DB, matching active environment identity, and required ComfyUI checkout.
- Opens the candidate DB without create semantics.
- Does not transfer current settings/draft into the existing DB.
- Verifies no relevant current/candidate/recorded-old-root ComfyUI process is alive.
- Uses the process-global maintenance gate, including bootstrap recovery mode.
- Atomically changes only the bootstrap pointer after validation.
- Rejects debug override mode.
- Quiesces background work before restart.

`select_new` is available in missing-root/bootstrap recovery and accepts an empty/new destination; it must not overwrite an existing installation.

Do not reuse current `FsRootSwitcher::switch_root` for opening existing installations: it creates databases and copies settings.

**Commands**

```text
setup_open_install(path: string) -> { restartRequired: true }
setup_new_install(path: string)  -> { restartRequired: true }
```

Have the desktop controller request a controlled restart after successful pointer publication. These commands must work when `state.services()` is unavailable.

Errors: `root_missing`, `root_invalid`, `root_fixed_by_override`, `root_switch_failed`, `comfyui_running`, `comfyui_state_unknown`, `maintenance_busy`, `relocation_recovery_required`.

#### Rebuild

Add `SetupMode::Rebuild`, serialized as `rebuild`.

Backend derives its immutable input from the manifest and successful recorded provisioning snapshot:

- Exact profile ID/revision, Python version, component selections and existing checkout.
- No fallback to the newest profile revision.
- If the exact snapshot/revision is unavailable, return `profile_revision_unavailable`.
- Always build a new app-owned venv at `<current-root>/envs/<new-id>`.
- Never mutate/delete the adopted venv. Explicit rebuild action confirms replacement; existing adopted files remain untouched.
- Reuse the checkout without fetch, checkout/reset or clone. Preserve local edits; unsafe/non-Git/missing checkout is a specific refusal.
- Reinstall enabled and disabled node requirement sets through accepted M5 `NodeRequirementSource`; all failures prevent activation.
- No automatic hooks or newly resolved curated node versions.
- Recreate the HF tool venv. Its existing executable plus `.vj-lock` marker is not sufficient after relocation; include interpreter/root identity in the marker and verify tool launch.
- Preserve models, nodes, `user`, `input`, `output`, external model roots and user launch options.

Migration 008 supplies an idempotent relocation record keyed by old/new root. On explicit rebuild acceptance, transactionally:

- Rebase only persisted paths proven to be under the recorded old root, using path components.
- Rebase associated authorized locations too; never leave Delete pointing into the original installation.
- Mark copied managed environments unusable using existing repair/failed states; preserve their directories.
- Leave external paths unchanged.
- Retain historical run documents, but prohibit retry/execution of runs bound to another root.
- Retain runtime-session history; after proving processes stopped, mark stale sessions finished. Do not silently delete their history.
- Keep downloads/model/node locations relative; do not perform a global string replacement.
- Refuse automatic relocation when incomplete activation, ComfyUI-update or ambiguous publication journals require recovery. Preserve them and report `relocation_recovery_required`.

Publish the new-root manifest only through the existing activation journal after all required verification passes. Normal services/workers start after successful rebuild and restart.

**Tests**

- `missing_bootstrap_root_creates_no_install_directories`.
- `first_run_default_root_is_distinct_from_missing_existing_root`.
- `open_existing_never_creates_a_database_or_copies_settings`.
- `relocation_is_detected_before_any_reconciliation`.
- `copied_run_cannot_publish_a_manifest_into_the_old_root`.
- `broken_adopted_interpreter_offers_owned_replacement_without_mutation`.
- `rebuild_uses_recorded_profile_revision_and_components`.
- `rebuild_preserves_checkout_and_user_data`.
- `relocation_rebases_authorized_locations_without_touching_external_paths`.
- `relocation_is_idempotent_and_preserves_history`.
- `moved_hf_marker_cannot_skip_tool_recreation`.
- `rebuild_node_dependency_failure_preserves_previous_selection`.

### B — One live bandwidth budget for native and HF

**Files**

- New `crates/infrastructure/src/models/acquisition/{bandwidth,hf_proxy}.rs`
- `crates/infrastructure/src/models/acquisition/{native_engine,hf_engine,stack,tests}.rs`
- Module-local tests for the new files
- I owns exports, dependency features, settings command and UI.

```rust
pub struct BandwidthLimiter { /* shared state */ }

impl BandwidthLimiter {
    pub fn new(limit_kib_s: Option<NonZeroU32>) -> Self;
    pub fn set_limit(&self, limit_kib_s: Option<NonZeroU32>);

    pub async fn acquire(
        &self,
        max_bytes: usize,
        cancel: &CancelToken,
    ) -> Result<usize, BandwidthError>;
}
```

`acquire` returns a partial allowance; callers slice large buffers. Use bounded FIFO grants, bounded bursts, overflow-safe `u64` byte-rate conversion, and cancellation polling while waiting. Tiny caps must not deadlock on a chunk larger than bucket capacity.

- Store one `Arc<BandwidthLimiter>` in `DownloadStack`.
- Native transfer consumes grants before writing each bounded slice.
- Every HF attempt uses a lifetime-bound loopback CONNECT relay, **including when unlimited**.
- Limit server-to-child bytes through the same bucket. TLS remains opaque.
- Bind only `127.0.0.1:0`; accept CONNECT only, DNS host plus port 443.
- Permit `huggingface.co`, subdomains of `huggingface.co`, and `hf.co`/its subdomains with proper DNS-label boundaries. Reject userinfo, IP literals, malformed authorities and private/loopback destination resolution.
- Bound headers, connection count and handshake timeouts.
- Close/join relay connections after child termination on success, pause, cancellation and error.
- Set upper/lowercase child HTTP(S) proxy variables consistently; clear bypass/alternate-proxy variables so they cannot evade the relay.
- Resolve upstream proxy settings once from the parent’s environment before constructing child overrides. Support the existing HTTP/HTTPS proxy forms; honor original bypass rules for the relay’s upstream route, never by bypassing the relay itself.
- Reject unsupported proxy configurations clearly; never silently connect directly.
- Do not log proxy credentials, CONNECT payloads or TLS contents.
- Do not disable Xet or switch engines to implement throttling. HF’s Rust Xet client has a separate HTTP stack, so Python-client configuration alone is insufficient. [Hugging Face’s explanation](https://github.com/huggingface/huggingface_hub/issues/3530)

**Settings**

Keep existing key `bandwidth_cap_kib_s`, existing `Option<NonZeroU32>` validation and persisted `"unlimited"` representation. After successful `settings_set`, update both concurrency and the shared limiter. No new command or settings key is necessary.

Add acquisition error code `bandwidth_proxy_unavailable`; it must not masquerade as `ToolLaunch` and trigger unthrottled native fallback.

**Tests**

- `shared_bucket_bounds_combined_native_and_relay_bytes`: total bounded by rate × elapsed plus documented burst.
- `small_limits_split_large_chunks_without_deadlock`.
- `queued_waiters_cancel_without_leaking_capacity`.
- `live_cap_changes_affect_already_running_hf_and_native_jobs`.
- `relay_rejects_non_connect_and_invalid_authorities`.
- `relay_host_checks_use_dns_label_boundaries`.
- `relay_chains_upstream_proxy_without_logging_credentials`.
- `hf_proxy_configuration_changes_only_child_environment`.
- `hf_child_exit_closes_all_relay_connections`.
- Opt-in `real_hf_xet_obeys_relay_with_direct_egress_denied`.

### P — Signed updater and complete diagnostics

**Files**

- New `crates/application/src/settings/{updates,diagnostics}.rs`, with module-local tests
- New `crates/desktop/src/{updates,diagnostics}.rs`
- New `crates/desktop/src/ipc/{app_commands,app_dto}.rs`
- New `crates/infrastructure/src/shared/log_tail.rs`
- `crates/infrastructure/src/shared/logging.rs` and tests
- I owns dependencies, exports, composition, registration and UI.

#### Updater

Pin updater `=2.14.0` and clipboard `=2.5.0`.

Use **`VJUMP_UPDATER_PUBKEY` and `VJUMP_UPDATER_ENDPOINT`**, matching the existing release workflow. Remove the draft’s inconsistent `VJ_UPDATER_*` names. Add build-script rerun tracking.

Register the updater plugin only with complete valid build-time configuration. Keep custom command registration stable; absent configuration returns a controlled refusal. Grant no updater/clipboard plugin IPC permissions to webviews.

Application port:

```rust
#[async_trait]
pub trait AppUpdater: Send + Sync {
    fn configured(&self) -> bool;

    async fn check(&self)
        -> Result<Option<UpdateOffer>, UpdateError>;

    async fn install(
        &self,
        update_id: &str,
        sink: Arc<dyn UpdateProgressSink>,
    ) -> Result<(), UpdateError>;
}
```

The desktop adapter retains the exact plugin `Update` behind an opaque process-local ID. Installation consumes that ID; it cannot re-resolve and install a different release.

**Commands**

```text
app_update_status()
  -> { configured, currentVersion }

app_update_check()
  -> null | { updateId, version, date, notes }

app_update_install(updateId, onEvent)
  -> void
```

Progress union: `downloading { downloadedBytes, totalBytes }`, `installing`, `restarting`, `failed { code }`. Byte counts follow existing JS-safe numeric DTO conventions.

Installation orchestration:

1. Acquire maintenance gate.
2. Verify stopped runtime, including headless instances.
3. Validate the retained reviewed offer.
4. Download and verify through the plugin.
5. Quiesce downloads and other background mutation before installer/exit; retain ownership through cleanup.
6. Install, then restart using platform-appropriate behavior.

A lost UI channel does not cancel installer work. Preserve default no-downgrade behavior and HTTPS/signature verification.

Windows updater installation can invoke cleanup and exit internally; cleanup cannot be deferred to code after `install()`. [Pinned updater implementation](https://raw.githubusercontent.com/tauri-apps/plugins-workspace/updater-v2.14.0/plugins/updater/src/updater.rs)

Errors: `updater_not_configured`, `update_stale`, `update_check_failed`, `update_signature_invalid`, `update_install_failed`, plus existing maintenance/runtime errors.

**Cut `check_updates_on_launch` from M6.** Manual signed self-update satisfies FR-SET-4; automatic checking and another setting are unnecessary for this release’s minimum scope. No new updater settings key.

#### Diagnostics

```rust
#[async_trait]
pub trait AppLogTail: Send + Sync {
    async fn last_lines(&self, limit: usize)
        -> Result<Vec<String>, DiagnosticsError>;
}
```

`CollectDiagnostics::execute()` returns a plain application snapshot containing:

- App version; OS; GPU/driver/driver CUDA.
- Active environment identity, ownership, Python, protected pins, accelerator outcomes.
- Manifest profile ID/revision.
- ComfyUI revision/status.
- Pinned uv/HF versions, explicitly labelled pinned rather than observed if not probed.
- Last 200 app-log lines and separately labelled runtime tail.
- Availability warnings for missing sections.

Missing checkout, missing env or unreadable logs must not make the whole report unavailable. Reuse existing runtime queries where possible; do not run setup or import node code.

Tail reads are bounded by bytes as well as lines and run off the UI thread.

**Commands**

```text
app_diagnostics() -> string
app_copy_diagnostics() -> { text: string, copied: boolean }
```

The copy command collects and copies the same fully redacted text it returns. Clipboard failure returns `copied=false`; the UI displays that text in a selectable read-only textarea.

Redact the **entire rendered report** immediately before return/copy. Replace known install-root occurrences with `<root>` for presentation. Extend redaction coverage for signed URL query credentials, including non-AWS signed URLs used by these downloads.

**Tests**

- `updater_without_complete_config_never_checks_network`.
- `update_install_requires_the_reviewed_offer`.
- `update_install_checks_runtime_after_acquiring_gate`.
- `invalid_signature_never_invokes_installer`.
- `update_cleanup_precedes_windows_installer_exit`.
- `disconnected_update_channel_does_not_release_ownership`.
- `diagnostics_handles_first_run_and_missing_sections`.
- `diagnostics_caps_app_tail_by_lines_and_bytes`.
- `diagnostics_redacts_every_section_and_clipboard_payload`.
- `clipboard_failure_returns_selectable_redacted_text`.

### I — Integrate IPC, recovery composition and UI

In addition to reserved shared files, this owner touches:

- `ui/src/shared/ipc/{gateway,gateway.test}.ts`
- `ui/src/features/setup/{model,setup-wizard,run-progress}` and their tests
- `ui/src/features/environment/environment-page.tsx` and tests
- `ui/src/features/settings/{settings-form,settings-page}.tsx`
- New settings `updates-panel.tsx`, `diagnostics-panel.tsx` and tests
- `ui/src/features/comfy-runtime/comfy-page.tsx` and tests
- `ui/src/app/App.tsx`, `shell/app-shell.tsx` and tests

Required integration:

- Construct M5 NodeAdapters → NodeRequirements → EnvironmentDeps → NodeService → ExecuteSetup with separate SetupNodes injection.
- Keep one process-global gate, usable in rootless recovery.
- Add restricted recovery/setup composition; do not construct/start normal acquisition/library workers against a moved installation before recovery.
- Register all seven new commands: `setup_list_curated_nodes`, `setup_open_install`, `setup_new_install`, `app_update_status`, `app_update_check`, `app_update_install`, `app_diagnostics`, `app_copy_diagnostics`—**eight commands in total**.
- Add valid-body ACL cases for all eight, including recovery-mode handlers.
- Keep all Tauri Channels and native access inside the gateway.
- Curated selection defaults on; execution consent remains unchecked and bound to actual reviews.
- Compose any reuse of M5 review UI through public exports/app composition; no feature-internal cross-imports.
- On profile changes, reset accelerator selections to that profile’s defaults.
- Show stored accelerator results on Environment.
- Show missing-root Locate/New actions and guided-rebuild summary.
- Enable the existing bandwidth field with integer KiB/s validation; empty means unlimited.
- Show updater actions only when configured; display notes as plain text.
- Route both diagnostics buttons through the new app diagnostics commands.
- Remove obsolete M6/deferred and “Updating ComfyUI is a later milestone” copy.

Required UI tests:

- `profile_change_resets_accelerator_selection`.
- `empty_accelerator_selection_remains_empty`.
- `curated_selection_does_not_authorize_execution`.
- `curated_binding_change_clears_consent`.
- `damaged_optional_step_cannot_be_skipped`.
- `missing_root_offers_locate_and_new_install`.
- `rebuild_shows_preserved_data_and_owned_replacement`.
- `bandwidth_empty_means_unlimited_and_zero_is_invalid`.
- `update_install_echoes_reviewed_update_id`.
- `diagnostics_copy_failure_exposes_read_only_fallback`.
- Gateway tests verify exact command payloads and channel encapsulation.

### R — Packaging, release audit and README

**Files**

- `crates/desktop/tauri{,.windows,.linux,.updater}.conf.json`
- `scripts/tauri.mjs`
- `.github/workflows/release.yml`
- `docs/{IMPLEMENTATION_STATUS,REQUIREMENTS}.md`
- `HANDOFF.md`
- New `docs/RELEASE_ACCEPTANCE.md`
- Finally `README.md`

Actions:

- Verify platform overlays are actually discovered by the wrapper.
- Verify executable/Cargo/Tauri versions agree.
- Build UI before production embedding.
- Produce and install NSIS/MSI on Windows; produce and install AppImage/deb on Linux.
- Verify WebView2 bootstrapper, Linux runtime dependencies, single-instance focus, bundled fonts/offline CSP, capability isolation and uninstall behavior.
- Preserve user installation data during application uninstall/update.
- Keep workflows commented out.
- Keep placeholder icons and existing license status.
- Record updater signing as unavailable without owner configuration; never substitute a generated development key into a release.
- Write README from verified implementation and the acceptance ledger. Document unresolved validation explicitly; documentation does not turn an unpassed requirement into completion.

## 3. Decisions and assumptions

| Question | Decision | Classification and evidence |
|---|---|---|
| **Q1 — Curated A or B?** | **B: explicit setup target, before FinalVerify/activation.** | AI-decidable. FR-SETUP-6 orders curated nodes before final verification; FR-SETUP-10/11 preserves the prior working environment. Current `StepId::ALL`, `final_verify()` and `activation::activate()` establish that boundary. A would activate before executing selected setup work. |
| **Q2 — Accelerator model/dependencies/constraints?** | Hashed direct wheels plus a package-component variant. ORT alone uses `no_deps=true`; FA3 uses normal constrained dependency resolution. Include the reference `transformers==5.5.0`, ONNX and NumPy/Pillow compatibility component. | AI-decidable. §5.4 explicitly permits package specs; FR-SETUP-8 includes these compatibility packages/constraints. The reference supplies evidence for ORT’s `--no-deps`, not FA3’s. |
| **Q3 — Unverified py312 wheels?** | Include as experimental, unchecked options with precise provenance. Do not call them validated or default-enable them. Native import/compatibility evidence is required before release claims. | AI-decidable. Supplied hashes establish identity; REQUIREMENTS §2.4 marks compatibility “to validate.” FA3’s module remains uncertain. Keep py313/314 lists empty as instructed. |
| **Q4 — Root switch/adopted/DB migration?** | Separate open-existing from create-new; restart after pointer publication. Detect relocation before reconciliation. Rebuild into a fresh owned env; preserve adopted files and external paths. Component-aware, idempotent migration only. | AI-decidable. Existing `switch_root` creates DBs/copies settings; current startup creates directories early. Persisted run/session paths can still point to the original root. FR-SETUP-5/12 and scenario 4 settle ownership and preservation. |
| **Q5 — HF relay timing/proxy source?** | Relay every HF attempt, unlimited or capped; one live shared limiter. Capture original proxy configuration before overriding the child. | AI-decidable. A cap-only relay misses already-running jobs when the cap is enabled. `process::run` clears inherited environment; explicit child configuration is necessary. Preserve Xet and upstream proxy behavior. |
| **Q6 — Plugin compatibility?** | Both exact pins are suitable candidates; integration must update and verify Cargo.lock. | AI-decidable. Published registry metadata for both declares Tauri `^2.12`, Rust 1.90 and tauri-plugin `^2.7.1`, compatible with this workspace’s direct pins. Updater uses reqwest `^0.13`; its published Windows ZIP dependency is `^4`, which may coexist with workspace ZIP 9. Neither plugin is currently locked, so successful resolution/build is **not yet proven**. [Updater registry metadata](https://index.crates.io/ta/ur/tauri-plugin-updater), [clipboard registry metadata](https://index.crates.io/ta/ur/tauri-plugin-clipboard-manager). |
| **Q7 — Other release gaps?** | Preserve M5 acceptance gates; close pin-baseline, recovery-startup, environment-info and HF-tool relocation gaps; execute outstanding platform/provider/security/performance acceptance. | AI-decidable. See the release checklist below. Existing M0–M4 evidence predates current changes; IMPLEMENTATION_STATUS is stale. |

Assumptions retained:

- Windows/Linux x64, NVIDIA-only release scope remains fixed.
- Current machine limitations are supplied facts, not newly verified hardware observations.
- CI stays disabled.
- Key custody, Authenticode, license and final artwork remain owner decisions already handled by documented inactive/placeholder behavior.
- No new source-build consent, remote profiles, node-set import/export, arbitrary installation switching or other P2 feature is added.

## 4. Risks

- **Relocation is the highest correctness risk.** Copied activation/run records must never mutate the original installation, even if it still exists.
- Curated nodes share real filesystem folders with the installation. Recovery, exact-source idempotence and candidate-scoped dependency outcomes must remain consistent with accepted M5.
- A wheel’s SHA-256 does not prove its ABI, CUDA or GPU-architecture suitability. FA3 and ORT need explicit native validation.
- HF/Xet proxy configuration must be demonstrated with direct egress denied; merely observing some relay traffic is insufficient.
- Windows updater installation can exit without ordinary Rust destructors running. Shutdown and flushing must precede installation.
- Native filesystem containment, trash behavior, process-tree cleanup and WebKitGTK behavior cannot be established by Windows GNU unit tests.
- No NVIDIA/Linux/MSVC environment is available here for the required native acceptance. Record these as open release gates.
- Third-party distribution notices and artifact redistribution terms require an audit; do not invent a project license.

## 5. Success checks

These are **instructions for the implementation owner later**, after coordinated build ownership. None was executed in this review.

After merging Rust integration, regenerate bindings explicitly:

```powershell
cargo test -p desktop --test bindings --locked -- --ignored export_bindings
```

Then run:

```powershell
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p desktop --test bindings --locked
cargo test -p desktop --test acl --locked
node --test scripts/check-arch-deps.test.mjs
node scripts/check-arch-deps.mjs

pnpm -C ui lint
pnpm -C ui typecheck
pnpm -C ui test
pnpm -C ui build

cargo build -p desktop --locked --features tauri/custom-protocol
```

Run every named regression above. Measure domain/application coverage against ≥90%/≥80%; do not rely on historical coverage.

`docs/RELEASE_ACCEPTANCE.md` must map **every P0/P1 requirement and all 17 scenarios** to passing evidence or an explicit open blocker. At minimum:

| Area | Required release evidence |
|---|---|
| Setup/environment | Clean default setup on both OSes; custom empty/adopted venv; failed/successful side-by-side replacement; disk/hardware gating; original protected pins preserved. |
| M6 setup | Selected accelerators with actual import outcomes; exact curated consent; node dependency failure cannot activate; copied-root rebuild with original root both present and absent. |
| Runtime | Headless detection, whole-tree termination, restart/rescan under one lease, dirty/diverged update refusal, embedded-window capability denial. |
| Providers | Real CivitAI integrity/resume/auth; HF gated/LFS/Xet/pause/fallback; Drive confirmation/quota; redaction across logs/DB/exports/diagnostics. |
| Downloads/library | Combined live bandwidth cap; cancellation while throttled; crash publication recovery; native locked-file and trash behavior; relative-root preservation. |
| M5 | Accepted backend/frontend checks and scenarios 10, 11, 14, 16 and 17. M6 must not substitute for those fixes. |
| Packaging/updater | Native install/launch/uninstall for all four bundle formats; invalid signature refusal; signed update when configured; inactive behavior otherwise. |
| UX/performance | Offline production CSP, both themes, Thai filenames, keyboard/focus, 200% zoom; cached 2,000-file scan <2 s; uncapped throughput ≥90% of comparison curl. |
| Architecture | Boundaries, binding drift and valid-body ACL checks; local checks substitute operationally while owner-disabled CI remains disabled. |

**M6 completion requires verified implementation; release readiness additionally requires the native/provider evidence.** README must distinguish those states.

## 6. User-only questions

None.