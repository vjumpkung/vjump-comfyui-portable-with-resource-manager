// All resets use disposable temp fixtures and fake process/credential/prompt actions.
import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { DatabaseSync } from 'node:sqlite'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

import { credentialListed, credentialTargets, discoverCandidates, inside, runReset, validateRoot } from './dev-reset.mjs'

const windows = { skip: process.platform !== 'win32' }
const migrations = new URL('../crates/infrastructure/migrations/', import.meta.url)

function writeFile(value, content = 'fixture content') {
  fs.mkdirSync(path.dirname(value), { recursive: true })
  fs.writeFileSync(value, content)
}

function manifest(root, changes = {}) {
  return {
    schema_version: 2,
    install_root: root,
    generation: 1,
    env_id: 'env-fixture',
    env_path: path.join(root, 'envs', 'env-fixture'),
    ownership: 'app_owned',
    python_version: '3.12.9',
    profile_id: 'win-fixture',
    profile_revision: 1,
    comfyui_reference: null,
    comfyui_commit: null,
    run_id: 'run-fixture',
    activated_at: '2026-10-20T00:00:00Z',
    components: { manager: null, accelerators: [] },
    ...changes,
  }
}

function appRoot(value, changes) {
  writeFile(path.join(value, 'install.json'), JSON.stringify(manifest(value, changes)))
  writeFile(path.join(value, 'ComfyUI', 'output', 'keep-until-confirmed.txt'))
  return value
}

function database(root, envs = [], models = []) {
  const value = path.join(root, 'data', 'app.db')
  fs.mkdirSync(path.dirname(value), { recursive: true })
  const db = new DatabaseSync(value)
  try {
    for (const migration of fs.readdirSync(migrations).filter((name) => name.endsWith('.sql')).sort()) {
      db.exec(fs.readFileSync(new URL(migration, migrations), 'utf8'))
    }
    const insert = db.prepare(`INSERT INTO envs
      (id, path, owned, source, python_version, profile_id, profile_revision, pins_json,
       authorized_location, state, created_at, updated_at)
      VALUES (?, ?, ?, 'custom_venv', '3.12.9', 'fixture', 1, '{}', ?, 'verified', 'now', 'now')`)
    envs.forEach((env, index) => insert.run(`env-${index}`, env.path, env.owned, env.path))
    models.forEach((value, index) => db.prepare("INSERT INTO model_roots (id, path, status) VALUES (?, ?, 'ready')").run(`root-${index}`, value))
  } finally {
    db.close()
  }
}

function snapshot(root) {
  const entries = []
  function visit(value) {
    const info = fs.lstatSync(value)
    const relative = path.relative(root, value)
    if (info.isSymbolicLink()) entries.push([relative, 'link', fs.readlinkSync(value)])
    else if (info.isDirectory()) {
      entries.push([relative, 'directory'])
      for (const name of fs.readdirSync(value).sort()) visit(path.join(value, name))
    } else entries.push([relative, 'file', fs.readFileSync(value).toString('base64'), info.mode])
  }
  visit(root)
  return entries
}

function fixture(t) {
  const base = fs.mkdtempSync(path.join(os.tmpdir(), 'vjump-dev-reset-'))
  t.after(() => fs.rmSync(base, { recursive: true, force: true, maxRetries: 3 }))
  const env = {
    USERPROFILE: path.join(base, 'user'),
    APPDATA: path.join(base, 'user', 'roaming'),
    LOCALAPPDATA: path.join(base, 'user', 'local'),
    SystemRoot: path.join(base, 'windows'),
    windir: path.join(base, 'windows'),
    ProgramFiles: path.join(base, 'program files'),
    'ProgramFiles(x86)': path.join(base, 'program files x86'),
    ProgramData: path.join(base, 'program data'),
  }
  for (const value of Object.values(env)) fs.mkdirSync(value, { recursive: true })
  const fx = {
    base, env,
    repoRoot: path.join(base, 'repo'),
    root: path.join(env.LOCALAPPDATA, 'vjumpkung-comfyui'),
    bootstrap: path.join(env.APPDATA, 'vjumpkung-comfyui', 'bootstrap.json'),
    webview: path.join(env.LOCALAPPDATA, 'dev.vjump.comfyui-desktop'),
    credentials: new Set(),
    deletes: [],
    processes: [],
    confirmation: 'no',
    prompts: [],
    output: [],
    bootstrapTo(root) {
      writeFile(fx.bootstrap, JSON.stringify({ schema_version: 1, install_root: root }))
    },
    async run(args = []) {
      // Never inherit any real machine env, repoRoot or system action.
      assert.ok(inside(fx.repoRoot, base))
      for (const value of Object.values(env)) assert.ok(inside(value, base))
      return runReset({ args, env, repoRoot: fx.repoRoot, system: fx.system, write: (line) => fx.output.push(line) })
    },
    text() { return fx.output.join('\n') },
  }
  fs.mkdirSync(fx.repoRoot)
  fx.system = {
    listProcesses() { return fx.processes },
    hasCredential(target) {
      assert.ok(credentialTargets.includes(target))
      return fx.credentials.has(target)
    },
    deleteCredential(target) {
      assert.ok(credentialTargets.includes(target))
      fx.deletes.push(target)
      fx.credentials.delete(target)
    },
    readConfirmation(prompt) {
      fx.prompts.push(prompt)
      return fx.confirmation
    },
  }
  return fx
}

async function unchanged(fx, pattern) {
  const before = snapshot(fx.base)
  const credentials = [...fx.credentials]
  assert.equal(await fx.run(), 1)
  assert.deepEqual(snapshot(fx.base), before)
  assert.deepEqual([...fx.credentials], credentials)
  assert.deepEqual(fx.deletes, [])
  assert.deepEqual(fx.prompts, [])
  assert.match(fx.text(), pattern)
}

test('Windows path comparisons use whole components and accept UNC paths', () => {
  assert.ok(inside('C:\\App\\envs\\one', 'c:\\app'))
  assert.ok(inside('C:\\APP', 'c:\\app'))
  assert.equal(inside('C:\\app-other', 'C:\\app'), false)
  assert.equal(inside('D:\\app', 'C:\\app'), false)
  assert.ok(inside('\\\\server\\share\\app\\env', '\\\\SERVER\\SHARE\\app'))
})

test('discovery independently finds override, bootstrap and default roots', windows, (t) => {
  const fx = fixture(t)
  fx.env.VJUMPKUNG_COMFYUI_HOME = path.join(fx.base, 'override')
  fx.bootstrapTo(path.join(fx.base, 'bootstrap-root'))
  const { candidates } = discoverCandidates(fx.env)
  assert.deepEqual(candidates.map((candidate) => candidate.path), [fx.env.VJUMPKUNG_COMFYUI_HOME, path.join(fx.base, 'bootstrap-root'), fx.root])
  assert.ok(candidates.every((candidate) => candidate.sources.length === 1))
})

test('discovery deduplicates normalized paths case-insensitively and merges sources', windows, (t) => {
  const fx = fixture(t)
  fx.env.VJUMPKUNG_COMFYUI_HOME = `${fx.root.toUpperCase()}\\.`
  fx.bootstrapTo(fx.root)
  const { candidates } = discoverCandidates(fx.env)
  assert.equal(candidates.length, 1)
  assert.equal(candidates[0].sources.length, 3)
})

test('discovery accepts a fully qualified UNC candidate with forward slashes without accessing it', windows, (t) => {
  const fx = fixture(t)
  fx.env.VJUMPKUNG_COMFYUI_HOME = '//server/share/app-root'
  const candidate = discoverCandidates(fx.env).candidates[0]
  assert.equal(candidate.path, '\\\\server\\share\\app-root')
  assert.equal(candidate.qualified, true)
})

for (const [name, content] of [
  ['bad JSON', '{'],
  ['wrong schema_version', JSON.stringify({ schema_version: 2, install_root: 'C:\\unused' })],
  ['empty install_root', JSON.stringify({ schema_version: 1, install_root: '' })],
  ['relative install_root', JSON.stringify({ schema_version: 1, install_root: 'relative' })],
  ['non-object', '[]'],
]) {
  test(`malformed bootstrap (${name}) aborts without changes despite an override`, windows, async (t) => {
    const fx = fixture(t)
    fx.env.VJUMPKUNG_COMFYUI_HOME = appRoot(path.join(fx.base, 'override'))
    appRoot(fx.root)
    writeFile(fx.bootstrap, content)
    await unchanged(fx, /Invalid or unreadable bootstrap\.json/)
  })
}

test('unreadable bootstrap (a directory) aborts without changes', windows, async (t) => {
  const fx = fixture(t)
  appRoot(fx.root)
  fs.mkdirSync(fx.bootstrap, { recursive: true })
  await unchanged(fx, /Invalid or unreadable bootstrap\.json/)
})

for (const protectedName of ['repoRoot', 'USERPROFILE', 'APPDATA', 'LOCALAPPDATA', 'SystemRoot', 'windir', 'ProgramFiles', 'ProgramFiles(x86)', 'ProgramData']) {
  test(`candidate containing protected ${protectedName} aborts without changes`, windows, async (t) => {
    const fx = fixture(t)
    const candidate = appRoot(path.join(fx.base, 'ancestor'))
    if (protectedName === 'repoRoot') fx.repoRoot = path.join(candidate, 'repo')
    else fx.env[protectedName] = path.join(candidate, 'protected')
    fs.mkdirSync(protectedName === 'repoRoot' ? fx.repoRoot : fx.env[protectedName], { recursive: true })
    fx.env.VJUMPKUNG_COMFYUI_HOME = candidate
    await unchanged(fx, /contains protected directory/)
  })
}

test('equal protected directory, drive/share roots and relative candidates are rejected before filesystem access', windows, (t) => {
  const fx = fixture(t)
  const before = snapshot(fx.base)
  const validate = (value, isQualified = true) => validateRoot({ path: value, qualified: isQualified, sources: ['test'] }, fx.env, fx.repoRoot)
  assert.throws(() => validate(fx.repoRoot), /protected directory/)
  assert.throws(() => validate('C:\\'), /Drive\/share root/)
  assert.throws(() => validate('\\\\server\\share\\'), /Drive\/share root/)
  fx.env.VJUMPKUNG_COMFYUI_HOME = 'relative-root'
  const relative = discoverCandidates(fx.env).candidates[0]
  assert.equal(relative.qualified, false)
  assert.throws(() => validateRoot(relative, fx.env, fx.repoRoot), /fully qualified absolute/)
  assert.deepEqual(snapshot(fx.base), before)
})

test('a candidate junction aborts without following it', windows, async (t) => {
  const fx = fixture(t)
  const sentinel = appRoot(path.join(fx.base, 'sentinel'))
  fx.env.VJUMPKUNG_COMFYUI_HOME = path.join(fx.base, 'linked-root')
  fs.symlinkSync(sentinel, fx.env.VJUMPKUNG_COMFYUI_HOME, 'junction')
  await unchanged(fx, /Link redirection/)
})

test('a candidate beneath a junction aborts on canonical-path mismatch', windows, async (t) => {
  const fx = fixture(t)
  const sentinel = appRoot(path.join(fx.base, 'sentinel', 'root'))
  const alias = path.join(fx.base, 'alias')
  fs.symlinkSync(path.dirname(sentinel), alias, 'junction')
  fx.env.VJUMPKUNG_COMFYUI_HOME = path.join(alias, 'root')
  await unchanged(fx, /Link redirection/)
})

test('an unrecognized folder or incomplete manifest aborts without changes', windows, async (t) => {
  const fx = fixture(t)
  writeFile(path.join(fx.root, 'install.json'), JSON.stringify({ schema_version: 1 }))
  writeFile(path.join(fx.root, 'sentinel'))
  await unchanged(fx, /Unrecognized folder/)
})

test('a later invalid candidate prevents deletion of earlier valid roots', windows, async (t) => {
  const fx = fixture(t)
  fx.env.VJUMPKUNG_COMFYUI_HOME = appRoot(path.join(fx.base, 'override'))
  fs.mkdirSync(fx.root)
  await unchanged(fx, /Unrecognized folder/)
})

test('database envs table recognizes an install root without install.json', windows, async (t) => {
  const fx = fixture(t)
  database(fx.root)
  fx.confirmation = 'YES'
  assert.equal(await fx.run(), 0)
  assert.equal(fs.existsSync(fx.root), false)
})

for (const location of ['inside root', 'equal root', 'another root', 'WebView data', 'build directory']) {
  test(`adopted venv ${location} aborts before confirmation`, windows, async (t) => {
    const fx = fixture(t)
    appRoot(fx.root)
    let adopted = path.join(fx.root, 'adopted', 'keep-me')
    if (location === 'equal root') adopted = fx.root
    if (location === 'another root') adopted = fx.env.VJUMPKUNG_COMFYUI_HOME = appRoot(path.join(fx.base, 'override'))
    if (location === 'WebView data') adopted = fx.webview
    if (location === 'build directory') adopted = path.join(fx.repoRoot, 'target', 'adopted')
    fs.mkdirSync(adopted, { recursive: true })
    writeFile(path.join(adopted, 'sentinel'))
    database(fx.root, [{ path: adopted, owned: 0 }])
    if (location === 'build directory') {
      const before = snapshot(fx.base)
      assert.equal(await fx.run(['/build']), 1)
      assert.deepEqual(snapshot(fx.base), before)
      assert.deepEqual(fx.prompts, [])
      assert.match(fx.text(), /Adopted environment/)
    } else await unchanged(fx, /Adopted environment/)
  })
}

test('an adopted manifest environment is protected even when the database is missing', windows, async (t) => {
  const fx = fixture(t)
  appRoot(fx.root, { ownership: 'adopted' })
  await unchanged(fx, /Adopted environment/)
})

test('external envs and model roots are listed as kept and remain unchanged', windows, async (t) => {
  const fx = fixture(t)
  appRoot(fx.root)
  const external = path.join(fx.base, 'external adopted venv')
  const owned = `${fx.root}-sibling`
  const models = path.join(fx.base, 'external models')
  for (const value of [external, owned, models]) writeFile(path.join(value, 'sentinel'), value)
  database(fx.root, [{ path: external, owned: 0 }, { path: owned, owned: 1 }], [models, path.join(fx.root, 'ComfyUI', 'models')])
  fx.confirmation = 'YES'
  assert.equal(await fx.run(), 0)
  for (const value of [external, owned, models]) {
    assert.ok(fx.text().includes(value))
    assert.equal(fs.readFileSync(path.join(value, 'sentinel'), 'utf8'), value)
  }
  assert.match(fx.text(), /KEPT:[\s\S]*HKCU\\Software\\Python\\Astral[\s\S]*Tracked repository files/)
})

test('empty scan model roots are ignored while external model roots are kept', windows, async (t) => {
  const fx = fixture(t)
  appRoot(fx.root)
  const models = path.join(fx.base, 'external models')
  writeFile(path.join(models, 'sentinel'), 'keep external models')
  database(fx.root, [], [models])
  const db = new DatabaseSync(path.join(fx.root, 'data', 'app.db'))
  try {
    db.exec("INSERT INTO model_roots (id, path, status) VALUES ('models', '', 'ready')")
  } finally {
    db.close()
  }
  fx.confirmation = 'YES'
  assert.equal(await fx.run(), 0, fx.text())
  assert.deepEqual(fx.output.filter((line) => line.startsWith('  external model root: ')), [
    `  external model root: ${models}`,
  ])
  assert.equal(fs.readFileSync(path.join(models, 'sentinel'), 'utf8'), 'keep external models')
})

for (const process of [
  { Name: 'VJUMPKUNG-COMFYUI-DESKTOP.EXE', ExecutablePath: null },
  { Name: 'python.exe', inRoot: true },
]) {
  test(`running ${process.Name} refuses reset without changes`, windows, async (t) => {
    const fx = fixture(t)
    appRoot(fx.root)
    fx.processes = [{ Name: process.Name, ExecutablePath: process.inRoot ? path.join(fx.root, 'envs', 'python.exe') : process.ExecutablePath }]
    await unchanged(fx, /Close the running process/)
  })
}

test('a process in a /build target refuses reset without changes', windows, async (t) => {
  const fx = fixture(t)
  const build = path.join(fx.repoRoot, 'target')
  writeFile(path.join(build, 'debug', 'fixture.exe'))
  fx.processes = [{ Name: 'fixture.exe', ExecutablePath: path.join(build, 'debug', 'fixture.exe') }]
  const before = snapshot(fx.base)
  assert.equal(await fx.run(['/build']), 1)
  assert.deepEqual(snapshot(fx.base), before)
  assert.deepEqual(fx.prompts, [])
  assert.match(fx.text(), /Close the running process/)
})

for (const confirmation of ['no', 'yes', null, 'YES ']) {
  test(`confirmation ${JSON.stringify(confirmation)} cancels without changes`, windows, async (t) => {
    const fx = fixture(t)
    appRoot(fx.root)
    fx.bootstrapTo(fx.root)
    writeFile(path.join(fx.webview, 'data'))
    fx.credentials = new Set(credentialTargets)
    fx.confirmation = confirmation
    const before = snapshot(fx.base)
    assert.equal(await fx.run(), 1)
    assert.deepEqual(snapshot(fx.base), before)
    assert.deepEqual([...fx.credentials], credentialTargets)
    assert.deepEqual(fx.deletes, [])
    assert.deepEqual(fx.prompts, ['Type YES to permanently delete everything listed above: '])
    assert.match(fx.text(), /Cancelled\. Nothing was changed\./)
  })
}

test('exact YES deletes all roots, bootstrap, WebView data and both fake credentials in order', windows, async (t) => {
  const fx = fixture(t)
  appRoot(fx.root)
  fx.env.VJUMPKUNG_COMFYUI_HOME = appRoot(path.join(fx.base, 'override'))
  const bootRoot = appRoot(path.join(fx.base, 'bootstrap-root'))
  fx.bootstrapTo(bootRoot)
  writeFile(path.join(fx.webview, 'data'))
  fx.credentials = new Set(credentialTargets)
  fx.confirmation = 'YES'
  assert.equal(await fx.run(), 0)
  const removed = fx.output.filter((line) => line.startsWith('Removed: ')).map((line) => line.slice(9))
  assert.deepEqual(removed, [fx.env.VJUMPKUNG_COMFYUI_HOME, bootRoot, fx.root, fx.bootstrap, fx.webview, ...credentialTargets])
  for (const value of removed.slice(0, -2)) assert.equal(fs.existsSync(value), false)
  assert.equal(fs.existsSync(path.dirname(fx.bootstrap)), false)
  assert.deepEqual(fx.deletes, credentialTargets)
  assert.equal(fx.credentials.size, 0)
  assert.match(fx.text(), /ComfyUI checkout with in-root models, custom nodes, inputs, outputs and workflows/)
  assert.match(fx.text(), /Reset complete/)
})

test('a nonempty bootstrap directory keeps its other files', windows, async (t) => {
  const fx = fixture(t)
  appRoot(fx.root)
  fx.bootstrapTo(fx.root)
  const keep = path.join(path.dirname(fx.bootstrap), 'other.txt')
  writeFile(keep, 'keep this')
  fx.confirmation = 'YES'
  assert.equal(await fx.run(), 0)
  assert.equal(fs.existsSync(fx.bootstrap), false)
  assert.equal(fs.readFileSync(keep, 'utf8'), 'keep this')
})

test('a WebView junction aborts without changing its target or app state', windows, async (t) => {
  const fx = fixture(t)
  appRoot(fx.root)
  const sentinel = path.join(fx.base, 'sentinel')
  writeFile(path.join(sentinel, 'keep'))
  fs.symlinkSync(sentinel, fx.webview, 'junction')
  await unchanged(fx, /Link redirection/)
})

test('rmSync handles read-only, hidden/system, Unicode, metacharacters, long paths and internal junctions', windows, async (t) => {
  const fx = fixture(t)
  appRoot(fx.root)
  const sentinel = path.join(fx.base, 'outside sentinel')
  writeFile(path.join(sentinel, 'keep.txt'), 'unchanged sentinel')
  const pack = path.join(fx.root, 'ComfyUI', '.git', 'objects', 'pack', 'fixture.pack')
  writeFile(pack)
  fs.chmodSync(pack, 0o444)
  const hidden = path.join(fx.root, 'hidden-system.txt')
  writeFile(hidden)
  const attribOutput = path.join(fx.base, 'attrib-output.txt')
  function attrib(args) {
    const fd = fs.openSync(attribOutput, 'w')
    try {
      const result = spawnSync(path.join(process.env.SystemRoot, 'System32', 'attrib.exe'), args, { stdio: ['ignore', fd, fd], windowsHide: true })
      assert.ifError(result.error)
      assert.equal(result.status, 0, fs.readFileSync(attribOutput, 'utf8'))
      return fs.readFileSync(attribOutput, 'utf8')
    } finally {
      fs.closeSync(fd)
    }
  }
  attrib(['+H', '+S', hidden])
  const attributes = attrib([hidden])
  assert.match(attributes.slice(0, attributes.indexOf(hidden)), /S/)
  assert.match(attributes.slice(0, attributes.indexOf(hidden)), /H/)
  writeFile(path.join(fx.root, 'ไทย 日本語 space & ^ % ! (parentheses)', 'file & ^ % ! ().txt'))
  const longFile = path.join(fx.root, ...Array(7).fill('long-component-abcdefghijklmnopqrstuvwxyz'), 'file.txt')
  assert.ok(longFile.length > 260)
  writeFile(longFile)
  fs.symlinkSync(sentinel, path.join(fx.root, 'outside-junction'), 'junction')
  fs.symlinkSync(path.join(fx.base, 'missing-target'), path.join(fx.root, 'dangling-junction'), 'junction')
  fx.confirmation = 'YES'
  assert.equal(await fx.run(), 0, fx.text())
  assert.equal(fs.existsSync(fx.root), false)
  assert.equal(fs.readFileSync(path.join(sentinel, 'keep.txt'), 'utf8'), 'unchanged sentinel')
  assert.deepEqual(fs.readdirSync(sentinel), ['keep.txt'])
})

test('an internal file symlink is deleted while its external file survives', windows, async (t) => {
  const fx = fixture(t)
  appRoot(fx.root)
  const sentinel = path.join(fx.base, 'external-file.txt')
  writeFile(sentinel, 'unchanged file')
  try {
    fs.symlinkSync(sentinel, path.join(fx.root, 'file-link'), 'file')
  } catch (error) {
    if (error.code !== 'EPERM') throw error
    t.skip('File symlink creation requires Windows privileges (EPERM); junction cases run separately.')
    return
  }
  fx.confirmation = 'YES'
  assert.equal(await fx.run(), 0)
  assert.equal(fs.existsSync(fx.root), false)
  assert.equal(fs.readFileSync(sentinel, 'utf8'), 'unchanged file')
})

test('already-clean state reports nothing to clean without a prompt', windows, async (t) => {
  const fx = fixture(t)
  const before = snapshot(fx.base)
  assert.equal(await fx.run(), 0)
  assert.deepEqual(snapshot(fx.base), before)
  assert.deepEqual(fx.prompts, [])
  assert.match(fx.text(), /Not present \[LOCALAPPDATA default\]/)
  assert.match(fx.text(), /Nothing to clean\./)
})

for (const flag of ['/build', '--build']) {
  test(`${flag} removes only generated directories in a fake repo and keeps bindings.ts`, windows, async (t) => {
    const fx = fixture(t)
    const directories = ['target', 'target-gnu', 'ui/node_modules', 'ui/dist', 'crates/desktop/gen'].map((value) => path.join(fx.repoRoot, value))
    for (const value of directories) writeFile(path.join(value, 'fixture'))
    const bindings = path.join(fx.repoRoot, 'ui', 'src', 'shared', 'ipc', 'bindings.ts')
    writeFile(bindings, '// tracked fixture')
    fx.confirmation = 'YES'
    assert.equal(await fx.run([flag]), 0)
    for (const value of directories) assert.equal(fs.existsSync(value), false)
    assert.equal(fs.readFileSync(bindings, 'utf8'), '// tracked fixture')
    assert.match(fx.text(), /Tracked repository files/)
  })
}

test('without /build generated directories are kept', windows, async (t) => {
  const fx = fixture(t)
  const value = path.join(fx.repoRoot, 'target', 'fixture')
  writeFile(value)
  assert.equal(await fx.run(), 0)
  assert.ok(fs.existsSync(value))
  assert.deepEqual(fx.prompts, [])
})

test('credentialListed matches cmdkey entry boundaries without matching headers', () => {
  const missing = 'Currently stored credentials for nonexistent.vjumpkung-comfyui:\r\n\r\n* NONE *\r\n'
  const existing = '\r\nCurrently stored credentials for civitai.vjumpkung-comfyui:\r\n\r\n'
    + '    Target: civitai.vjumpkung-comfyui\r\n    Type: Generic \r\n    User: civitai\r\n'
  const fullListing = '    Target: LegacyGeneric:target=civitai.vjumpkung-comfyui\r\n'
  const target = credentialTargets[0]
  for (const [output, query, expected] of [
    [missing, 'nonexistent.vjumpkung-comfyui', false],
    [existing, target, true],
    [fullListing, target, true],
    ['    Target: xcivitai.vjumpkung-comfyui\r\n', target, false],
    ['Target: civitai.vjumpkung-comfyui2\r\n', target, false],
    ['Target: civitaiXvjumpkung-comfyui\r\n', target, false],
    [target, target, true],
    [`Target:${target} \t\r\n`, target, true],
  ]) {
    assert.equal(credentialListed(output, query), expected, output)
    assert.equal(credentialListed(output.toLowerCase(), query.toUpperCase()), expected, output)
    assert.equal(credentialListed(output.toUpperCase(), query.toLowerCase()), expected, output)
  }
})

test('credential matching uses target strings independently of localized labels', () => {
  assert.equal(credentialListed(`เป้าหมาย: LegacyGeneric:target=${credentialTargets[0]}\nชนิด: Generic`, credentialTargets[0]), true)
  assert.equal(credentialListed('Keine gespeicherten Anmeldeinformationen', credentialTargets[0]), false)
})

test('credentials already absent at deletion time are fine', windows, async (t) => {
  const fx = fixture(t)
  fx.credentials = new Set(credentialTargets)
  fx.system.readConfirmation = () => {
    fx.credentials.clear()
    return 'YES'
  }
  assert.equal(await fx.run(), 0)
  assert.deepEqual(fx.deletes, credentialTargets)
})

test('root removal failure stops before bootstrap, WebView and credentials; a retry completes', windows, async (t) => {
  const fx = fixture(t)
  appRoot(fx.root)
  fx.bootstrapTo(fx.root)
  writeFile(path.join(fx.webview, 'data'))
  fx.credentials = new Set(credentialTargets)
  fx.confirmation = 'YES'
  fx.system.removeTree = (value) => {
    assert.ok(inside(value, fx.base))
    throw new Error('fixture root is locked')
  }
  const before = snapshot(fx.base)
  assert.equal(await fx.run(), 1)
  assert.deepEqual(snapshot(fx.base), before)
  assert.deepEqual(fx.deletes, [])
  const remaining = fx.text().split('Remaining targets')[1]
  for (const value of [fx.root, fx.bootstrap, fx.webview, ...credentialTargets]) assert.ok(remaining.includes(value))
  delete fx.system.removeTree
  assert.equal(await fx.run(), 0)
  assert.equal(fs.existsSync(fx.root), false)
  assert.equal(fx.credentials.size, 0)
})

test('credential delete failure is verified, lists remaining targets and succeeds on retry', windows, async (t) => {
  const fx = fixture(t)
  appRoot(fx.root)
  fx.credentials = new Set(credentialTargets)
  fx.confirmation = 'YES'
  const realFakeDelete = fx.system.deleteCredential
  fx.system.deleteCredential = () => {} // A silent failure must still be detected.
  assert.equal(await fx.run(), 1)
  assert.equal(fs.existsSync(fx.root), false)
  assert.match(fx.text(), /Credential still present/)
  const remaining = fx.text().split('Remaining targets')[1]
  for (const target of credentialTargets) assert.ok(remaining.includes(target))
  fx.system.deleteCredential = realFakeDelete
  assert.equal(await fx.run(), 0)
  assert.equal(fx.credentials.size, 0)
})

test('unknown or duplicate arguments return usage and exit 2 without actions', windows, async (t) => {
  const fx = fixture(t)
  fx.system = new Proxy({}, { get() { throw new Error('No system actions allowed') } })
  for (const args of [['/force'], ['/build', '/build'], ['--yes']]) assert.equal(await fx.run(args), 2)
  assert.match(fx.text(), /Usage: scripts\\dev-reset\.cmd \[\/build\]/)
})

test('the cmd wrapper contains only the specified Node launcher', () => {
  const wrapper = fs.readFileSync(fileURLToPath(new URL('./dev-reset.cmd', import.meta.url)), 'utf8').trim().split(/\r?\n/)
  assert.deepEqual(wrapper, [
    '@echo off',
    'rem Developer-only reset of local app state (see README).',
    'setlocal',
    'node --disable-warning=ExperimentalWarning "%~dp0dev-reset.mjs" %*',
    'exit /b %errorlevel%',
  ])
})
