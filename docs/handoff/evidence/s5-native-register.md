# Session 5 — native evidence register (Windows 11, RTX 3060)

Raw artifacts (screenshots, run logs, manifests, DB copies, system_stats) live under `D:\vjc-s5\evidence\` on the
development machine (not in the repository). Every run used the DEBUG build with `--features tauri/custom-protocol`
(no dev server) and the debug-only root override `VJUMPKUNG_COMFYUI_HOME`, set for the child process only.
Owner isolation: `%APPDATA%\vjumpkung-comfyui\bootstrap.json` SHA-256 `69b4da8c…760e` and the owner's root
`D:\vjump\test-comfyui` (10,246-file manifest) were baselined before the runs; no Locate/New/root switching was used;
no credential is stored under the app's keyring service (`cmdkey` shows none), so HF runs were anonymous.

Machine: Windows 11 Education 10.0.26300, NVIDIA GeForce RTX 3060 12 GiB (driver 610.88, CUDA 13.3), 12 logical
CPUs, 32 GB RAM. Toolchain: rustc 1.99.0 (MSVC), node 24.13.0, pnpm 10.24.0. HEAD `a4175d1` + uncommitted P0 + I.

## Binary
| ID | Binary | SHA-256 | Source checkpoint |
|---|---|---|---|
| S5-BIN-02 | `D:\vjc-s5\bin\vjumpkung-comfyui-desktop-s5-02.exe` (debug, custom-protocol) | `a642d3eb58376753580b6b050f2cbb1c2540e98edaa12235e167b7489d4342d0` | `s5-02-through-i-verified.patch` (`d99a96f9…cf5`) |

## Runs with S5-BIN-02
| Evidence ID | Scenario / check | Procedure | Result |
|---|---|---|---|
| S5-NATIVE-WIN-P0-001 | Startup crash (owner's "segfault") | Run committed debug exe on a fresh root | FAIL before fix: exit 101, `state() called before manage()` at lib.rs:131. After P0: smoke test passes (exit 0 after WM_CLOSE). |
| S5-NATIVE-WIN-P0-002 | Startup paths | Half-set-up copy of owner root; recovery mode (root is a file); 2nd instance during init and after Ready; WM_CLOSE during init and after Ready | PASS: all exit 0; second instance exits 0 and the first survives; recovery alert shown. |
| S5-NATIVE-WIN-N1-001 | S1 default first run (all 22 curated) | Fresh root `D:\vjc-s5\a`, recommended profile, Latest release, Manager off | FAIL (finding D2): every step through `manager` succeeded (clone_comfyui succeeded — first-run scaffold fix confirmed from the app; torch 2.14.1+cu130 installed and CUDA-verified); `curated_nodes` failed "unsafe node path, archive or source" at ComfyUI-PlagueKind-Nodes (ships `__pycache__`). Batch review UI for the 21 prepared plans rendered correctly. |
| S5-NATIVE-WIN-N1-002 | S1 continuation (PlagueKind deselected, 21 curated) | Fresh root `D:\vjc-s5\a2`; one batch review of 21 plans (42 consents: 21 execute + 21 warning acks, all unchecked initially, no protected-package changes); Approve and retry | PASS: run `6c4c8f0f…` succeeded 03:42→03:51 UTC; final verify `{"python":"3.13.16","torch":"2.14.1+cu130","cuda":true,"triton":"3.8.0","device":"NVIDIA GeForce RTX 3060","cuda_ok":true}`; app shell shown. |
| S5-NATIVE-WIN-N1-003 | S1 runtime | Start ComfyUI from the app; `GET /system_stats`; embedded window; Stop | PASS: ComfyUI v0.42.1 (63be2f9b) on 127.0.0.1:8188, `cuda:0 NVIDIA GeForce RTX 3060 : cudaMallocAsync`, PyTorch 2.14.1+cu130, Python 3.13.16; embedded "ComfyUI" window loads the frontend; Stop frees the port, no python left. Findings: 3 of 21 curated nodes fail to import — MelBandRoFormer and WanVideoWrapper (`No module named 'torchaudio'`, profile omits torchaudio by design), rgthree-comfy @5f940ddfbb83 (`execution.recursive_output_delete_if_changed` missing in ComfyUI 0.42.1). xterm log frame fits its border in real WebView2 (residual 10 visual check). |
| S5-NATIVE-WIN-N2a-001 | S4 moved install (original present) | Close app; manifest of `a2` (124,359 entries, `8be53cb0…`); `robocopy /E` a2→`D:\vjc-s5\b` (116,393 files, 11.24 GB; follows the uv minor-link junction); launch on b | PARTIAL: `rebuild_required` (moved) detected; guided-rebuild screen shows root, recorded profile `win-py313-torch2141-cu130` rev 1, Python 3.13.16 and preserved data. Rebuild FAILED at `tools` (finding D3: copied minor-version link, os error 145); failed run stays inspectable/retryable with a fresh-rebuild option. Original `a2` byte-identical afterwards (124,359-entry manifest equal). |
| S5-NATIVE-WIN-N4-001 | S6 HF public Xet file | Fresh healthy root `D:\vjc-s5\c` (no curated); Add download `stabilityai/sd-vae-ft-mse-original/blob/main/vae-ft-mse-840000-ema-pruned.safetensors` (commit 629b3ad…, 334,641,190 B, sha256 735e4c3a…8f3c75, Xet) → vae | FAIL (finding D4): Look up worked (HF detected, size, "checksum published"); job failed in ~1 s, `ToolRuntime`, cause unlogged; reproduced: hf 2.2.0 crashes at import without USERPROFILE. |
| S5-NATIVE-WIN-N6-001 | S16 (part) rename while ComfyUI runs | Fixture VAE (sha256 verified) in `c\ComfyUI\models\vae`; library watcher picked it up; ComfyUI running; Rename → `ทดสอบ fixture vae.safetensors` | PASS: file renamed on disk, library row/details updated, Thai renders, ComfyUI kept running. |
| S5-NATIVE-WIN-N9-001 | S8 (part) controlled lock + Recycle Bin | Separate process holds the file open without delete sharing for 90 s; Delete via UI; after release Delete again | PASS: locked delete refused "Another program is using the file. Close it, or stop ComfyUI, then retry."; after release the confirm dialog moved it to the Recycle Bin (original location recorded); "1 of 1 file deleted"; ComfyUI kept running. Label: controlled sharing lock, not a loaded-model lock. |
| S5-NATIVE-WIN-REG-001 | Side effect check | `HKCU\Software\Python\Astral` | FINDING D5: CPython3.13.16 → `D:\vjc-s5\c\…`, CPython3.12.15 → `D:\vjump\test-comfyui\…` (owner's earlier run). Left in place (owner's registry). |

Pending after fixes D2 → D3+D5 → D4 → D1 (new binary): S1 default with 21 curated; S4 retry on b + N2b (original absent);
S6 download/pause/resume (N4) + crash (N5); S10 registry node (N7); S11 dirty Git + enable/disable (N8); R6 live test;
packaging build.
