#!/usr/bin/env node
// Runs every CI check locally with one command, in a fixed order and with no options:
// the steps of the `check` job in .github/workflows/ci.yml, then the steps of the
// `core-coverage` job. CI's setup steps (toolchain, system libraries, cargo-llvm-cov) are
// not run. Stops at the first failing step. Keep `steps` in sync with ci.yml.
import { spawnSync } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

// Each entry is one ci.yml step: its `name` and its exact `run` command.
export const steps = [
  { name: 'Install UI dependencies', command: 'pnpm -C ui install --frozen-lockfile' },
  { name: 'Lint UI', command: 'pnpm -C ui lint' },
  { name: 'Typecheck UI', command: 'pnpm -C ui typecheck' },
  { name: 'Test UI', command: 'pnpm -C ui test' },
  { name: 'Build UI', command: 'pnpm -C ui build' },
  { name: 'Rust format', command: 'cargo fmt --all -- --check' },
  { name: 'Rust clippy', command: 'cargo clippy --workspace --all-targets --locked -- -D warnings' },
  { name: 'Rust tests', command: 'cargo test --workspace --locked' },
  { name: 'Build desktop (release assets embedded)', command: 'cargo build -p desktop --locked --features tauri/custom-protocol' },
  { name: 'Architecture dependency tests', command: 'node --test scripts/check-arch-deps.test.mjs' },
  { name: 'Architecture dependency check', command: 'node scripts/check-arch-deps.mjs' },
  { name: 'Domain coverage (90 percent)', command: 'cargo llvm-cov -p domain --locked --fail-under-lines 90' },
  { name: 'Application coverage (80 percent)', command: 'cargo llvm-cov -p application --locked --fail-under-lines 80' },
]

export function runSteps(list, cwd) {
  for (const [index, { name, command }] of list.entries()) {
    console.log(`\n==> [${index + 1}/${list.length}] ${name}: ${command}`)
    const started = Date.now()
    // A fixed command string run through the shell (cmd.exe on Windows, sh elsewhere), because
    // pnpm is a .cmd shim on Windows. GitHub Actions sets CI=true for every step.
    const result = spawnSync(command, {
      cwd,
      stdio: 'inherit',
      shell: true,
      windowsHide: true,
      env: { ...process.env, CI: 'true' },
    })
    if (result.status !== 0) {
      const reason = result.error?.message ?? result.signal ?? `exit code ${result.status}`
      console.error(`\ncheck-all: FAILED at "${name}" (${reason})`)
      return 1
    }
    console.log(`<== ${name}: passed in ${Math.round((Date.now() - started) / 1000)}s`)
  }
  console.log(`\ncheck-all: all ${list.length} checks passed`)
  return 0
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.length > 2) {
    console.error('Usage: node scripts/check-all.mjs (takes no arguments)')
    process.exitCode = 2
  } else {
    process.exitCode = runSteps(steps, repoRoot)
  }
}
