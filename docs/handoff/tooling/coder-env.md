## Build environment (mandatory for every cargo command)
Rust is built with the **MSVC** toolchain pinned by `rust-toolchain.toml` (`1.99.0-x86_64-pc-windows-msvc`) into the default `target/` dir. Run every cargo command from the repo root with no extra environment (Git Bash or PowerShell); do not set `RUSTUP_TOOLCHAIN` or `CARGO_TARGET_DIR`. Run `pnpm -C ui build` before `cargo build -p desktop --features tauri/custom-protocol` (it needs `ui/dist`).

**GNU fallback** — only if the MSVC build fails for toolchain reasons (e.g. `LNK1181 kernel32.lib` because the VS C++ tools/Windows SDK are missing); report that you switched. In Git Bash: `source docs/handoff/tooling/gnu-env.sh`; or in PowerShell:

```powershell
$env:RUSTUP_TOOLCHAIN='1.99.0-x86_64-pc-windows-gnu'; $env:CARGO_TARGET_DIR='target-gnu'; $env:PATH='C:\msys64\ucrt64\bin;' + $env:PATH
```

`cargo fmt` works with either toolchain. UI commands: `pnpm -C ui <script>`.

## Repository rules
Read AGENTS.md. Rust: rustfmt, clippy `-D warnings`, no serde/tokio/reqwest/rusqlite/tauri in domain or application, ports via async-trait `Arc<dyn …>`, composition only in crates/desktop/src/state.rs, errors per layer with thiserror. TypeScript: 2-space, single quotes, no semicolons, kebab-case files, Tauri access only via ui/src/shared/ipc/gateway.ts, never hand-edit ui/src/shared/ipc/bindings.ts (regenerate with `cargo test -p desktop --test bindings --locked -- --ignored export_bindings`). Do not commit, push, or touch git history. Do not modify files outside the stated scope; if the plan is underspecified, stop and report the gap instead of improvising.
