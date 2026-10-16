#!/usr/bin/env node
// Runs the Tauri CLI installed in ui/node_modules against crates/desktop.
// The CLI is not in a Cargo or npm workspace root, so the app and frontend directories
// are given through TAURI_APP_PATH and TAURI_FRONTEND_PATH.
import { spawn } from 'node:child_process'
import { createRequire } from 'node:module'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const uiDir = path.join(repoRoot, 'ui')

let cliEntry
try {
  const requireFromUi = createRequire(path.join(uiDir, 'package.json'))
  const packageJson = requireFromUi('@tauri-apps/cli/package.json')
  cliEntry = requireFromUi.resolve(`@tauri-apps/cli/${packageJson.bin.tauri}`)
} catch {
  console.error('Could not find @tauri-apps/cli. Run `pnpm -C ui install` first.')
  process.exit(1)
}

const child = spawn(process.execPath, [cliEntry, ...process.argv.slice(2)], {
  stdio: 'inherit',
  env: {
    ...process.env,
    TAURI_APP_PATH: path.join(repoRoot, 'crates', 'desktop'),
    TAURI_FRONTEND_PATH: uiDir,
  },
})

child.on('error', (error) => {
  console.error(`Failed to start the Tauri CLI: ${error.message}`)
  process.exit(1)
})

child.on('exit', (code, signal) => {
  if (signal) {
    console.error(`The Tauri CLI was terminated by ${signal}.`)
    process.exit(1)
  }
  process.exit(code ?? 1)
})
