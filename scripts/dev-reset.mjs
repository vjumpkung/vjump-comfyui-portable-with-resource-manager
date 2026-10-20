// Developer-only Windows reset. Preview and validate everything before deleting.
import fs from 'node:fs'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { createInterface } from 'node:readline'
import { DatabaseSync } from 'node:sqlite'
import { fileURLToPath } from 'node:url'

const win = path.win32
const defaultRepoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
export const credentialTargets = ['civitai.vjumpkung-comfyui', 'huggingface.vjumpkung-comfyui']
const buildDirectories = ['target', 'target-gnu', 'ui/node_modules', 'ui/dist', 'crates/desktop/gen']
const protectedVariables = [
  'USERPROFILE', 'APPDATA', 'LOCALAPPDATA', 'SystemRoot', 'windir',
  'ProgramFiles', 'ProgramFiles(x86)', 'ProgramData',
]

function object(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function qualified(value) {
  if (typeof value !== 'string') return false
  const normalized = win.normalize(value)
  return (/^[a-z]:\\/i.test(normalized) || /^\\\\[^\\]+\\[^\\]+(?:\\|$)/.test(normalized))
    && !/^\\\\[?.]\\/.test(normalized)
}

function key(value) {
  return win.resolve(value).toLowerCase()
}

export function inside(value, parent) {
  const relative = win.relative(key(parent), key(value))
  return relative === '' || (!win.isAbsolute(relative) && relative !== '..' && !relative.startsWith('..\\'))
}

function stat(value) {
  try {
    return fs.lstatSync(value)
  } catch (error) {
    if (error.code === 'ENOENT' || error.code === 'ENOTDIR') return null
    throw error
  }
}

function noRedirection(value) {
  if (fs.lstatSync(value).isSymbolicLink() || key(fs.realpathSync.native(value)) !== key(value)) {
    throw new Error(`Link redirection is not allowed: ${value}`)
  }
}

export function discoverCandidates(env) {
  if (!qualified(env.APPDATA) || !qualified(env.LOCALAPPDATA)) {
    throw new Error('APPDATA and LOCALAPPDATA must be fully qualified Windows paths.')
  }
  const bootstrap = win.join(env.APPDATA, 'vjumpkung-comfyui', 'bootstrap.json')
  const candidates = new Map()
  function add(value, source) {
    const normalized = win.resolve(value)
    const previous = candidates.get(key(normalized))
    if (previous) {
      previous.sources.push(source)
      previous.qualified &&= qualified(value)
    } else {
      candidates.set(key(normalized), { path: normalized, sources: [source], qualified: qualified(value) })
    }
  }
  if (env.VJUMPKUNG_COMFYUI_HOME) add(env.VJUMPKUNG_COMFYUI_HOME, 'VJUMPKUNG_COMFYUI_HOME override')
  if (stat(bootstrap)) {
    try {
      noRedirection(bootstrap)
      const doc = JSON.parse(fs.readFileSync(bootstrap, 'utf8'))
      if (!object(doc) || doc.schema_version !== 1 || typeof doc.install_root !== 'string'
        || !doc.install_root.trim() || !qualified(doc.install_root)) {
        throw new Error('expected schema_version 1 and a non-empty absolute install_root')
      }
      add(doc.install_root, 'bootstrap.json install_root')
    } catch (error) {
      throw new Error(`Invalid or unreadable bootstrap.json (${bootstrap}): ${error.message}`)
    }
  }
  add(win.join(env.LOCALAPPDATA, 'vjumpkung-comfyui'), 'LOCALAPPDATA default')
  return { candidates: [...candidates.values()], bootstrap }
}

function manifestAt(root) {
  try {
    const doc = JSON.parse(fs.readFileSync(win.join(root, 'install.json'), 'utf8'))
    const strings = ['install_root', 'env_id', 'env_path', 'ownership', 'python_version', 'profile_id', 'run_id', 'activated_at']
    const unsigned = (value, max) => Number.isInteger(value) && value >= 0 && value <= max
    if (!object(doc) || ![1, 2].includes(doc.schema_version)
      || !strings.every((field) => typeof doc[field] === 'string' && doc[field].length > 0)
      || !unsigned(doc.generation, Number.MAX_SAFE_INTEGER) || !unsigned(doc.profile_revision, 0xffffffff)
      || !['app_owned', 'adopted'].includes(doc.ownership)
      || !/^[a-z0-9][a-z0-9-]{0,63}$/.test(doc.env_id) || !/^[a-z0-9-]{1,64}$/.test(doc.run_id)
      || !/^\d+\.\d+\.\d+$/.test(doc.python_version) || doc.python_version.split('.').some((part) => Number(part) > 255)
      || !['comfyui_reference', 'comfyui_commit'].every((field) => doc[field] == null || typeof doc[field] === 'string')
      || (doc.schema_version === 2 && (!object(doc.components) || !Array.isArray(doc.components.accelerators)))) return null
    return doc
  } catch {
    return null
  }
}

function withDatabase(root, read) {
  const db = new DatabaseSync(win.join(root, 'data', 'app.db'), { readOnly: true })
  try {
    return read(db)
  } finally {
    db.close()
  }
}

function hasTable(db, name) {
  return Boolean(db.prepare("SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?").get(name))
}

export function validateRoot(candidate, env, repoRoot) {
  if (!candidate.qualified || !qualified(candidate.path)) throw new Error(`Not a fully qualified absolute path: ${candidate.path}`)
  if (key(candidate.path) === key(win.parse(candidate.path).root)) throw new Error(`Drive/share root is forbidden: ${candidate.path}`)
  for (const protectedPath of [repoRoot, ...protectedVariables.map((name) => env[name])].filter(Boolean)) {
    if (inside(protectedPath, candidate.path)) throw new Error(`Root ${candidate.path} contains protected directory ${protectedPath}`)
  }
  noRedirection(candidate.path)
  if (!fs.lstatSync(candidate.path).isDirectory()) throw new Error(`Unrecognized folder: ${candidate.path}`)
  const manifest = manifestAt(candidate.path)
  let recognized = Boolean(manifest)
  if (!recognized && stat(win.join(candidate.path, 'data', 'app.db'))) {
    recognized = withDatabase(candidate.path, (db) => hasTable(db, 'envs'))
  }
  if (!recognized) throw new Error(`Unrecognized folder: ${candidate.path}`)
  return { ...candidate, manifest }
}

function collectKept(roots, deletionTargets) {
  const kept = new Set()
  function inspect(value, adopted, label, root) {
    if (typeof value !== 'string' || !value) throw new Error(`Invalid ${label} path in ${root}`)
    const resolved = win.resolve(root, value)
    const deleted = deletionTargets.some((target) => inside(resolved, target.path))
    if (adopted && deleted) throw new Error(`Adopted environment is inside a deletion target: ${resolved}`)
    if (!deleted) kept.add(`${label}: ${resolved}`)
  }
  for (const root of roots) {
    if (root.manifest) inspect(root.manifest.env_path, root.manifest.ownership === 'adopted', 'external venv', root.path)
    if (!stat(win.join(root.path, 'data', 'app.db'))) continue
    withDatabase(root.path, (db) => {
      for (const row of db.prepare('SELECT path, owned FROM envs').all()) {
        if (row.owned !== 0 && row.owned !== 1) throw new Error(`Invalid environment ownership in ${root.path}`)
        inspect(row.path, row.owned === 0, 'external venv', root.path)
      }
      if (hasTable(db, 'model_roots')) {
        for (const row of db.prepare('SELECT path FROM model_roots').all()) {
          if (row.path === '') continue
          inspect(row.path, false, 'external model root', root.path)
        }
      }
    })
  }
  return [...kept]
}

export function credentialListed(output, target) {
  const escapedTarget = target.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  const entry = new RegExp(`(?:^|[\\s:=])${escapedTarget}$`, 'i')
  return output.split(/\r?\n/).some((line) => entry.test(line.trim()))
}

function realSystem(env) {
  const systemRoot = env.SystemRoot || env.windir
  const executable = (name) => systemRoot ? win.join(systemRoot, 'System32', name) : name
  function command(exe, args) {
    const result = spawnSync(exe, args, { encoding: 'utf8', windowsHide: true })
    if (result.error) throw result.error
    return result
  }
  const cmdkey = executable('cmdkey.exe')
  const system = {
    listProcesses() {
      const script = "$ErrorActionPreference = 'Stop'; [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new(); @(Get-CimInstance Win32_Process | Select-Object Name, ExecutablePath) | ConvertTo-Json -Compress"
      const result = command(executable('WindowsPowerShell/v1.0/powershell.exe'), ['-NoProfile', '-NonInteractive', '-Command', script])
      if (result.status !== 0) throw new Error(`Could not list processes: ${result.stderr.trim()}`)
      const parsed = JSON.parse(result.stdout.trim() || '[]')
      const processes = Array.isArray(parsed) ? parsed : [parsed]
      if (!processes.every((entry) => object(entry) && typeof entry.Name === 'string'
        && (entry.ExecutablePath === null || typeof entry.ExecutablePath === 'string'))) throw new Error('Invalid process listing')
      return processes
    },
    hasCredential(target) {
      const result = command(cmdkey, [`/list:${target}`])
      if (result.status !== 0) throw new Error(`Could not list credential ${target}`)
      return credentialListed(result.stdout, target)
    },
    deleteCredential(target) {
      command(cmdkey, [`/delete:${target}`])
      if (system.hasCredential(target)) throw new Error(`Credential could not be deleted: ${target}`)
    },
    async readConfirmation(prompt) {
      const lines = createInterface({ input: process.stdin, crlfDelay: Infinity })
      process.stdout.write(prompt)
      try {
        for await (const line of lines) return line
        return null
      } finally {
        lines.close()
      }
    },
  }
  return system
}

async function remainingTargets(targets, system) {
  const remaining = []
  for (const target of targets) {
    try {
      if (target.kind === 'credential' ? await system.hasCredential(target.path) : stat(target.path)) remaining.push(target.path)
    } catch (error) {
      remaining.push(`${target.path} (could not verify: ${error.message})`)
    }
  }
  return remaining
}

export async function runReset({ args = [], env = process.env, repoRoot = defaultRepoRoot, system, write = console.log } = {}) {
  if (args.length > 1 || (args.length === 1 && !['/build', '--build'].includes(args[0]))) {
    write('Usage: scripts\\dev-reset.cmd [/build]')
    return 2
  }
  if (process.platform !== 'win32') {
    write('Windows only')
    return 2
  }
  system ??= realSystem(env)
  let targets
  let bootstrap
  try {
    const discovered = discoverCandidates(env)
    bootstrap = discovered.bootstrap
    const roots = []
    for (const candidate of discovered.candidates) {
      if (!stat(candidate.path)) {
        write(`Not present [${candidate.sources.join(', ')}]: ${candidate.path}`)
        continue
      }
      roots.push({ ...validateRoot(candidate, env, repoRoot), kind: 'root' })
    }
    targets = [...roots]
    if (args.length) {
      for (const directory of buildDirectories) {
        const value = win.resolve(repoRoot, directory)
        if (stat(value)) {
          // A build directory may itself be a link, but its parent must not redirect deletion.
          noRedirection(win.dirname(value))
          targets.push({ path: value, kind: 'build', sources: ['/build generated directory'] })
        }
      }
    }
    if (stat(bootstrap)) targets.push({ path: bootstrap, kind: 'bootstrap', sources: ['APPDATA bootstrap pointer'] })
    const webview = win.join(env.LOCALAPPDATA, 'dev.vjump.comfyui-desktop')
    if (stat(webview)) {
      noRedirection(webview)
      targets.push({ path: webview, kind: 'webview', sources: ['LOCALAPPDATA WebView2 data'] })
    }
    const kept = collectKept(roots, targets)
    for (const target of credentialTargets) {
      if (await system.hasCredential(target)) targets.push({ path: target, kind: 'credential', sources: ['Windows stored token'] })
    }
    for (const process of await system.listProcesses()) {
      if (process.Name?.toLowerCase() === 'vjumpkung-comfyui-desktop.exe'
        || (process.ExecutablePath && targets.some((target) => target.kind !== 'credential' && inside(process.ExecutablePath, target.path)))) {
        throw new Error(`Close the running process before resetting: ${process.Name} (${process.ExecutablePath || 'path unavailable'})`)
      }
    }
    if (!targets.length) {
      write('Nothing to clean.')
      return 0
    }
    write('Permanently DELETE:')
    for (const target of targets) write(`  [${target.sources.join(', ')}] ${target.path}`)
    if (roots.length) write('Each install root includes the ComfyUI checkout with in-root models, custom nodes, inputs, outputs and workflows; environments, tools, database, logs, caches and downloads.')
    if (targets.some((target) => target.kind === 'bootstrap')) write(`  ${win.dirname(bootstrap)} is removed only if empty after deleting bootstrap.json.`)
    write('KEPT:')
    for (const value of kept) write(`  ${value}`)
    write('  External venvs and external model folders outside the deletion targets')
    write('  HKCU\\Software\\Python\\Astral')
    write('  Tracked repository files (including ui\\src\\shared\\ipc\\bindings.ts)')
    if (await system.readConfirmation('Type YES to permanently delete everything listed above: ') !== 'YES') {
      write('Cancelled. Nothing was changed.')
      return 1
    }
  } catch (error) {
    write(`Reset refused. Nothing was changed. ${error.message}`)
    return 1
  }

  try {
    for (const target of targets) {
      if (target.kind === 'credential') {
        await system.deleteCredential(target.path)
        if (await system.hasCredential(target.path)) throw new Error(`Credential still present: ${target.path}`)
      } else {
        if (target.kind === 'bootstrap') {
          fs.unlinkSync(target.path)
          const directory = win.dirname(target.path)
          if (fs.readdirSync(directory).length === 0) fs.rmdirSync(directory)
        } else {
          const removeTree = system.removeTree ?? ((value) => fs.rmSync(value, { recursive: true, force: true, maxRetries: 3 }))
          await removeTree(target.path)
        }
        if (stat(target.path)) throw new Error(`Target still present: ${target.path}`)
      }
      write(`Removed: ${target.path}`)
    }
    const remaining = await remainingTargets(targets, system)
    if (remaining.length) throw new Error('Some targets remain after deletion.')
    write(`Reset complete. Removed ${targets.length} target(s).`)
    return 0
  } catch (error) {
    write(`Reset stopped: ${error.message}`)
    const remaining = await remainingTargets(targets, system)
    write('Remaining targets (retry after fixing the failure):')
    for (const value of remaining) write(`  ${value}`)
    return 1
  }
}

if (process.argv[1] && path.resolve(process.argv[1]).toLowerCase() === fileURLToPath(import.meta.url).toLowerCase()) {
  process.exitCode = await runReset({ args: process.argv.slice(2) })
}
