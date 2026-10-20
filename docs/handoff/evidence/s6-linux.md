# Session 6 — first Linux x64 compile and test run (2026-10-20, 17:00–17:05 +07:00)

Environment: Docker Desktop 29.4.0 (WSL2 backend, linux/amd64, 12 CPUs, 16 GB) on the Windows development machine;
image `rust:1.99.0-trixie` (Debian 13.7, rustc 1.99.0 b940084d7 2026-09-28); apt: libwebkit2gtk-4.1-dev (WebKitGTK
2.54.0), libgtk-3-dev, libayatana-appindicator3-dev, librsvg2-dev, libxdo-dev, libssl-dev, git 2.47.3, Python 3.13.5
(`python-is-python3`), xvfb; pinned uv 0.13.1 Linux (SHA-256 checked against `pins.rs` `UV_LINUX`). Script:
`docs/handoff/tooling/linux-check.sh` (copies the repository from a read-only mount, removes the Windows-host
`rust-toolchain.toml` and uses the same Rust 1.99.0, runs everything under `xvfb-run`). Command:
`docker run --rm -v <repo>:/src:ro -v <script dir>:/out -v vjc-linux-target:/target -v vjc-linux-cargo:/usr/local/cargo/registry rust:1.99.0-trixie bash /out/linux-check.sh`.

| Run | Build `--workspace --all-targets` | Clippy `-D warnings` | `cargo test --workspace --no-fail-fast` |
|---|---|---|---|
| 1 (16:56, no `python`/uv in the image) | ok — **first ever Linux compile** of the Linux-only paths (openat pins, `/proc/<pid>/fd` anchors, `renameat2`, `O_NONBLOCK`, trash, Unix BOOT/preflight) | clean | 770 passed, **8 failed**, 10 ignored |
| 2 (17:02, + `python`, uv 0.13.1, test fix) | ok | clean | **778 passed, 0 failed, 10 ignored** |

Run-1 failures: 4 tests need a real `python`/`uv` on PATH (environment, fixed in the script); 4 `environment::tools`
tests had a Unix-only test bug — the fake tool (`/bin/sh`) received `-c "echo uv 0.13.1 (fake build)"` and dash
rejects the unquoted `(` → fixed in `crates/infrastructure/src/environment/tools/tests.rs` (`echo '<text>'`).
Windows-only tests are compiled out on Linux (778 vs 802 Windows tests).

Not established: a Linux GUI/WebKitGTK run of the app, NVIDIA/CUDA on Linux (S1-Linux), Secret Service, trash on a
real desktop, AppImage/.deb packaging. The Linux release gate moves from "never compiled" to PARTIAL.
