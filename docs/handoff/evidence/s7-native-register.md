# Session 7 — native evidence register (2026-10-20, 18:00–19:45 +07:00)

Machine: Windows 11 Education 10.0.26300 x64, NVIDIA GeForce RTX 3060 12 GiB (driver 610.88, CUDA 13.3), Docker Desktop
29.9.1 (WSL2, linux/amd64, NVIDIA container runtime). Source: HEAD `7d7b8f3` + package G7 (Google Drive through gdown,
uncommitted, verified by main: fmt, clippy `-D warnings`, 813 passed / 0 failed / 13 ignored, bindings, ACL 11, UI lint,
typecheck, Vitest 30 files / 202 tests, arch OK). Advisor ruling: read-only Opus stand-in (Codex rate-limited), GO with
corrections; coder: Sonnet 5.5 xhigh fallback.

Drivers: Windows — `docs/handoff/tooling/cdp.mjs` (Chrome DevTools Protocol against WebView2 started with
`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`; Node 24, no dependencies). Linux —
`docs/handoff/tooling/linux-gui/` (Docker image with the built `.deb`, Xvfb, D-Bus, `tauri-driver` 2.x + WebKitWebDriver;
`wd.py` W3C WebDriver client; GPU via `--gpus all`). Public Drive fixture: `1l_5RK28JRL19wpT22B-DY9We3TVXnnQQ`
(`fcn8s_from_caffe.npz`, 498,881,336 B, goes through Drive's virus-scan confirmation page).

## Windows (debug + `tauri/custom-protocol` build `vjumpkung-comfyui-desktop-s7-01.exe`, SHA-256 `da16e0d7…1f09da`, root `D:\vjc-s7\r1` via `VJUMPKUNG_COMFYUI_HOME`)
| # | When | Check | Result |
|---|---|---|---|
| W1 | 19:05 | First-run setup, recommended `win-py312-torch2100-cu130`, Manager on, curated off | PASS: 14/14 steps; tools step created `tools/gdown` (marker `gdown=6.4.2`, `python=3.13.16`, lock `7316c1a3…`); `import gdown` → 6.4.2 on 3.13.16 |
| W2 | 19:11 | Add download: `https://drive.google.com/file/d/1l_5RK28JRL19wpT22B-DY9We3TVXnnQQ/view?usp=sharing` → Look up | PASS: resolved by gdown to `fcn8s_from_caffe.npz`, "Unknown size · unverified" |
| W3 | 19:11 | Download to `checkpoints` | PASS: exact total 475.8 MiB shown during transfer; staging `data/downloads/gdrive/<job>/payload*.part` |
| W4 | 19:12 | Pause / resume | PASS: Pause button titled "Pause (this download will restart from 0%)"; PAUSED + "Resuming restarts this download from 0%."; staging removed; no gdown `python.exe` left (Job Object covers the venv launcher's child); resume restarted from 0 |
| W5 | 19:12 | Completion | PASS: COMPLETED; `models\checkpoints\fcn8s_from_caffe.npz` 498,881,336 B, SHA-256 `c187c387…ca934`; staging empty |
| W6 | 19:13 | Look up `https://drive.google.com/open?id=1AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA` | PASS: "Google Drive denied access or the file does not exist. The file must be shared with "Anyone with the link"." |
| W7 | 19:13 | Look up `https://drive.google.com/drive/folders/…?usp=sharing` | PASS: "Google Drive folders are not supported. Use a link to a single file." |
| W8 | 19:14–19:25 | `tools/gdown` deleted (simulates a root set up before G7) → Environment → Repair | PASS: Repair does not stop at "nothing to repair"; tools step reinstalled `tools/gdown` (marker present). Observation: Repair builds a replacement environment for this case (torch from the uv cache here); the Manager consent prompt of Repair appeared as designed and Retry same run after approval → 14/14 succeeded |
| W9 | 19:26 | Start ComfyUI on the repaired env, `/system_stats` | PASS: Python 3.12.15, torch 2.10.0+cu130, ComfyUI 0.42.1, `cuda:0 NVIDIA GeForce RTX 3060`; Stop; WM_CLOSE → "application shutdown finished" |
| W10 | 19:30 | No new `HKCU\Software\Python\Astral` keys after W1/W8 | PASS (`--no-registry`, D5) |

## Linux (Docker `rust:1.99.0-trixie`, Debian 13, WebKitGTK 4.1, Xvfb 1400×900, `--gpus all`)
| # | When | Check | Result |
|---|---|---|---|
| L1 | 19:09–19:11 | `docs/handoff/tooling/linux-check.sh` on the G7 tree | PASS: build ok, clippy `-D warnings` clean, **789 passed / 0 failed / 11 ignored** |
| L2 | 19:12–19:14 | `linux-bundle.sh` (`pnpm -C ui tauri build`, release) | PASS: `vjumpkung-comfyui-desktop_0.1.0_amd64.deb` 16,610,902 B (SHA-256 `14a037d7…aa41f`), `vjumpkung-comfyui-desktop_0.1.0_amd64.AppImage` 120,621,560 B (SHA-256 `0ead974d…72b09`); no rpm produced (Release 1 = deb + AppImage) |
| L3 | 19:27 | `apt-get install ./…deb` in a clean container | PASS: Depends `libwebkit2gtk-4.1-0, libgtk-3-0`; first launch (WebDriver) shows the first-run wizard, default root `/root/.local/share/vjumpkung-comfyui` — **first GUI run of the app on Linux** |
| L4 | 19:28–19:36 | First-run setup, recommended `linux-py313-torch2141-cu130`, Manager on | PASS: run succeeded (final_verify ok); `tools/gdown` marker `gdown=6.4.2`, `python=3.13.16`, lock `2ab5c634…`; app restarted itself after the first run (as on Windows) |
| L5 | 19:37 | Drive download of the same share link | PASS: resolved by gdown, exact total, COMPLETED; file 498,881,336 B, SHA-256 `c187c387…ca934` (identical to Windows). Observation: gdown publishes with mode `0600` and Drive's modification time |
| L6 | 19:38 | Start ComfyUI from the app, `/system_stats` | PASS: linux, Python 3.13.16, torch 2.14.1+cu130, ComfyUI 0.42.1, `cuda:0 NVIDIA GeForce RTX 3060` (GPU via Docker Desktop WSL2, not a native Linux driver). Observation: ComfyUI-Manager logs "[ComfyUI-Manager] PyTorch is not installed" on Linux only (not on Windows); ComfyUI itself runs CUDA |
| L7 | 19:38 | ComfyUI page (xterm log) and "Open ComfyUI" embedded window in WebKitGTK | PASS: page and live log render; embedded window loads the ComfyUI frontend (templates dialog) |
| L8 | 19:39 | AppImage with `APPIMAGE_EXTRACT_AND_RUN=1` on the same root | PASS: launches to the main shell; credential store "SESSION ONLY" (no Secret Service in the container) |

Not established on Linux: a real Linux desktop with a native NVIDIA driver, Secret Service, freedesktop trash on a real
desktop session, WebKitGTK canvas performance, Thai glyph rendering on WebKitGTK, graceful shutdown on SIGTERM (the
container runs closed the app by signal/WebDriver, so "application shutdown finished" was not logged).

## Packaging (Windows, `pnpm -C ui tauri build`, 19:34)
| Artifact | Size | SHA-256 | Authenticode |
|---|---|---|---|
| `target/release/bundle/nsis/vjumpkung-comfyui-desktop_0.1.0_x64-setup.exe` | 9,343,473 B (8.91 MiB) | `7d16c8a1989d3c57583d78b89ad76c0b4831c09bac631614e629e6966e48f4ac` | NotSigned |
| `target/release/bundle/msi/vjumpkung-comfyui-desktop_0.1.0_x64_en-US.msi` | 13,336,576 B (12.72 MiB) | `dc34e2f69def941ce1f9e693d85ca22ed6463275144215e35c21c6d30d97c40a` | NotSigned |
| `target/release/vjumpkung-comfyui-desktop.exe` | 37,448,704 B | `7199abb1b80d101a55af9d884ecad57d197af5871ff151cc600f98eff69e09de` | NotSigned |
Installers were not executed (needs a disposable VM, G02).
