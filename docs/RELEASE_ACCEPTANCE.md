# Release 1 acceptance ledger

This ledger maps every P0 and P1 requirement, every non-functional clause and all 17 acceptance scenarios of [REQUIREMENTS.md](REQUIREMENTS.md) to evidence or to an explicit open blocker. Its structure follows the advisor ruling in [s5-advisor-r.md](handoff/evidence/s5-advisor-r.md) (Q5).

**Release verdict: NO-GO.** Linux compiles, passes its automated tests (S7-NATIVE-LINUX-L1) and ran as a GUI app in a GPU container (first-run setup, ComfyUI on CUDA, a Google Drive download, `.deb` and AppImage; [s7-native-register.md](handoff/evidence/s7-native-register.md)) but never on a real Linux desktop with a native NVIDIA driver, the Windows installers have not been executed, the provider-token scenarios (CivitAI, gated Hugging Face, Google Drive quota) are open, Authenticode signing and updater key custody are unresolved, and further Windows native scenarios are open. See [Release gates](#release-gates).

## How to read this ledger

| Status | Meaning |
|---|---|
| `PASS` | The clause has the required automated evidence and, where it needs one, a native run on every platform in scope, with no open gap. |
| `PARTIAL` | Implemented, with automated evidence and/or native evidence for some clauses, platforms or profiles. The remaining gap is named. |
| `FAIL` | Evidence shows the clause does not hold. |
| `OPEN` | The acceptance evidence the clause needs does not exist yet. The implementation may exist. |
| `ACCEPTED RESIDUAL` | A known gap that an approved ruling accepts for Release 1. It is never a pass. |
| `OWNER DECISION` | The outcome depends on a decision reserved to the owner. |

"Automated" means a default-discovered Rust or Vitest test that ran green in the latest check run ([S7-AUTO-WIN-003](#automated-and-packaging-evidence), 813 Rust tests on Windows, 789 on Linux, 202 Vitest tests). "Native" means a run of the real application or a real external tool on the development machine, recorded in [s7-native-register.md](handoff/evidence/s7-native-register.md) (Windows W1–W10, Linux container L1–L8), [s6-native-register.md](handoff/evidence/s6-native-register.md) (N1–N29) or [s5-native-register.md](handoff/evidence/s5-native-register.md). Linux runs so far happened in a Docker container with the GPU passed through by Docker Desktop (WSL2); they narrow the Linux gaps but do not replace a Linux desktop. Release 1 covers Windows x64 **and** Linux x64, so a requirement with Windows evidence only is at most `PARTIAL`.

## Snapshot

| Item | Value |
|---|---|
| Date | 2026-10-20 (times are +07:00); last updated after session 7 (~20:00) |
| Source | **Session 7:** commit `0502a5e` on `feat/m0-skeleton` (pushed) = `7d7b8f3` + package G7 (Google Drive through gdown 6.4.2) + session-7 evidence and tooling; every S7 row was produced from this code. **Session 6 (rows S6-…):** HEAD `e384d43` plus uncommitted verified session-6 work: cumulative checkpoint `s6-14-final-code-verified.patch`, SHA-256 `5a7861881fce447c35cf78280137f44b6163e3237516709940e511aea8b36794` (unchanged); its code is identical to `s6-13-through-n-verified.patch`, the two differ only in two evidence files. The work was committed as `8dc99b1` (code = this checkpoint plus the Linux test-fixture fix in `crates/infrastructure/src/environment/tools/tests.rs`); the checkpoint files were later removed from the tree — read them with `git show 8dc99b1:docs/handoff/patches/<name>.patch`. |
| Release artifacts | **Session 7** (code of `0502a5e`, [S7-PKG-WIN-001](#automated-and-packaging-evidence)): NSIS `vjumpkung-comfyui-desktop_0.1.0_x64-setup.exe` 9,343,473 B, SHA-256 `7d16c8a1989d3c57583d78b89ad76c0b4831c09bac631614e629e6966e48f4ac`; MSI `vjumpkung-comfyui-desktop_0.1.0_x64_en-US.msi` 13,336,576 B, SHA-256 `dc34e2f69def941ce1f9e693d85ca22ed6463275144215e35c21c6d30d97c40a`; release exe SHA-256 `7199abb1b80d101a55af9d884ecad57d197af5871ff151cc600f98eff69e09de` (all `NotSigned`). Linux ([S7-PKG-LINUX-001](#automated-and-packaging-evidence)): `vjumpkung-comfyui-desktop_0.1.0_amd64.deb` 16,610,902 B, SHA-256 `14a037d7642c8c836cab4247ece369d8ddbd4afd0d1d6e91b59dda454d8aa41f`; `vjumpkung-comfyui-desktop_0.1.0_amd64.AppImage` 120,621,560 B, SHA-256 `0ead974dc6593d55642e244bf074b2bfb8c6ccff4d3d0b9d66e9261b9f872b09`. **Session 6:** built by `pnpm -C ui tauri build` from the checkpoint above (see [S6-PKG-WIN-001](#automated-and-packaging-evidence)). NSIS `vjumpkung-comfyui-desktop_0.1.0_x64-setup.exe` 9,336,453 B, SHA-256 `d2f328a438bf3b83edf5dab09c4f7f35305d5ab0353235770c86c16e4c800308`. MSI `vjumpkung-comfyui-desktop_0.1.0_x64_en-US.msi` 13,340,672 B, SHA-256 `51005095abcbfe20f5842591cfb50a383e1cea507a7523cd9e80cc96ea6da299`. Release exe `vjumpkung-comfyui-desktop.exe`, SHA-256 `c49e79323fe6fe9df46e5b41a0648d595304c17aef5b7c552320be011dc3ab56`. All three `NotSigned`. |
| Native acceptance binaries | Session 7: debug + custom-protocol `s7-01` (SHA-256 `da16e0d75be362a7ce17be1af7f742e691d7bae07880860b1c7bcd69011f09da`) for W1–W10; the Linux runs used the release `.deb`/AppImage above. Session 6: debug builds with `--features tauri/custom-protocol` (no dev server), frozen under `D:\vjc-s6\bin\` on the development machine. Truncated SHA-256 values as recorded in the native register: `s6-01` `2ab32a47…`, `s6-02` `6eb4c7cd…`, `s6-04` `910381b7…`, `s6-06` `669ecb97…`, `s6-07` `eef6b0cd…`, `s6-08` `8439a631…`, `s6-09` `2e2b5efa…`, `s6-10` `56aa7c3a…`, `s6-11` `8426e5d9…`, `s6-13` `0d75e2bd…` (code = final checkpoint). Session 5: `S5-BIN-02` `a642d3eb58376753580b6b050f2cbb1c2540e98edaa12235e167b7489d4342d0`. |
| Machine | Windows 11 Education 10.0.26300 x64; NVIDIA GeForce RTX 3060 12 GiB, driver 610.88, CUDA 13.3; 32 GB RAM; 12 logical CPUs. |
| Toolchain | Rust `1.99.0-x86_64-pc-windows-msvc` (VS Build Tools 2022), Node 24.13.0, pnpm 10.24.0, git 2.55.0.windows.3. Pinned tools used natively: uv 0.13.1, hf 2.2.0, gdown 6.4.2. Linux: Docker Desktop 29.9.1 (WSL2, NVIDIA container runtime), image `rust:1.99.0-trixie` (Debian 13, WebKitGTK 4.1, git 2.47), `tauri-driver` 2.x + WebKitWebDriver under Xvfb. |
| Configuration | Native runs used the debug-only root override `VJUMPKUNG_COMFYUI_HOME`, set for the child process only, on disposable roots `D:\vjc-s6\r1`, `r1-copy`, `r2`, `r3` and `D:\vjc-s7\r1` (fresh install roots on an existing development machine, not a clean machine; all removed after the runs). Linux runs used the default root `/root/.local/share/vjumpkung-comfyui` in a throw-away container volume. The owner's root `D:\comfyui-test\v1` was only read and started (N1–N7). Updater inactive (no `VJUMP_UPDATER_PUBKEY` / `VJUMP_UPDATER_ENDPOINT` at build time). Placeholder icons. No license. |
| Isolation | No provider token was used in any run; all Hugging Face runs used public files. A new root does not isolate credentials or WebView2 storage, and the single-instance identifier is shared with the production build ([s5-advisor-r.md](handoff/evidence/s5-advisor-r.md) Q2). |
| Limitations | Debug/custom-protocol evidence does not establish release-binary, installer, production-bootstrap or clean-machine acceptance. The Windows release exe and installers were not launched. Linux evidence comes from a container (no desktop session, window manager, Secret Service or native NVIDIA driver). |

## Requirements

FR-NODE-6 is split into its P0 operations and the P1 "Update all" batch. §6.2–§6.4 and §6.7 have no IDs in REQUIREMENTS; the ledger names their bullets `NFR-REL-n`, `NFR-PLAT-…`, `NFR-PERF-n` and `NFR-OBS-n` in document order. P2 items (FR-ENV-5, FR-MODEL-6, FR-NODE-11) are out of scope and not listed.

Gate IDs refer to [Release gates](#release-gates).

### Setup and environment

| ID | Pri | Clause / platform / profile | Automated evidence | Native evidence | Status | Remaining gap | Gate | Owner / next action |
|---|---|---|---|---|---|---|---|---|
| FR-SETUP-1 | P0 | Wizard on first launch; Win + Linux | `first_run_executes_the_steps_in_order_and_activates`; wizard Vitest | N8 (root holds only `data` before setup), N9, N10, N25, N26; S7-NATIVE-WIN-W1; S7-NATIVE-LINUX-L3, L4 (Linux wizard and first run, container) | PARTIAL | Linux desktop | G01 | Engineering: Linux run |
| FR-SETUP-2 | P0 | Hardware check, disabled profiles with reason | `every_gpu_must_be_compatible`, `wrong_os_and_disk_shortfalls_are_reported_per_volume`, wizard reason test | N9, N25 (eligible profiles on RTX 3060) | PARTIAL | Disabled-profile reasons on real unsupported hardware (S13); Linux | G01, G05 | Owner: unsupported-hardware machine |
| FR-SETUP-3 | P0 | Managed Python, custom venv, custom interpreter | `an_empty_custom_folder_creates_an_app_owned_venv_there_with_the_managed_python`, `an_existing_venv_is_adopted_after_explicit_confirmation_and_never_recreated`, `a_custom_interpreter_creates_a_new_app_owned_venv_under_envs` | Managed Python: N11, N28 | PARTIAL | Custom venv and interpreter natively (S02); Linux | G01, G05 | Engineering: S02 run |
| FR-SETUP-4 | P0 | Custom Python validation | `msvc_runtime_failure_blocks_setup_on_windows`, `missing_headers_block_only_triton_and_are_judged_per_os`; ignored `a_real_interpreter_is_probed` not executed | None | PARTIAL | Native validation; Linux | G01, G05 | Engineering |
| FR-SETUP-5 | P0 | App-owned vs adopted | `adopted_environments_are_never_deleted_and_need_rebuild_confirmation`, `adopted_environments_are_never_removed_by_the_adapter` | None | PARTIAL | Native S02b | G05 | Engineering |
| FR-SETUP-6 | P0 | Install steps; Windows profile `win-py312-torch2100-cu130` | `first_run_executes_the_steps_in_order_and_activates`, `uv_is_downloaded_verified_extracted_and_published_once` | N11 (full first run, ~8.5 min), N28, S7-NATIVE-WIN-W1; S7-NATIVE-LINUX-L4 (`linux-py313-torch2141-cu130`, container); historical 2026-10-16 direct run with `win-py313-torch2141-cu130` | PARTIAL | Linux profile on a Linux desktop | G01 | Engineering |
| FR-SETUP-7 | P0 | Torch pin guard | `installs_after_torch_use_the_constraints_file`, `a_pin_change_during_requirements_fails_the_step_and_the_new_environment` | N29 (pins unchanged after a node install with `install.py` and requirements), N11 final verify | PARTIAL | Linux | G01 | Engineering |
| FR-SETUP-8 | P1 | Accelerators, py312 profile revision 2 (five experimental wheels + compatibility packages, unchecked) | `accelerators_are_best_effort_when_a_profile_defines_them`, `wheel_hash_failure_never_runs_uv`, `accelerator_failure_reasserts_original_pins` | None (all native runs left accelerators unselected) | PARTIAL | Real wheel install and import (residual R19); FlashAttention 3 module name unverified | G08 | Engineering: native accelerator run |
| FR-SETUP-9 | P1 | Curated nodes: 22 selectable, unticked, latest default-branch commit, consented exceptions | `curated_auto_consent_installs_every_node_and_logs_warnings`, `setup_content_policy_requires_persisted_consent_and_selected_curated_source`, `bundled_catalog_has_no_unavailable_entries`, `plaguekind_is_selectable_and_sent_with_review_consent` | N10–N13 (21 curated + Manager, all import on cuda:0), N26 (unticked default), N28 (PlagueKind with acknowledged `__pycache__` warning) | PARTIAL | Linux | G01 | Engineering |
| FR-SETUP-10 | P0 | Re-setup modes, side-by-side activation | `resetup_modes_follow_the_documented_rules`, `repair_builds_a_replacement_when_verification_fails`, `replacement_node_dependency_failure_never_activates` | None | PARTIAL | Native S03; Linux | G01, G05 | Engineering: S03 run |
| FR-SETUP-11 | P0 | Failure handling, Retry/Skip/Abort | `a_required_failure_keeps_the_previous_environment_and_retry_resumes_the_same_run`, `a_failed_optional_step_can_be_skipped_and_required_ones_cannot` | S5-NATIVE-WIN-N1-001 (failed step shown), N19 (failed rebuild), N20 (Retry same run succeeds) | PARTIAL | Linux | G01 | Engineering |
| FR-SETUP-12 | P1 | Moved install, guided rebuild | `relocation_is_detected_with_the_original_root_present_or_vanished`, `moved_rebuild_preserves_files_and_launch_options`, `copied_minor_links_of_other_versions_are_removed_too` | S5-NATIVE-WIN-N2a-001 (detected, original preserved), N19, N20 (rebuild succeeds, ComfyUI on cuda:0) | PARTIAL | Original-root-absent variant; Linux symlink branch; residual R21 | G01, G05, G08 | Engineering |
| FR-SETUP-13 | P1 | Disk preflight | `disk_budget_depends_on_volume_layout`, `wrong_os_and_disk_shortfalls_are_reported_per_volume` | None | PARTIAL | Only `win-py313-torch2141-cu130` is calibrated; py312 estimate `calibrated: false` | G05 | Engineering: measure |
| FR-ENV-1 | P0 | Active environment view | `environment_info_and_packages_describe_the_selected_environment`; Environment page Vitest | Not recorded | PARTIAL | Native view; Linux | G01, G05 | Engineering |
| FR-ENV-2 | P1 | Package list with search | Same as FR-ENV-1 | Not recorded | PARTIAL | Native; Linux | G01, G05 | Engineering |
| FR-ENV-3 | P1 | Package install/upgrade/uninstall with pin confirmation | `a_pin_conflict_is_previewed_and_applied_only_with_the_exact_confirmation`, `mutations_wait_for_the_gate_and_a_stopped_runtime` | Not recorded | PARTIAL | Native; Linux | G01, G05 | Engineering |
| FR-ENV-4 | P0 | Serialized, ComfyUI stopped | `the_gate_is_exclusive_until_the_lease_is_dropped`, `node_operations_need_a_stopped_runtime_the_gate_and_a_verified_environment` | Indirect: N7, N29 | PARTIAL | Native race (S16a); Linux | G01, G05 | Engineering |

### ComfyUI runtime

| ID | Pri | Clause / platform / profile | Automated evidence | Native evidence | Status | Remaining gap | Gate | Owner / next action |
|---|---|---|---|---|---|---|---|---|
| FR-COMFY-1 | P0 | Start/stop/restart, readiness, port conflict | `ready_owns_listener_releases_gate_and_stop_is_idempotent`, `detects_conflicts_without_wrapping`; port-conflict Vitest | N5, N13, N20, N21, N28 (`/system_stats` CUDA on cuda:0), S7-NATIVE-WIN-W9; S7-NATIVE-LINUX-L6 (torch 2.14.1+cu130 on cuda:0, container) | PARTIAL | Port conflict natively; Linux desktop | G01, G05 | Engineering |
| FR-COMFY-2 | P0 | Logs, rotation, xterm | `fragmented_unicode_secrets_and_carriage_returns`, `ring_is_bounded_and_oversized_records_do_not_leak` | N5 (no `pwd` traceback), S5-NATIVE-WIN-N1-003 (xterm frame fits in WebView2), S7-NATIVE-LINUX-L7 (xterm log renders in WebKitGTK under Xvfb) | PARTIAL | Linux desktop | G01 | Engineering |
| FR-COMFY-3 | P0 | Embedded window without IPC; system browser | `only_the_current_comfyui_origin_is_embedded`, ACL `comfyui_window_with_local_origin_is_denied` | S5-NATIVE-WIN-N1-003 (embedded window loads the frontend), S7-NATIVE-LINUX-L7 (same in WebKitGTK, container) | PARTIAL | Native invoke denial is policy-tested only; WebKitGTK canvas performance on a desktop | G01 | Engineering |
| FR-COMFY-4 | P0 | Close policy, whole-tree termination | `shared_runner_bridge_preserves_raw_output_and_owns_child_and_grandchild`, `idle_runtime_exits_and_active_runtime_follows_the_setting` | S5-NATIVE-WIN-N1-003 (Stop frees the port, no python left), N5, N7 | PARTIAL | Headless keep-running natively; Linux process group | G01, G05 | Engineering |
| FR-COMFY-5 | P1 | Launch options | `argv_is_controlled_and_keeps_tokens`, `restart_acquires_once_and_saves_options_apply_only_on_next_start` | Not recorded | PARTIAL | Native; Linux | G01, G05 | Engineering |
| FR-COMFY-6 | P0 | Update: tag, latest commit, fast-forward; branch tracking; dirty/diverged refusal | `dirty_or_diverged_updates_change_neither_source_nor_packages`, `latest_commit_update_records_the_actual_reference`, `checkout_placement_attaches_only_when_nothing_local_is_lost`, `checkout_placement_derives_release_lines` | N12, N14 (finding, fixed G2c), N18, N21, N22 | PARTIAL | Dirty/diverged refusal natively; Linux | G01, G05 | Engineering |
| FR-COMFY-7 | P1 | Version picker, rollback | `tags_resolve_to_the_peeled_commit`, `branches_and_the_default_head_resolve` | N18, N21, N22 | PARTIAL | Linux | G01 | Engineering |
| FR-COMFY-8 | P1 | `extra_model_paths.yaml` read and generator | `generate_writes_a_managed_section_and_creates_missing_folders`, `preserves_loader_order_default_and_multiline_paths` | N15, N16, N17 (Thai and space in base path) | PARTIAL | Linux | G01 | Engineering |

### Downloads

| ID | Pri | Clause / platform / profile | Automated evidence | Native evidence | Status | Remaining gap | Gate | Owner / next action |
|---|---|---|---|---|---|---|---|---|
| FR-DL-1 | P0 | Add download, provider detection | `parses_sources_before_url_normalization`, `drive_urls_follow_gdown_fuzzy_forms`; enqueue Vitest | N24, S7-NATIVE-WIN-W2, W6, W7; S7-NATIVE-LINUX-L5 | PARTIAL | CivitAI natively; Linux desktop | G01, G03 | Engineering |
| FR-DL-2 | P0 | CivitAI | `publication_time_not_array_order_and_explicit_selection_is_strict`; parser tests | None | OPEN | Live CivitAI with and without token (S05) | G03 | Owner: test token |
| FR-DL-3 | P0 | Hugging Face URLs, repo browser, gated errors, integrity | `never_uses_redirect_length_or_xet_hash_as_sha`; source tests | N24 (`/blob/` URL, checksum published, verified) | PARTIAL | Gated/private/missing errors, repo multi-select natively | G03, G05 | Owner: gated-repo token |
| FR-DL-3a | P0 | `hf` CLI engine | `hf_proxy_configuration_changes_only_child_environment`; restart-from-0 Vitest | N23 (live engine test without profile variables), N24 | PARTIAL | Pause/restart-from-0 and token-in-child natively | G03, G05 | Engineering |
| FR-DL-3b | P0 | Native HF engine and fallback | `resumes_from_actual_length_with_strong_if_range_and_restarts_on_200`, `supervisor_limits_admissions_and_only_falls_back_on_spawn_failure` | None | PARTIAL | Missing-CLI fallback natively (S06f) | G05 | Engineering |
| FR-DL-4 | P0 | Google Drive (gdown 6.4.2 engine; native fallback) | `drive_urls_follow_gdown_fuzzy_forms`, `gdown_command_isolates_python_and_home`, `gdown_command_requires_the_provisioned_tool`, `driver_lines_parse_and_errors_map`, `driver_never_persists_cookies`, `relay_drive_scope_allows_only_google_hosts`, `moved_gdown_marker_cannot_skip_tool_recreation`, `repair_installs_missing_download_tools`; `confirmation_is_bounded_and_allowlisted` (native fallback); ignored `real_gdown_resolves_and_downloads_through_the_relay` executed (pass) | S7-NATIVE-WIN-W2…W8, S7-NATIVE-LINUX-L5 | PARTIAL | Over-quota page (S07b) | G03 | Owner: fixture |
| FR-DL-5 | P0 | Direct HTTPS, `http://` confirmation | `http_confirmation_happens_before_contact_and_direct_auth_is_never_forwarded`, `disposition_handles_quotes_and_encodings` | None | PARTIAL | Native | G05 | Engineering |
| FR-DL-6 | P0 | Persistent queue, states | `enqueue_is_atomic_for_identity_and_destination_and_rejects_stale_worker`, `startup_preserves_paused_native_partial_and_removes_only_owned_orphans` | N24 (job COMPLETED VERIFIED) | PARTIAL | Restart/crash natively (S14) | G05 | Engineering |
| FR-DL-7 | P0 | Progress at ≤ 4 Hz | `subscriptions_snapshot_atomically_drop_stale_progress_and_signal_overflow`, `progress_lines_become_exact_samples` | N24, S7-NATIVE-WIN-W3 (exact gdown total); S7-NATIVE-LINUX-L5 | PARTIAL | Linux desktop | G01 | Engineering |
| FR-DL-8 | P0 | Cancel, pause, resume | Range-resume and startup-pause tests, `tool_engines_restart_from_zero_and_native_does_not` | S7-NATIVE-WIN-W4 (gdown: pause stops the child tree and removes staging; resume restarts from 0 with the warning) | PARTIAL | Native-engine Range resume and cancel natively | G05 | Engineering |
| FR-DL-9 | P0 | Atomic, durable finalization | `durable_publish_recovers_rename_before_sql_update_and_catalog_is_idempotent`, `hash_mismatch_has_no_model_and_late_conflict_keeps_both_files` | N24 (published to `models\vae`, SHA-256 verified), S7-NATIVE-WIN-W5; S7-NATIVE-LINUX-L5 (identical SHA-256 on both) | PARTIAL | Linux desktop | G01 | Engineering |
| FR-DL-10 | P0 | Dedupe | `enqueue_is_atomic_for_identity_and_destination_and_rejects_stale_worker` | None | PARTIAL | Native | G05 | Engineering |
| FR-DL-11 | P0 | Name conflicts | `overwrite_needs_the_observed_identity_and_a_changed_file_raises_a_fresh_prompt` | None | PARTIAL | Native | G05 | Engineering |
| FR-DL-12 | P0 | Retries | `retry_deadline_survives_storage_and_is_not_claimed_early` | None | PARTIAL | Native | G05 | Engineering |
| FR-DL-13 | P0 | `resource_list` import/export | Pack preview Vitest (mocked gateway); no Rust pack parser test | None | PARTIAL | Rust parser test; native import | G05 | Engineering |
| FR-DL-14 | P1 | Bandwidth cap incl. HF relay | `shared_bucket_bounds_combined_native_and_relay_bytes`, `live_cap_changes_affect_already_running_hf_and_native_jobs`; `bandwidth_empty_means_unlimited_and_zero_is_invalid` | None | PARTIAL | Native combined cap | G05 | Engineering |
| FR-DL-15 | P1 | Xet via `hf`; native engine Xet error | Ignored `real_hf_xet_obeys_relay_with_direct_egress_denied` not executed | N23, N24 (Xet-backed file) | PARTIAL | Native engine Xet behaviour; relay egress test | G05 | Engineering |

### Model library

| ID | Pri | Clause / platform / profile | Automated evidence | Native evidence | Status | Remaining gap | Gate | Owner / next action |
|---|---|---|---|---|---|---|---|---|
| FR-MODEL-1 | P0 | Scan incl. extra roots | `scanning_catalogs_models_hides_sidecars_and_a_cached_rescan_changes_nothing`, `deferred_library_roots_register_comfyui_models_once_it_exists` | N16 (extra-root file listed), S5-NATIVE-WIN-N6-001 | PARTIAL | Linux | G01 | Engineering |
| FR-MODEL-2 | P0 | Rename, case-only, locked | `rename_moves_real_sidecars_updates_the_catalog_and_survives_a_rescan`, `a_case_only_rename_changes_the_name_on_a_case_insensitive_volume` | S5-NATIVE-WIN-N6-001 (Thai name while running), S5-NATIVE-WIN-N9-001 (controlled lock) | PARTIAL | Lock held by a loaded model (S08a); Linux | G01, G05 | Engineering |
| FR-MODEL-3 | P0 | Delete to Recycle Bin | `trash_failure_never_unlinks_and_needs_a_separate_permanent_confirmation`, `model_trash_moves_a_canonical_root_file_to_the_recycle_bin` | S5-NATIVE-WIN-N9-001 | PARTIAL | Non-recyclable volume cases (R11); Linux trash | G01, G05 | Engineering |
| FR-MODEL-4 | P1 | Move | `move_creates_folders_and_a_download_reservation_blocks_the_destination` | None | PARTIAL | Cross-volume natively | G05 | Engineering |
| FR-MODEL-5 | P1 | Identify on CivitAI | `identification_writes_sidecars_and_a_second_run_uses_the_hash_cache` | None | PARTIAL | Live lookup | G03 | Engineering |

### Custom nodes

| ID | Pri | Clause / platform / profile | Automated evidence | Native evidence | Status | Remaining gap | Gate | Owner / next action |
|---|---|---|---|---|---|---|---|---|
| FR-NODE-1 | P0 | Registry browse, cache | `browse_search_pagination_and_cache_keys` | Not recorded | PARTIAL | Native | G05 | Engineering |
| FR-NODE-2 | P0 | Registry install | `a_registry_release_is_staged_published_and_recorded_with_provenance`, `malicious_archives_write_nothing_outside_owned_staging` | None | PARTIAL | Native S10 | G05 | Engineering |
| FR-NODE-3 | P0 | Git install | `git_install_resolves_one_immutable_commit_and_tracks_a_branch`, `git_install_refuses_root_native_image_before_checkout` | N29 (Install from Git, `## main...origin/main`), N11 | PARTIAL | Linux | G01 | Engineering |
| FR-NODE-4 | P0 | Dependencies: approved `install.py` before reviewed requirements, pins, failure state | `install_py_runs_only_with_consent_before_requirements_and_trust_is_namespaced`, `hook_failure_skips_requirements_and_retains_repair_state`, `requirements_after_hook_use_reviewed_snapshot_and_frozen_versions`, `a_dependency_failure_marks_the_node_and_the_environment_and_keeps_the_backup` | N29 (order logged, pins unchanged) | PARTIAL | Failure and Repair natively (S17); Linux | G01, G05 | Engineering |
| FR-NODE-5 | P0 | List installed | `scanning_reads_files_only_and_classifies_each_provenance` | N13, N29 | PARTIAL | Setup-installed ComfyUI-Manager shows `DEPS NOT RUN` / `Observed` (observation N7); Linux | G01 | Engineering |
| FR-NODE-6 | P0 | Update registry and Git nodes (single) | `a_registry_update_stages_the_new_release_and_reports_up_to_date`, `git_update_fast_forwards_in_place_and_never_discards_edits`, `git_update_refuses_root_native_image_in_frozen_target_before_checkout` | None | PARTIAL | Native S11 | G05 | Engineering |
| FR-NODE-6 (Update all) | P1 | Batch update in reviewed groups of up to 128 | `update-all-dialog.test.tsx` | None | PARTIAL | Native | G05 | Engineering |
| FR-NODE-7 | P0 | Enable/disable via `.disabled/` | `enable_and_disable_round_trip_through_the_disabled_folder` | None | PARTIAL | Native S11b | G05 | Engineering |
| FR-NODE-8 | P0 | Uninstall to trash | `enable_disable_and_uninstall_use_the_journal_and_recycle_the_backup` | None | PARTIAL | Native | G05 | Engineering |
| FR-NODE-9 | P0 | Serialized, ComfyUI stopped, restart offer | `node_operations_need_a_stopped_runtime_the_gate_and_a_verified_environment` | Indirect: N29 | PARTIAL | Native S16a | G05 | Engineering |
| FR-NODE-10 | P1 | Manager optional (ticked in first run), post-stop rescan | `runtime_node_checks_reconcile_and_scan_without_acquiring_again`; `first_run_ticks_comfyui_manager_and_no_curated_nodes_by_default` | N11, N26, N28; N7 (post-stop rescan) | PARTIAL | Manager-driven change then rescan natively | G05 | Engineering |

### Settings

| ID | Pri | Clause / platform / profile | Automated evidence | Native evidence | Status | Remaining gap | Gate | Owner / next action |
|---|---|---|---|---|---|---|---|---|
| FR-SET-1 | P0 | Keyring or session memory | `available_backend_round_trip_and_one_shot_kind_binding`, `unavailable_probe_uses_session_memory_and_clear_succeeds`; ignored `real_keyring_round_trip` not executed | S7-NATIVE-LINUX-L8 (no Secret Service in the container → "SESSION ONLY") | PARTIAL | Real Credential Manager and Secret Service | G01, G03 | Engineering |
| FR-SET-2 | P0 | Token Test without read-back | Presence tests only (`credential_use_cases_report_presence_backend_and_session_warning`); no test of the Test request | None | OPEN | Test request for both providers | G03 | Owner: test tokens |
| FR-SET-3 | P0 | Settings set | `defaults_match_settings_policy`, `settings_default_load_and_one_call_save` | Not recorded | PARTIAL | Native; Linux | G01, G05 | Engineering |
| FR-SET-4 | P1 | Signed self-update | `invalid_signature_never_invokes_installer`, `update_install_requires_the_reviewed_offer` | None; updater inert in every build | OPEN | Key custody, endpoint, signed update install | G04 | Owner decision |

### Non-functional requirements

| ID | Pri | Clause / platform / profile | Automated evidence | Native evidence | Status | Remaining gap | Gate | Owner / next action |
|---|---|---|---|---|---|---|---|---|
| NFR-SEC-1 | — | Secret redaction | Redaction and diagnostics tests | None with a real token | PARTIAL | S12 | G03 | Owner: token |
| NFR-SEC-2 | — | Credential scoping; `HF_TOKEN` only in the `hf` child | `http_confirmation_happens_before_contact_and_direct_auth_is_never_forwarded`; N23 asserts no `HF_TOKEN`, `HOMEDRIVE`, `HOMEPATH` in the child | N23 | PARTIAL | Token-bearing runs | G03 | Owner: token |
| NFR-SEC-3 | — | Consent; first-run exception | `review_page_discloses_third_party_code_and_sends_consent`, `approved_hook_discloses_install_py_before_requirements`, `curated_auto_consent_installs_every_node_and_logs_warnings` | N10, N28 (review disclosure, URL@commit logged), N29 (page review, all boxes unchecked) | PARTIAL | Linux | G01 | Engineering |
| NFR-SEC-4 | — | Archive extraction | `malicious_archives_write_nothing_outside_owned_staging` | None needed for deterministic cases | PARTIAL | Linux-only pinned paths compiled and unit-tested in a container only (R04, S6-NATIVE-LINUX-N31) | G01 | Engineering |
| NFR-SEC-5 | — | Path validation and containment | `hostile_names_and_paths`, `consented_setup_git_prepare_allows_content_findings_but_preserves_containment` | None | PARTIAL | Linux (R04); DOS remapping boundary (R05) | G01 | Engineering |
| NFR-SEC-6 | — | Webview hardening, ACL | ACL integration tests (11 pass), `only_the_current_comfyui_origin_is_embedded` | S5-NATIVE-WIN-N1-003 | PARTIAL | Native invoke denial; Linux | G01 | Engineering |
| NFR-SEC-7 | — | Tool integrity | `uv_is_downloaded_verified_extracted_and_published_once`, `wheel_hash_failure_never_runs_uv` | N11, N28 (tools step), S7-NATIVE-WIN-W1 and S7-NATIVE-LINUX-L4 (hash-locked `tools/gdown` on both platforms) | PARTIAL | Linux desktop | G01 | Engineering |
| NFR-SEC-8 | — | Loopback default, LAN opt-in | `argv_is_controlled_and_keeps_tokens`; launch-options validation | N5 (`127.0.0.1`) | PARTIAL | LAN acknowledgement natively | G05 | Engineering |
| NFR-REL-1 | — | Long operations resumable or retryable | Setup retry, download resume, node recovery tests | N20 (Retry same run) | PARTIAL | S14 natively | G05 | Engineering |
| NFR-REL-2 | — | Atomic placement, side-by-side activation | `durable_publish_recovers_rename_before_sql_update_and_catalog_is_idempotent`, `restart_after_the_intent_rolls_the_activation_forward` | N11, N24 | PARTIAL | Linux | G01 | Engineering |
| NFR-REL-3 | — | No rollback of in-place changes; `repair_required` | `unapproved_drift_of_a_protected_package_marks_the_environment_repair_required`, `hook_failure_skips_requirements_and_retains_repair_state` | None | PARTIAL | S17 natively | G05 | Engineering |
| NFR-REL-4 | — | Git never resets or forces | `dirty_or_diverged_updates_change_neither_source_nor_packages`, `git_update_fast_forwards_in_place_and_never_discards_edits` | N18, N22 (local `master` kept) | PARTIAL | Dirty refusal natively | G05 | Engineering |
| NFR-REL-5 | — | Startup crash recovery | `startup_preserves_paused_native_partial_and_removes_only_owned_orphans`, `install_recovery_at_every_boundary` | None | PARTIAL | S14 | G05 | Engineering |
| NFR-PLAT-WIN-1 | — | WebView2 bootstrapper | Configuration only (`tauri.windows.conf.json`) | None (installers not executed) | OPEN | Installer run | G02 | Owner: VM |
| NFR-PLAT-WIN-2 | — | Unicode and spaces in paths | Library and writer tests | N15, N16 (`models ทดสอบ`), S5-NATIVE-WIN-N6-001 | PARTIAL | Install root with Unicode | G05 | Engineering |
| NFR-PLAT-WIN-3 | — | `LongPathsEnabled` warning | Hardware report tests | Not recorded | PARTIAL | Native with long paths off | G05 | Engineering |
| NFR-PLAT-WIN-4 | — | Locked files | See FR-MODEL-2/3 | S5-NATIVE-WIN-N9-001 | PARTIAL | Loaded-model lock | G05 | Engineering |
| NFR-PLAT-LINUX-1 | — | AppImage and .deb, `libwebkit2gtk-4.1` | Configuration only | S7-NATIVE-LINUX-L2 (both built), L3 (`.deb` installs with `libwebkit2gtk-4.1-0, libgtk-3-0` and runs), L8 (AppImage launches), all in a Debian 13 container | PARTIAL | Run on a Linux desktop | G01 | Owner: Linux machine |
| NFR-PLAT-LINUX-2 | — | System git, Secret Service fallback | Unit tests (compiled and run on Linux, S7-AUTO-LINUX-001) | S7-NATIVE-LINUX-L4 (setup cloned ComfyUI and ComfyUI-Manager with Debian git 2.47), L8 (session-only fallback without Secret Service) | PARTIAL | Secret Service on a desktop | G01 | Owner: Linux machine |
| NFR-PLAT-LINUX-3 | — | freedesktop trash | Compiled and unit-tested in a Linux container (S6-NATIVE-LINUX-N31) | None | OPEN | Linux | G01 | Owner: Linux machine |
| NFR-PLAT-LINUX-4 | — | WebKitGTK canvas performance | None | None | OPEN | Linux | G01 | Owner: Linux machine |
| NFR-PLAT-BOTH | — | Single instance | Single-instance plugin wiring | S5-NATIVE-WIN-P0-002 (second instance exits 0, first survives) | PARTIAL | Linux | G01 | Engineering |
| NFR-PERF-1 | — | UI interactions < 100 ms | None | Not measured | OPEN | Measurement | G07 | Engineering |
| NFR-PERF-2 | — | 2,000-file cached scan < 2 s | None | Not measured | OPEN | Measurement | G07 | Engineering |
| NFR-PERF-3 | — | Throughput ≥ 90 % of `curl` | None | Not measured | OPEN | Measurement | G07 | Engineering |
| NFR-UX-1 | — | DESIGN.md followed | Component Vitest | N3, N4 (overflow and contrast fixes) | PARTIAL | Full audit | G07 | Engineering |
| NFR-UX-2 | — | English copy; Thai glyphs | — | S5-NATIVE-WIN-N6-001, N15 (Thai renders on Windows) | PARTIAL | Thai on WebKitGTK | G01 | Engineering |
| NFR-UX-3 | — | Bundled fonts, offline, both themes, both webviews | Build embeds assets | Custom-protocol builds ran (no dev server); release `.deb`/AppImage rendered under WebKitGTK (S7-NATIVE-LINUX-L3…L8) | PARTIAL | Offline and both-theme audit; WebKitGTK on a desktop | G01, G07 | Engineering |
| NFR-UX-4 | — | WCAG AA, keyboard, 200 % zoom | None | Not audited | OPEN | Audit | G07 | Engineering |
| NFR-MAINT-1 | — | Architecture, boundaries, binding drift in CI | `check-arch-deps` 10/10 + OK, ESLint boundaries, bindings drift test | — | OWNER DECISION | Checks pass locally; CI workflows disabled by the owner | G10 | Owner |
| NFR-MAINT-2 | — | New provider through ports | Port structure (`SourceResolver`, `Downloader`) | — | PARTIAL | Structural only; no test | — | Engineering |
| NFR-MAINT-3 | — | Domain/application without Tauri, network, DB, GPU | `check-arch-deps` allow-lists; domain 48 and application 215 tests run without them | — | PASS | — | — | — |
| NFR-MAINT-4 | — | Coverage domain ≥ 90 %, application ≥ 80 % | Not measured for this tree | — | OPEN | `cargo llvm-cov` run | G06 | Engineering |
| NFR-MAINT-5 | — | ADRs for deviations | `docs/adr/0001`, `0002` | — | PASS | — | — | — |
| NFR-OBS-1 | — | App, ComfyUI-session and setup logs | Log hub tests | N11 (setup log), N5, N24 (`app.log`); S7-NATIVE-LINUX-L4, L6 (setup and ComfyUI session logs on Linux) | PARTIAL | Linux desktop | G01 | Engineering |
| NFR-OBS-2 | — | Copy diagnostics, redacted | Diagnostics tests (`== Application log (last 200 lines) ==`) | Not recorded | PARTIAL | Native copy | G05 | Engineering |

## Scenarios

| Scenario | Subclause | Windows result | Linux result | Evidence IDs | Remaining blocker / owner |
|---|---|---|---|---|---|
| S01 Clean setup | a. Fresh root, recommended profile, ComfyUI starts, `/system_stats` reports CUDA, final verify imports `torch`, `triton`, `comfy` | PARTIAL: passes on fresh roots of the development machine (debug/custom-protocol build) | PARTIAL: release `.deb` in a Debian 13 container with the GPU passed through by Docker Desktop (WSL2), not a Linux desktop | S6-NATIVE-WIN-N8 … N13, N28; S7-NATIVE-WIN-W1; S7-NATIVE-LINUX-L4, L6 | Clean machine with the installed release bundle (G02); Linux desktop with a native driver (G01) |
| S01 | b. The final wizard defaults (Manager ticked, curated unticked, recommended profile first, Latest release) | PARTIAL: defaults shown (N25, N26); a full run with exactly these defaults (Manager only) was not run | OPEN | S6-NATIVE-WIN-N25, N26 | Engineering: one default run (G05) |
| S02 Custom venv | a. Empty folder → app-owned venv | OPEN (automated only) | OPEN | — | G05, G01 |
| S02 | b. Existing venv → adopted, never deleted | OPEN (automated only) | OPEN | — | G05, G01 |
| S03 Change torch | a. Failed build keeps the old env active, ComfyUI starts | OPEN (automated only) | OPEN | — | G05, G01 |
| S03 | b. Success activates, data untouched | OPEN (automated only) | OPEN | — | G05, G01 |
| S04 Moved install | a. Copy detected, rebuild offered | PASS | OPEN | S5-NATIVE-WIN-N2a-001, S6-NATIVE-WIN-N19 | G01 |
| S04 | b. Rebuild succeeds; ComfyUI starts with models and nodes | PASS (after D3/D3b) | OPEN | S6-NATIVE-WIN-N19 (FAIL, fixed), N20 | G01 |
| S04 | c. Original root absent | OPEN | OPEN | — | G05 |
| S04 | d. Original root unchanged | PASS | OPEN | S5-NATIVE-WIN-N2a-001 | G01 |
| S05 CivitAI | a. Token download, SHA-256, suggested category | OPEN | OPEN | — | Owner: token (G03) |
| S05 | b. Gated without token → "token required" | OPEN | OPEN | — | G03 |
| S05 | c. Network cut at 50 % and resume | OPEN | OPEN | — | G03 |
| S06 Hugging Face | a. Gated `/blob/` without license → error with link | OPEN | OPEN | — | Owner: gated repo/token (G03) |
| S06 | b. LFS file via `hf`, verified | PARTIAL: one public file with a published SHA-256 verified; a non-Xet LFS file not separately exercised | OPEN | S6-NATIVE-WIN-N24 | G05 |
| S06 | c. Xet-backed file downloads and publishes | PASS | OPEN | S6-NATIVE-WIN-N23, N24 (S5-NATIVE-WIN-N4-001 failed before D4) | G01 |
| S06 | d. Pause shows "will restart from 0%", resume completes | OPEN (Vitest label only) | OPEN | — | G05 |
| S06 | e. No token in logs, DB, diagnostics | OPEN | OPEN | — | G03 |
| S06 | f. `hf` deleted → native fallback, resume after network cut | OPEN | OPEN | — | G05 |
| S07 Google Drive | a. > 100 MB public file via confirmation page | PASS (gdown engine; 498,881,336 B, pause restarts from 0) | PASS (container) | S7-NATIVE-WIN-W2…W5, S7-NATIVE-LINUX-L5 | — |
| S07 | b. Over-quota error | OPEN | OPEN | — | G03 |
| S08 Rename/delete while running | a. Locked-file error, Stop ComfyUI and retry succeeds | PARTIAL: controlled sharing lock refused; a lock held by a loaded model not exercised | OPEN | S5-NATIVE-WIN-N9-001 | G05 |
| S08 | b. Deleted files in the Recycle Bin | PASS | OPEN | S5-NATIVE-WIN-N9-001 | G01 |
| S09 Path attacks | a. Hostile rename rejected | PASS (automated, deterministic) | OPEN | S6-AUTO-WIN-007 (`hostile_names_and_paths`) | G01 |
| S09 | b. Crafted `node.zip` rejected, nothing outside staging | PASS (automated) | OPEN | S6-AUTO-WIN-007 (`malicious_archives_write_nothing_outside_owned_staging`) | G01 (Linux-only pinned paths, R04) |
| S10 Registry node | a. Consent prompt, id@version recorded | OPEN (automated only) | OPEN | — | G05 |
| S10 | b. Dependencies installed, torch unchanged | PARTIAL: shown for a Git node through the page (not Registry) | OPEN | S6-NATIVE-WIN-N29 | G05 |
| S10 | c. Requirement asking another torch is blocked and reported | OPEN (automated `node_previews_refuse_protected_package_changes`) | OPEN | — | G05 |
| S11 Git node with edits | a. Update refuses and shows `git status` | OPEN (automated only) | OPEN | — | G05 |
| S11 | b. Enable/disable round trip | OPEN (automated only) | OPEN | — | G05 |
| S12 Secret redaction | Grep logs, DB, exports after a CivitAI download | OPEN | OPEN | — | G03 |
| S13 Unsupported hardware | Profiles disabled with reasons; cu126 offered | OPEN (automated only; no such hardware) | OPEN | — | Owner: hardware (G05) |
| S14 Crash recovery | a. Kill during download → paused, resumable | OPEN | OPEN | — | G05 |
| S14 | b. Kill during node install → staging cleaned, `custom_nodes/` unchanged | OPEN | OPEN | — | G05 |
| S15 Architecture guard | a. Forbidden edge or cross-feature import fails the check | PASS (local) | PASS (repository-level check; its result does not depend on the OS) | S6-AUTO-WIN-002, S6-AUTO-WIN-008 | — |
| S15 | b. The guard runs in CI | OWNER DECISION (CI disabled) | OWNER DECISION | — | Owner (G10) |
| S16 Maintenance race | a. Start waits or is refused during a node install | OPEN (automated only) | OPEN | — | G05 |
| S16 | b. Unlocked rename while ComfyUI runs | PASS | OPEN | S5-NATIVE-WIN-N6-001 | G01 |
| S17 Failed dependency install | `repair_required`, Repair restores with torch unchanged | OPEN (automated only) | OPEN | — | G05 |

## Evidence register

### Automated and packaging evidence

Session 7: run by main on 2026-10-20, 19:03–19:06 (Windows) and 19:09–19:14 (Linux container), on the code of `0502a5e`. Session 6: run by main on 2026-10-20, 16:29–16:45, on the session-6 checkpoint ([s6-packaging.md](handoff/evidence/s6-packaging.md)). Every command exited 0.

| Evidence ID | Command / procedure | Expected / actual |
|---|---|---|
| S7-AUTO-WIN-001 | `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --locked -- -D warnings` | clean / ok, ok |
| S7-AUTO-WIN-002 | `pnpm -C ui install --frozen-lockfile`, `lint`, `typecheck`, `VITEST_MAX_WORKERS=2 pnpm -C ui test` | all pass / ok, ok, ok, 30 files, 202 tests |
| S7-AUTO-WIN-003 | `cargo test --workspace --locked --no-fail-fast` | 0 failed / 813 passed, 0 failed, 13 ignored |
| S7-AUTO-WIN-004 | `cargo test -p desktop --test bindings --locked` (after regeneration); `cargo test -p desktop --test acl --locked` | no drift; all pass / 1 passed, 1 ignored; 11 passed |
| S7-AUTO-WIN-005 | `node --test scripts/check-arch-deps.test.mjs`, `node scripts/check-arch-deps.mjs` | pass / 10/10, `OK` |
| S7-AUTO-WIN-006 | Ignored `real_gdown_resolves_and_downloads_through_the_relay` with `VJ_REAL_GDOWN_PYTHON` (coder run, scratch venv from the Windows lock) | pass / resolved `fcn8s_from_caffe.npz`, 498,881,336 B through the Drive relay; nonexistent id → `DrivePermission` |
| S7-PKG-WIN-001 | `pnpm -C ui tauri build` (release profile) | bundles / exit 0, NSIS and MSI, hashes in the snapshot; `NotSigned` |
| S7-AUTO-LINUX-001 | `docs/handoff/tooling/linux-check.sh` in `rust:1.99.0-trixie` | build, clippy, tests / ok, clean, 789 passed, 0 failed, 11 ignored |
| S7-PKG-LINUX-001 | `docs/handoff/tooling/linux-bundle.sh` (`pnpm -C ui tauri build`, release) | `.deb` + AppImage / both built, hashes in the snapshot; no rpm |
| S6-AUTO-WIN-001 | `pnpm -C ui install --frozen-lockfile` | ok / ok |
| S6-AUTO-WIN-002 | `pnpm -C ui lint`, `pnpm -C ui typecheck` | no diagnostics / ok, ok |
| S6-AUTO-WIN-003 | `VITEST_MAX_WORKERS=2 pnpm -C ui test` | all pass / 30 files, 202 tests passed |
| S6-AUTO-WIN-004 | `pnpm -C ui build` | ok / ok |
| S6-AUTO-WIN-005 | `cargo fmt --all -- --check` | clean / ok |
| S6-AUTO-WIN-006 | `cargo clippy --workspace --all-targets --locked -- -D warnings` | no warnings / ok |
| S6-AUTO-WIN-007 | `cargo test --workspace --locked --no-fail-fast` | 0 failed / 802 passed, 0 failed, 12 ignored |
| S6-AUTO-WIN-008 | `node --test scripts/check-arch-deps.test.mjs`, `node scripts/check-arch-deps.mjs` | pass / 10/10 pass, `OK` |
| S6-AUTO-WIN-009 | `cargo test -p desktop --test bindings --locked` | no drift / 1 passed, 1 ignored (exporter) |
| S6-AUTO-WIN-010 | `cargo test -p desktop --test acl --locked` | all pass / 11 passed |
| S6-AUTO-WIN-011 | `cargo build -p desktop --locked --features tauri/custom-protocol` | ok / ok |
| S6-AUTO-WIN-012 | Ignored `startup_smoke::fresh_root_starts_and_closes_cleanly`, run explicitly | pass / 1 passed |
| S6-PKG-WIN-001 | `pnpm -C ui tauri build` (release profile) | bundles / exit 0, NSIS and MSI, hashes in the snapshot |
| S6-PKG-WIN-002 | `Get-AuthenticodeSignature` on both installers and the release exe | — / `NotSigned` (unsigned by owner decision) |
| S6-PKG-WIN-003 | PE `SizeOfStackReserve` of the release exe | 8 MiB / 8,388,608 |
| S6-PKG-WIN-004 | Updater configuration at build time | absent / inert |
| S5-AUTO-WIN-001 | Session-5 check list (HEAD `a4175d1` + P0, I, D2) | 718 passed, 0 failed, 10 ignored; Vitest 29 files, 184 tests |
| HIST-WIN-2026-10-16 | `cargo test -p infrastructure --locked --test real_setup -- --ignored --nocapture`, `VJ_REAL_ROOT=D:/vjc-acceptance-20261016` | Direct setup (bypasses app startup) completed in 163 s: Python 3.13.16, torch 2.14.1+cu130, CUDA tensor operation on the RTX 3060. Historical. |

### Native evidence, session 6

Procedure: the driver script `D:\vjc-s6\drive.ps1` launches the frozen binary with `VJUMPKUNG_COMFYUI_HOME` for the child only, takes PrintWindow screenshots, drives UI Automation and closes with WM_CLOSE. Screenshots and logs stay on the development machine under `D:\vjc-s6\`. Full rows: [s6-native-register.md](handoff/evidence/s6-native-register.md).

| Evidence ID | Time | Binary / root | Procedure | Expected / actual | Result |
|---|---|---|---|---|---|
| S6-NATIVE-WIN-N1 | 12:40 | `s6-01` / `D:\comfyui-test\v1` | Start the app on an installation with custom nodes | Starts / main-thread stack overflow, exit 127 (P0 S0) | FAIL (fixed by S0) |
| S6-NATIVE-WIN-N2 | 12:41 | `s6-01` with `editbin /STACK:8388608` / same | Start | Starts / "application setup finished" | PASS (diagnosis) |
| S6-NATIVE-WIN-N3 | 12:42 | same | Settings and ComfyUI pages scrolled to the bottom | One scrollbar / one scrollbar, sidebar full height | PASS |
| S6-NATIVE-WIN-N4 | 12:43 | same | Models and Custom Nodes active row | Readable / muted row with accent bar | PASS |
| S6-NATIVE-WIN-N5 | 12:44 | same | Start ComfyUI, py312 profile | Running on CUDA / Python 3.12.15, torch 2.10.0+cu130, ComfyUI 0.42.1, cuda:0; no `pwd` traceback | PASS |
| S6-NATIVE-WIN-N6 | 12:52 | `s6-02` / same | Start without editbin | Starts / setup finished | PASS |
| S6-NATIVE-WIN-N7 | 12:54 | same | Start, Stop (worker rescan), Rescan, WM_CLOSE | No overflow / clean shutdown | PASS |
| S6-NATIVE-WIN-N8 | 13:41 | `s6-04` / fresh `r1` | First launch | Only `data` in the root / only `data` | PASS |
| S6-NATIVE-WIN-N9 | 13:42 | same | Wizard profile step | py312 recommended preselected / preselected (listed last; fixed in G2b, see N25) | PASS |
| S6-NATIVE-WIN-N10 | 13:43 | same | Review step, Manager + 21 curated | Disclosure, sources, "Start setup" / as expected | PASS |
| S6-NATIVE-WIN-N11 | 13:41–13:50 | same | Full first run | No consent pause, final verify, app restarts / ~8.5 min, restart observed | PASS |
| S6-NATIVE-WIN-N12 | 13:51 | same | Git state after setup | Branches track origin / nodes on default branches; ComfyUI detached at v0.42.1 | FAIL for ComfyUI (fixed by G2b, G2c) |
| S6-NATIVE-WIN-N13 | 13:52 | same | Start ComfyUI | All nodes import / 21 curated + Manager imported, cuda:0 | PASS |
| S6-NATIVE-WIN-N14 | 14:16 | `s6-06` / `r1` | Switch to the current tag | Attached / still detached | FAIL (fixed by G2c) |
| S6-NATIVE-WIN-N15 | 14:17 | same | Generate extra model paths, base `D:\vjc-s6\models ทดสอบ` | Section written, folders created / 27 keys, 18 folders, checkout clean | PASS |
| S6-NATIVE-WIN-N16 | 14:18 | same | ComfyUI loads the extra path | Model listed / `GET /models/loras` and Models page list it | PASS |
| S6-NATIVE-WIN-N17 | 14:20 | same | Remove the generated section | File removed, folders kept / as expected; close needed a WM_CLOSE retry | PASS |
| S6-NATIVE-WIN-N18 | 14:31 | `s6-07` / `r1` | Switch to v0.42.1 | Tracks `release/v0.42` / `git pull --ff-only` "Already up to date." | PASS |
| S6-NATIVE-WIN-N19 | 14:35 | `s6-08` / `r1-copy` (`robocopy /E`, 11.2 GB) | Moved copy, Rebuild | Rebuild succeeds / `rebuild_required` shown; rebuild failed at tools (copied `cpython-3.12` link) | FAIL (fixed by D3b) |
| S6-NATIVE-WIN-N20 | 14:45 | `s6-09` / `r1-copy` | Retry same run | Rebuild succeeds, no new registry key / succeeded ~2 min, restart, ComfyUI on cuda:0, no new HKCU entry | PASS |
| S6-NATIVE-WIN-N21 | 15:05 | `s6-10` / `r1` | Update to latest commit (git pull) | On `master` tracking origin / `## master...origin/master`, ComfyUI runs | PASS |
| S6-NATIVE-WIN-N22 | 15:12 | same | Switch back to v0.42.1 | On `release/v0.42` / as expected, local `master` kept | PASS |
| S6-NATIVE-WIN-N23 | 15:27 | live test `real_hf_download_succeeds_without_user_profile_variables`, `VJ_REAL_HF_EXE` = pinned hf 2.2.0 | Ignored test run explicitly | Download, size and SHA-256 / 1 passed in 18.74 s | PASS |
| S6-NATIVE-WIN-N24 | 15:31 | `s6-11` / `r1` | In-app Hugging Face download (Xet), `vae-ft-mse-840000-ema-pruned.safetensors` | COMPLETED VERIFIED / 334,641,190 B, SHA-256 `735e4c3a…3c75` | PASS |
| S6-NATIVE-WIN-N25 | 15:50 | `s6-11` / fresh `r2` | Wizard profile order | Recommended first / first and preselected | PASS |
| S6-NATIVE-WIN-N26 | 15:52 | same | Wizard Options defaults | Latest release; Manager on; curated off / as expected; root holds only `data` | PASS |
| S6-NATIVE-WIN-N27 | 16:12 | test `node_preview_never_imports_python_modules_from_node_directory` with pinned uv 0.13.1 | D1 stop condition | No trap import / 1 passed, no trap file | PASS |
| S6-NATIVE-WIN-N28 | 16:18 | `s6-13` (final code) / fresh `r3` | First run with Manager + ComfyUI-PlagueKind-Nodes | Selectable, disclosure, warning acknowledged / run succeeded, PlagueKind imported, cuda:0 | PASS |
| S6-NATIVE-LINUX-N31 | 17:02 | Docker `rust:1.99.0-trixie` (linux/amd64) | Workspace build, clippy `-D warnings`, `cargo test --workspace` under xvfb (`docs/handoff/tooling/linux-check.sh`) | Build ok, clippy clean, all tests pass / 778 passed, 0 failed, 10 ignored (after a Unix-only test quoting fix) | PASS (compile/unit only) |
| S6-NATIVE-WIN-N29 | 16:25 | same | Custom Nodes page: Install from Git `ltdrdata/ComfyUI-Impact-Subpack` with `install.py` | Hook before requirements, pins unchanged / order logged; torch 2.10.0+cu130, torchvision 0.25.0+cu130, torchaudio 2.10.0+cu130 unchanged | PASS |

Not run natively: a Custom Nodes page install of a repository with a top-level `.dll` (no public fixture). It is covered by the Windows real-Git tests `git_install_refuses_root_native_image_before_checkout` and `git_update_refuses_root_native_image_in_frozen_target_before_checkout`.

### Native evidence, session 5

Binary `S5-BIN-02` (session-5 code; superseded by session 6 for the areas it changed). Full rows: [s5-native-register.md](handoff/evidence/s5-native-register.md).

| Evidence ID | Scenario | Result |
|---|---|---|
| S5-NATIVE-WIN-P0-001 | Startup crash before the P0 fix | FAIL before fix; smoke test passes after |
| S5-NATIVE-WIN-P0-002 | Startup paths, second instance, WM_CLOSE | PASS |
| S5-NATIVE-WIN-N1-001 | S01 with all 22 curated (then default) | FAIL at PlagueKind (`__pycache__`); superseded by N28 |
| S5-NATIVE-WIN-N1-002 | S01 with 21 curated, py313 profile | PASS |
| S5-NATIVE-WIN-N1-003 | S01 runtime, embedded window, Stop | PASS (three node import findings, resolved by the py312 profile with torchaudio, see N13) |
| S5-NATIVE-WIN-N2a-001 | S04 copy detected; original unchanged | PARTIAL (rebuild failed, fixed by D3, D3b) |
| S5-NATIVE-WIN-N4-001 | S06 public Xet download | FAIL (fixed by D4, see N24) |
| S5-NATIVE-WIN-N6-001 | S16b rename while running, Thai name | PASS |
| S5-NATIVE-WIN-N9-001 | S08 controlled lock and Recycle Bin | PASS (controlled sharing lock, not a loaded model) |
| S5-NATIVE-WIN-REG-001 | `HKCU\Software\Python\Astral` side effect | FINDING (fixed by D5, see N20) |

### Open evidence

| Evidence ID | Missing evidence |
|---|---|
| S6-OPEN-LINUX-001 | Any Linux compile, test or run, including the Linux-only containment code and Linux bundles |
| S6-OPEN-PKG-001 | NSIS and MSI install, launch, uninstall and data preservation on a disposable Windows machine; WebView2 bootstrapper |
| S6-OPEN-PROVIDER-001 | CivitAI token, gated Hugging Face, Google Drive confirmation and quota |
| S6-OPEN-SIGN-001 | Authenticode signature; updater key, endpoint and a signed update install |
| S6-OPEN-COV-001 | `cargo llvm-cov` for domain and application on this tree |

### Ignored-test inventory

The final run reports 12 ignored tests, and the code has 12 `#[ignore]` definitions (the docs-package prompt said 11; the code count is used here).

| Test | File | Kind | Session 6 |
|---|---|---|---|
| `export_bindings` | `crates/desktop/tests/bindings.rs` | Binding exporter | Run by coders to regenerate; the drift test passes |
| `fresh_root_starts_and_closes_cleanly` | `crates/desktop/tests/startup_smoke.rs` | Native acceptance | **Executed, pass** (S6-AUTO-WIN-012) |
| `shared_bridge_grandchild`, `shared_bridge_child` | `crates/infrastructure/src/comfy_runtime/process/tests.rs` | Helper subprocess fixtures | Executed indirectly by the bridge ownership test |
| `working_directory_child`, `dll_lookup_child` | `crates/infrastructure/src/shared/process/tests.rs` | Helper subprocess fixtures | Executed indirectly |
| `real_hf_download_succeeds_without_user_profile_variables` | `crates/infrastructure/src/models/acquisition/hf_engine.rs` | Live acceptance | **Executed, pass** (N23) |
| `real_gdown_resolves_and_downloads_through_the_relay` | `crates/infrastructure/src/models/acquisition/gdown_engine.rs` | Live acceptance (added in session 7) | Not executed (needs `VJ_REAL_GDOWN_PYTHON`) |
| `real_hf_xet_obeys_relay_with_direct_egress_denied` | `crates/infrastructure/src/models/acquisition/hf_proxy/tests.rs` | Live acceptance | Not executed (needs external egress control) |
| `a_real_interpreter_is_probed` | `crates/infrastructure/src/environment/python/tests.rs` | Live acceptance | Not executed (needs `VJ_TEST_PYTHON`) |
| `real_tools_are_provisioned` | `crates/infrastructure/src/environment/tools/tests.rs` | Live acceptance | Not executed; the tools step ran natively in N11, N19, N20, N28 |
| `real_keyring_round_trip` | `crates/infrastructure/src/settings/secrets/tests.rs` | Live acceptance | Not executed |
| `real_first_run_setup` | `crates/infrastructure/tests/real_setup.rs` | Live acceptance (bypasses app startup) | Not executed; superseded by app-driven runs N11, N28 |

`node_preview_never_imports_python_modules_from_node_directory` is not ignored but skips without uv; it ran with pinned uv 0.13.1 (N27).

### Session 7 native evidence

Recorded in [s7-native-register.md](handoff/evidence/s7-native-register.md) (Windows W1–W10 with the debug/custom-protocol build `s7-01`; Linux L1–L8 in Docker `rust:1.99.0-trixie` with `--gpus all`; Windows packaging 19:34). Source: HEAD `7d7b8f3` + package G7 (Google Drive through gdown 6.4.2): 813 passed / 0 failed / 13 ignored on Windows, 789 / 0 / 11 on Linux.

| Evidence ID | Check | Result |
|---|---|---|
| S7-NATIVE-WIN-W1 | First run, recommended profile; tools step provisions `tools/gdown` | PASS |
| S7-NATIVE-WIN-W2…W5 | Drive share link `/file/d/<id>/view?usp=sharing` resolved by gdown, exact progress total, pause restarts from 0, published 498,881,336 B (SHA-256 `c187c387…`) | PASS |
| S7-NATIVE-WIN-W6, W7 | Nonexistent id → permission/not-found message; folder link → folder error | PASS |
| S7-NATIVE-WIN-W8, W9 | Missing `tools/gdown` → Repair reinstalls it (replacement build, R29); ComfyUI starts on cuda:0 | PASS |
| S7-NATIVE-WIN-W10 | No new `HKCU\Software\Python\Astral` keys after first run and Repair (`--no-registry`) | PASS |
| S7-NATIVE-LINUX-L1 | `linux-check.sh`: build, clippy, 789 passed / 0 failed / 11 ignored | PASS |
| S7-NATIVE-LINUX-L2, L3 | `linux-bundle.sh`: `.deb` + AppImage built; `.deb` installs and launches (first GUI run on Linux) | PASS |
| S7-NATIVE-LINUX-L4…L8 | First run `linux-py313-torch2141-cu130`; Drive download; ComfyUI on cuda:0 (torch 2.14.1+cu130); xterm log and embedded window in WebKitGTK; AppImage launch | PASS (container, GPU via WSL2); observations R30, R31, R32 |

## Residuals

Source: [s3-residuals.md](handoff/evidence/s3-residuals.md).

| ID | Residual | Current evidence | Disposition | Consequence / workaround | Owner |
|---|---|---|---|---|---|
| R01 | Downloads stay stopped after a failed self-update install until restart | Code (no supervisor restart API) | OPEN | Restart the app | Engineering |
| R02 | Updater inert without build-time key and endpoint; tracing worker stopped after a failed install | `updates.rs`; S6-PKG-WIN-004 | OWNER DECISION | No self-update in current builds | Owner |
| R03 | HF relay on loopback has no per-download authentication | Advisor H plan §6 | ACCEPTED RESIDUAL | Another local process can tunnel to HF hosts during a transfer | Engineering |
| R04 | Linux-only code compiled, clippy-clean and unit-tested on Linux x64 in a container (789 passed); the app ran as a GUI in a GPU container but never on a Linux desktop | S6-NATIVE-LINUX-N31, S7-NATIVE-LINUX-L1…L8 | PARTIAL | No Linux support claim | Owner: Linux machine (G01) |
| R05 | Drive-letter remapping outside the Windows boundary; all reparse points refused | Advisor reparse policy | ACCEPTED RESIDUAL | OneDrive placeholders, dedup and WOF entries unsupported in node trees | Engineering |
| R06 | Child DLL search after consent | Windows admission tests, N27 | PASS before consent (D1); ACCEPTED RESIDUAL after consent and for installed or user-placed nodes ([s6-advisor-n.md](handoff/evidence/s6-advisor-n.md)) | Consented curated setup may install top-level native images with a warning | Engineering |
| R07 | Early child exit reported as `InputFailed` | H2 report | OPEN (minor) | Message less specific | Engineering |
| R08 | Git config preflight is a separate process | H2 report; spec | ACCEPTED RESIDUAL | Same-user edit window | Engineering |
| R09 | Editable/source refusal order | H2 report | PASS (evidence note) | — | — |
| R10 | Scaffold publication recovery; xterm geometry | `publish.rs`; S5-NATIVE-WIN-N1-003 | PASS | — | — |
| R11 | Recycle cases not exercised (non-recyclable volume, disabled Recycle Bin, oversized, UNC) | Fixed NTFS only | OPEN | Unknown behaviour on those volumes | Engineering (G05) |
| R12 | Verbatim-path recycle bug | Fixed; S5-NATIVE-WIN-N9-001 | PASS | — | — |
| R13 | Headless process after a refused close when no window remains | Code | OPEN (minor) | Launch the app again to restore the manager | Engineering |
| R14 | Curated nodes detached at a catalog commit | Superseded by F: latest default-branch commit, tracking (N12) | PASS (superseded) | — | — |
| R15 | Setup skipped `install.py` | Consented setup now runs it first (N report); non-consented setup still skips it | PASS for consented first-run setup; OPEN for non-consented setup | Reinstall in Custom Nodes and approve `install.py` | Engineering |
| R16 | One failing curated repository stops the batch until the step is skipped | Thinker E2 review | OPEN | Abort and start setup without the failing node | Engineering |
| R17 | Final-validation mismatch leaves `DependenciesInProgress` | Thinker E2 review | OPEN | Restart the app to reconcile | Engineering |
| R18 | No real-Git curated test | Native N11 (21 curated), N28 | PASS on Windows (native); `real_setup` still selects none | — | — |
| R19 | Accelerator wheels never installed for real | Fakes only | OPEN | Accelerators experimental and unchecked | Engineering (G08) |
| R20 | Conservative runtime assessment can return Unknown | Design | ACCEPTED RESIDUAL | Retry Stop/close | Engineering |
| R21 | Move detection needs a valid `install.json`; related relocation cases | Thinker E3 review; [s5-advisor-r.md](handoff/evidence/s5-advisor-r.md) | OPEN (separately gated) | A copied root without a valid manifest composes against old-root DB paths | Engineering (G08) |
| R22 | Debug main-thread stack overflow | Fixed by S0; N6, N7, S6-PKG-WIN-003 | PASS | — | — |
| R23 | Copied uv minor-version links | Fixed by D3, D3b; N20 | PASS on Windows; Linux symlink branch OPEN | — | Engineering (G01) |
| R24 | Managed Python registered under HKCU | Fixed by D5; N20 | PASS | Stale keys from older builds can be removed by the user | — |
| R25 | Every `hf` download failed on Windows | Fixed by D4; N23, N24 | PASS | — | — |
| R26 | Children inherited the manager's working directory | Closed by D1 | PASS | — | — |
| R27 | Setup IPC still accepts `allow_source_builds=true` (UI never sends it) | [s5-exp-r6.md](handoff/evidence/s5-exp-r6.md) | OPEN (latent) | None observed | Engineering |
| R28 | Git `UnsafeSource` may leave `.disabled/.vjump-staging-*` | Needs verification | OPEN | Possible leftover staging folder | Engineering (G08) |
| R29 | Repair on a healthy root where only a download tool (`tools/hf` or `tools/gdown`) is missing builds a whole replacement environment | S7-NATIVE-WIN-W8 | OPEN (minor) | Correct but slow; torch is downloaded again when the uv cache was cleaned. Drive downloads use the native fallback until then | Engineering (advisor) |
| R30 | ComfyUI-Manager logs "[ComfyUI-Manager] PyTorch is not installed" on Linux only | S7-NATIVE-LINUX-L6 | OPEN (needs investigation) | ComfyUI itself runs on CUDA; Manager features that check torch may misbehave | Engineering (G01) |
| R31 | gdown publishes Drive files with mode `0600` and Drive's modification time (Linux) | S7-NATIVE-LINUX-L5 | OPEN (minor) | Other local users cannot read the model; ComfyUI under the same user can | Engineering |
| R32 | Graceful shutdown on Linux (SIGTERM / window close) not verified: container runs closed the app by signal or WebDriver | S7-NATIVE-LINUX-L1…L8 | OPEN | Unknown whether "application shutdown finished" runs on Linux | Engineering (G01) |

## Release gates

| Gate | Unmet criterion | Required closure evidence | Owner |
|---|---|---|---|
| G01 Linux | Release 1 includes Linux x64; the workspace compiles and passes its automated tests in a Debian container (S7-NATIVE-LINUX-L1); the release `.deb`/AppImage ran in a GPU container (first run, ComfyUI on CUDA, Drive download; S7-NATIVE-LINUX-L2…L8), but nothing ran on a Linux desktop | Linux x64 NVIDIA desktop with a native driver: S01 with the installed `.deb` and AppImage, Secret Service, trash, WebKitGTK canvas and Thai fonts, graceful shutdown (R32), the Manager torch check (R30) | Owner provides the machine; engineering runs it |
| G02 Installers | NSIS and MSI never executed; release exe never launched | On a disposable Windows VM: install both hashed artifacts, WebView2 bootstrapper, first run, uninstall and reinstall with data preserved | Owner provides the VM; engineering runs it |
| G03 Providers | CivitAI, gated Hugging Face, the Google Drive quota case and redaction scenarios need credentials or fixtures (S07a passes, S7-NATIVE-WIN-W2…W5) | S05, S06a/e, S07b, S12, FR-SET-2 with test credentials in a separate credential context | Owner: test tokens |
| G04 Signing | Unsigned bundles; updater inert | Authenticode certificate, updater key custody and endpoint, a signed update installed end to end (FR-SET-4) | Owner decision |
| G05 Windows native scenarios | Scenarios and clauses with automated evidence only | S01b default run, S02, S03, S04c, S06d/f, S08a, S10, S11, S13 (needs hardware), S14, S16a, S17; other "Native" gaps in the requirements tables | Engineering |
| G06 Coverage | NFR-MAINT-4 not measured | `cargo llvm-cov` on this tree: domain ≥ 90 %, application ≥ 80 % lines | Engineering |
| G07 Performance and UX audit | NFR-PERF-1..3, NFR-UX-3/4 not measured | Timing measurements; offline, both-theme, keyboard and 200 % zoom audit | Engineering |
| G08 Security and robustness residuals | R19, R21, R28 open | Native accelerator run or removal; containment evidence or a fix for R21; verification of R28 | Engineering, advisor |
| G09 License and icons | No license; placeholder icons | Owner chooses a license and final icons | Owner decision |
| G10 CI | Workflows disabled | Owner re-enables CI or confirms local checks | Owner decision |

The verdict stays **NO-GO** while any of G01–G05 is open. G06–G08 must also close before a release is called accepted. G09 and G10 are owner decisions and do not change the evidence above.
