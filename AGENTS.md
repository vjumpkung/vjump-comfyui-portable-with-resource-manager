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

- `node scripts/check-all.mjs`: run every CI check locally, in CI order: UI install, lint, typecheck, Vitest and build; Rust format, clippy and tests (including binding drift and ACL checks); the release-protocol desktop build; the architecture checks; and the coverage gates. Use this one command whenever you run tests or checks (see "Before finishing a feature").
- `pnpm -C ui install --frozen-lockfile`: install frontend dependencies.
- `pnpm -C ui tauri dev`: launch desktop development; `pnpm -C ui dev`: frontend only.
- `pnpm -C ui build`: build frontend assets; `pnpm -C ui tauri build`: bundle desktop.
- `pnpm -C ui format` / `pnpm -C ui format:check`: apply or verify Prettier formatting.

CI (`.github/workflows/ci.yml`) runs the same checks on every push and pull request. `.github/workflows/release.yml` builds draft release bundles only when `version` in `crates/desktop/tauri.conf.json` changes on the default branch.

### Before finishing a feature

When you implement a new feature, run `node scripts/check-all.mjs` before calling it done; it must exit 0 and end with `check-all: all N checks passed`. Whenever you run tests or checks, run this script instead of the individual commands or a hand-written chain. Run a single command (one test, one crate, or an ignored test such as the native start-up smoke test) only to reproduce or debug a failure or for a test the script does not cover; it never replaces a full script run.

The script runs the steps of the `check` job in `ci.yml` in order (the UI is built before the Rust steps because the desktop crate embeds `ui/dist`), then the steps of the `core-coverage` job, with `CI=true` as on GitHub Actions; it skips CI's setup steps, takes no options, and stops at the first failing step. A full run takes about 15 minutes on Windows. The coverage gates need `cargo-llvm-cov` 0.9.1 and `llvm-tools-preview`. When you change a step in `ci.yml`, change `steps` in `scripts/check-all.mjs` in the same change, and the reverse.

CI also runs the check job on Ubuntu; if you cannot run Linux locally, say so and call out any `cfg(target_os = "linux")`/`cfg(unix)` code you changed.

## Coding Style & Naming Conventions

Use rustfmt, four-space Rust indentation, and `snake_case` modules/functions. Prettier (`ui/.prettierrc`) owns formatting under `ui/`: two-space indentation, single quotes, omitted semicolons, and Tailwind class order (`prettier-plugin-tailwindcss`, including `cn()` and `cva()` arguments). Use PascalCase components/types and kebab-case feature filenames. ESLint enforces frontend boundaries. Regenerate IPC bindings through the export test; never edit generated files manually.

**Always run `pnpm -C ui format` after editing anything under `ui/`**, before running checks or committing. Do this even if an editor or agent hook formats on save, because shell edits and generators such as `shadcn add` bypass those hooks. `ui/.prettierignore` already excludes the generated `bindings.ts`.

## Testing Guidelines

Keep Rust unit tests beside modules or in `tests.rs`; integration tests belong in each crate's `tests/`. Colocate Vitest/React Testing Library tests as `*.test.ts` or `*.test.tsx`. Line coverage targets: domain ≥90%, application ≥80%, measured with `cargo llvm-cov`. Ignored real setup tests require explicit execution, downloads, and NVIDIA hardware (or a CPU testing profile via `VJ_REAL_PROFILE`).

## Commit & Pull Request Guidelines

Split every change into one commit per area. Claude Code does this with the `commit-by-area` skill (user scope: `~/.claude/skills/commit-by-area`). That also applies when committing is one step of a larger request such as commit-and-PR: make the split commits first, then only push and open the PR, never one all-in-one commit. Without the skill, follow the same rules by hand:

- Order: `build` (root `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`), `domain`, `application`, `infrastructure`, `desktop`, `ui`, `scripts`, `ci` (`.github/`), `docs` (`docs/` and root `*.md`), and a version bump last.
- `ui/src/shared/ipc/bindings.ts` is generated from the desktop crate and goes with `desktop`; `ui/package.json` and `ui/pnpm-lock.yaml` go with `ui`; `Cargo.lock` goes with the last area whose `Cargo.toml` changed.
- Files whose only change is the version (`Cargo.toml`, `Cargo.lock`, `crates/desktop/tauri.conf.json`, `ui/package.json`) form one `chore(release): bump version to X.Y.Z` commit.
- Messages: `type(area): summary`, such as `feat(domain): …` or `fix(ui): …`; `docs:`, `ci:` and `build:` take no scope. Run the formatters above before committing.
- Commit only. Push only when the user asks: a push runs CI, and a version change in `crates/desktop/tauri.conf.json` reaching `main` starts the release workflow.

PRs should explain changes, link issues or requirement IDs, report checks, and include screenshots for UI changes.

## Security & Configuration

Keep credentials in the OS keyring; never place tokens in URLs, logs, or SQLite. Preserve Tauri capability restrictions and review ACL coverage when adding commands.
