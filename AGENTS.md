# Repository Guidelines

## Project Structure & Module Organization

This Rust/Tauri 2 desktop application manages ComfyUI environments, models, and custom nodes.

- `crates/domain`: business types and invariants.
- `crates/application`: use cases and port traits.
- `crates/infrastructure`: system adapters and SQLite `migrations/`.
- `crates/desktop`: Tauri composition, IPC, capabilities, integration tests, and icons.
- `ui/src`: React/TypeScript code organized into `app/`, `features/`, and `shared/`.
- `docs/`: requirements, design, and ADRs; `scripts/`: launcher and architecture checks.

Read `docs/REQUIREMENTS.md` and relevant ADRs before architectural changes. Dependencies point inward; keep serialization outside domain/application and access Tauri through `ui/src/shared/ipc/gateway.ts`.

## Build, Test, and Development Commands

Run from the repository root. Use the Rust toolchain in `rust-toolchain.toml` and pnpm version in `ui/package.json`.

- `pnpm -C ui install --frozen-lockfile`: install frontend dependencies.
- `pnpm -C ui tauri dev`: launch desktop development; `pnpm -C ui dev`: frontend only.
- `pnpm -C ui build`: build frontend assets; `pnpm -C ui tauri build`: bundle desktop.
- `pnpm -C ui lint` / `pnpm -C ui typecheck`: check lint and types.
- `pnpm -C ui test`: run Vitest.
- `cargo test --workspace --locked`: run Rust tests, including binding drift and ACL checks.
- `cargo fmt --all -- --check`: check formatting.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: lint Rust.
- `node scripts/check-arch-deps.mjs`: validate dependency boundaries.

CI (`.github/workflows/ci.yml`) runs these checks on every push and pull request; still run relevant checks locally. `.github/workflows/release.yml` builds draft release bundles only when `version` in `crates/desktop/tauri.conf.json` changes on the default branch.

## Coding Style & Naming Conventions

Use rustfmt, four-space Rust indentation, and `snake_case` modules/functions. Match TypeScript's two-space indentation, single quotes, and omitted semicolons. Use PascalCase components/types and kebab-case feature filenames. ESLint enforces frontend boundaries. Regenerate IPC bindings through the export test; never edit generated files manually.

## Testing Guidelines

Keep Rust unit tests beside modules or in `tests.rs`; integration tests belong in each crate's `tests/`. Colocate Vitest/React Testing Library tests as `*.test.ts` or `*.test.tsx`. Line coverage targets: domain ≥90%, application ≥80%, measured with `cargo llvm-cov`. Ignored real setup tests require explicit execution, downloads, and NVIDIA hardware.

## Commit & Pull Request Guidelines

History contains `initial commit` and `chore: ...` messages. Prefer concise conventional prefixes such as `feat:`, `fix:`, and `docs:`. PRs should explain changes, link issues or requirement IDs, report checks, and include screenshots for UI changes.

## Security & Configuration

Keep credentials in the OS keyring; never place tokens in URLs, logs, or SQLite. Preserve Tauri capability restrictions and review ACL coverage when adding commands.
