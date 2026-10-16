# M6 Release 1 plan (draft by main, for advisor review)

Scope: REQUIREMENTS §7 M6 + HANDOFF "M6 audit gap map": accelerators (FR-SETUP-8, NFR-SEC-7, §5.4), curated nodes (FR-SETUP-9), moved-install rebuild (FR-SETUP-12, scenario 4), app updater (FR-SET-4), global bandwidth cap (FR-DL-14), diagnostics (§6.7), packaging/release audit (§6.3, NFR-SEC-6). Then README.
Evidence: scratchpad `exp-d.out.md` (environment side), `exp-e.out.md` (platform side), `m6-reference-data.md` (curated list pinned to exact commits; accelerator wheels with publisher-provided SHA-256 for the py312 profile). M5 corrected plans (`adv-m5-backend.out.md`, `adv-m5-frontend.out.md`) are being implemented concurrently; M6-2/M6-3 depend on M5 WP5 (`NodeRequirementSource`) and M5 consent/publishing.

Environment facts: this machine is Windows 11 x64 with an **AMD GPU only (no NVIDIA)**, no MSVC C++ tools (GNU toolchain used for local verification), no Linux. Real CUDA setup, accelerator import verification, Linux packaging and native acceptance cannot be produced here; they must be documented as unverified, never claimed.

Fixed decisions (HANDOFF §3): updater key custody/Authenticode/license are owner decisions → updater inert without build-time configuration, placeholder icons, no license added. CI remains disabled by the user's commit `chore: disable ci (temp)` — do not re-enable.

## M6-1 Accelerators
- Domain `AcceleratorSpec` becomes a direct hashed wheel: `name`, `module` (import name for verification), `url` (HTTPS), `filename` (safe `.whl`), `sha256` (64 lowercase hex, validated), `size: Option<u64>`, `no_deps: bool`, `default_selected: bool`, `provenance: String`. Index/package-spec accelerators are not supported (NFR-SEC-7 pins direct wheels). Profile JSON `schema_version` 2 adds these fields; schema 1 still parses (empty accelerator list).
- Profiles: `win-py312-torch2100-cu130` gets the five reference wheels (SageAttention, llama-cpp-python, FlashAttention 2, FlashAttention 3, onnxruntime-gpu 1.26.0 CUDA 13 feed) with the recorded hashes/provenance; `no_deps` true for onnxruntime-gpu (reference used --no-deps) and FA3? (advisor decide per wheel); constraints `numpy<2.3`, `pillow<12` added to that profile (reference step 9). py313/py314 profiles keep `accelerators: []` (no verified wheels; HANDOFF).
- Application: new environment port `WheelFetcher { async fn fetch(&self, root, spec: &AcceleratorSpec, ctx) -> Result<PathBuf, ProvisionError> }` (infrastructure: shared `Downloader::fetch` with sha256 into `data/cache/wheels/<sha256>/<filename>`, reuse when hash matches). `InstallSpec` gains `no_deps: bool`; accelerators install the local wheel file with the constraint file (pin guard) and `allow_source_builds: false`; pins reasserted after each attempt incl. failure; each failure is a non-fatal warning (BestEffort step).
- Selection: `SetupRequest.accelerators: Vec<String>` (names; default = profile's `default_selected`), validated against the profile; persisted in run JSON and in `install.json`/env record as selected + per-accelerator result (`installed`/`failed`/`skipped`). Final verification reports presence for the selected modules (plus existing sageattention/flash_attn/llama_cpp), and the env info query/DTO (FR-ENV-1) lists accelerators with results.
- UI: setup wizard optional-components step lists accelerators (name, module, host, size, checkbox default per profile); review step summarizes; Environment page shows accelerator results. Remove the "deferred to M6" alert text.
- Tests: domain validation (bad hash/url/filename), profile parse schema 1/2, bundled profile test (py312 has 5 hashed wheels, others empty), application accelerator step (hash fetch failure → warning; install failure → warning + pins reasserted; unselected skipped; no_deps passed), infra wheel fetcher (hash mismatch rejected, cache reuse), wizard tests.

## M6-2 Curated nodes (after M5)
- Bundled `crates/infrastructure/src/environment/data/curated_nodes.json`: 22 ordered entries {name, repository, branch, commit, provenance} from `m6-reference-data.md`. Port `CuratedNodeCatalog::list()`; IPC `setup_list_curated_nodes`.
- Wizard: "Recommended custom nodes" list with exact `repository@commit` (short) per row, all checked by default (FR-SETUP-9 default list), plus a required explicit consent checkbox (NFR-SEC-3 text: third-party code, venv is not a sandbox) when any are selected. install.py is never run during setup (no per-node hook consent in the wizard); nodes with install.py get a warning telling the user to use Retry dependencies with the script option in Custom Nodes.
- `SetupRequest.curated_nodes: Vec<{repository, commit}>` + `curated_consent: bool`; persisted in the run.
- Execution design choice (advisor decide):
  (A, main prefers) Move `CuratedNodes` to run **after** FinalVerify/activation inside the same held setup lease, so the target env is the freshly activated env and ordinary M5 node semantics apply unchanged (prepare_held Git@exact commit → verify resolved commit equals the consented commit → commit_held with execute consent, no hook → deps under pin guard; failure → node deps failed + env `RepairRequired`, shown as a setup warning). No new "setup target" concept needed; no silent use of the old env because activation already happened.
  (B, HANDOFF suggestion) Explicit validated `NodeTarget::Setup{root, env}` bound into prepared plans and commit checks, running before FinalVerify.
- Re-setup modes: offer curated nodes only in first-run (and Rebuild? no).
- Tests: catalog parse (22 entries, exact 40-hex commits, https github), wizard consent gating, execution (commit mismatch skipped with warning; dep failure → RepairRequired; unselected skipped; never runs hook).

## M6-3 Moved-install rebuild (after M5 WP5)
- Startup: if the bootstrap root does not exist, do **not** create directories; setup status phase `root_missing` with the recorded path; UI offers "Locate installation…" (folder picker) or "Set up a new installation".
- New IPC `setup_open_install(path)`: validates `<path>/install.json` (+ `data/app.db`), switches the bootstrap pointer, and restarts the app (tauri `AppHandle::restart`) so composition uses the new root. Allowed only when ComfyUI is stopped and no maintenance op runs.
- Detection (setup status query): `manifest.install_root` != current root (normalized/canonical), or the active app-owned venv is broken: `pyvenv.cfg` `home` missing / not inside `<root>/data/...uv python dir` for managed envs, or a bounded interpreter probe (`python -I -c "import sys"`, 10 s timeout) fails → phase `rebuild_required { reason: moved | interpreter_broken }`.
- Guided rebuild = new `SetupMode::Rebuild`: side-by-side new env under the current root with the recorded profile id/revision and recorded component selections (manager, accelerators), reuse the existing ComfyUI checkout (verify, no re-clone), reinstall node requirement sets via M5 `NodeRequirementSource` (enabled + disabled, no hooks, failure fatal per M5 rule), re-provision tools incl. the hf tool venv, then activation writes `install.json` with the new root. Path migration: app-owned env records under the old root are marked unusable (never deleted automatically; listed for user deletion), runtime session JSON with stale absolute paths is cleared, run records keep history. Adopted/external venvs that still verify are kept; if broken → rebuild into a new app-owned env (never delete adopted).
- UI: App.tsx recovery/rebuild screen replaces the "deferred to M6" text; wizard runs mode `rebuild` with a summary of what is kept (models, user/, input/, output/, custom nodes).
- Tests: status detection (moved root, broken interpreter, healthy), open-install validation (no install.json → refusal), rebuild run (reuses checkout, installs node sets, activation writes new root, old env marked), startup missing root does not create dirs.

## M6-4 Global bandwidth cap (FR-DL-14)
- Infrastructure `acquisition/bandwidth.rs`: one shared token bucket `BandwidthLimiter` (bytes/s, `set_limit(Option<NonZeroU32 KiB/s>)` live, fair across tasks, `async fn take(n)`), injected into `NativeDownloader` (await before each chunk write) and `HfDownloader`.
- hf engine: when a cap is set, an allow-listed loopback CONNECT relay (127.0.0.1:0) throttled by the same limiter; child gets `HTTPS_PROXY`/`HTTP_PROXY=http://127.0.0.1:<port>` and `NO_PROXY=` cleared; relay accepts only `CONNECT <host>:443` for HF hosts (`huggingface.co`, `*.huggingface.co`, `*.hf.co`, xet bridge hosts), tunnels opaque TLS, chains to the user's upstream proxy when the app process has `HTTPS_PROXY` set, rejects everything else. Without a cap: unchanged (no relay).
- `settings_set` updates the limiter live; Settings UI enables the field (KiB/s or MiB/s input, empty = unlimited).
- Tests: limiter rate accuracy with a fake clock, live change, native transfer throttled (local test server), relay allow-list/deny, host:443 validation, upstream chaining, env vars set only for the child.

## M6-5 Updater (FR-SET-4)
- Dependency `tauri-plugin-updater = "=2.14.0"` (workspace). Build-time `option_env!("VJ_UPDATER_PUBKEY")` and `option_env!("VJ_UPDATER_ENDPOINT")`; the plugin is registered and commands enabled only when both are present; otherwise updater is inert and the UI hides it.
- Commands (Rust-only updater access): `app_update_status` → {configured, currentVersion, checkOnLaunch}, `app_update_check` → {available, version, date, notes}, `app_update_install(onEvent: Channel)` → requires ComfyUI stopped and acquires the maintenance gate; streams download progress; installs; restarts.
- Setting `check_updates_on_launch` (domain/app/sqlite/settings DTO/UI) default **false**; when true and configured, check once after startup and notify the UI (no auto-install).
- Extend IPC bindings/ACL/capability/gateway; Settings "Updates" section.
- Tests: config absent → status configured=false and check/install refuse with `updater_not_configured`; install refused while ComfyUI runs/maintenance busy; settings round-trip.

## M6-6 Diagnostics (§6.7)
- Application `CollectDiagnostics` use case: app version, OS, GPU/driver/CUDA (hardware probe), active env (python, torch/cuda, triton, accelerators), manifest profile id/revision + install root (redacted to `<root>`?), uv/hf pinned versions, ComfyUI revision/status, last 200 app-log lines (new port `AppLogTail`), runtime log tail. Desktop presenter renders text; the **entire** output passes the infrastructure redactor.
- Commands `app_diagnostics` → string; `app_copy_diagnostics` → copies via `tauri-plugin-clipboard-manager = "=2.5.0"` from Rust. UI: Settings "Copy diagnostics" + ComfyUI page button use the gateway; on failure show a read-only textarea with the text.
- Tests: redaction of tokens/signed URLs in every section, 200-line cap, missing env handled, ACL.

## M6-7 Packaging / release audit
- Build the real UI then `cargo build -p desktop --locked --features tauri/custom-protocol` (GNU), attempt `tauri build` NSIS/MSI on Windows via `scripts/tauri.mjs`; record artifacts or blockers (MSVC absent). Launch smoke: app starts, single-instance focus, ComfyUI window has no IPC (ACL tests). Linux AppImage/deb not buildable here unless WSL/Docker is available — check, otherwise document as unverified.
- Review `.github/workflows/release.yml` content for correctness against the updater build-time env names; keep it commented out (CI disabled by owner).

## Ordering / parallelism
M6-4, M6-5+M6-6, M6-1 are independent of M5 and can run in separate git worktrees (patch-merged, no commits). M6-2 and M6-3 after M5 WP5. Then M6-7, full verification, docs (REQUIREMENTS §9/IMPLEMENTATION_STATUS/HANDOFF) and README.

## Questions for advisor
Q1 Curated execution: option A (after activation, ordinary node semantics) vs B (explicit setup target).
Q2 Accelerator data model: direct-wheel-only OK? `no_deps` per wheel; py312 constraints `numpy<2.3`, `pillow<12`; include transformers==5.5.0 pin or not.
Q3 Including py312 accelerators whose import was never verified by this app (hashes are publisher-provided): acceptable with provenance "not import-verified", or keep empty?
Q4 Moved install: open-existing-install via bootstrap pointer switch + app restart; handling of adopted venvs; DB path migration scope.
Q5 Bandwidth: hf relay only when a cap is set; upstream proxy chaining source.
Q6 Updater/diagnostics plugin versions (=2.14.0 updater, =2.5.0 clipboard) — verify they are compatible with tauri 2.12.4 in Cargo.lock terms (read-only reasoning; no network if not possible).
Q7 Anything missing for release-1 P0/P1 acceptance that M6 must also cover (audit REQUIREMENTS P0/P1 vs current code at a high level).
