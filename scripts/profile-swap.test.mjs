// Windows-only integration tests for scripts/profile-swap.ps1. NOT run in CI.
// Every test drives the real script under Windows PowerShell 5.1 against a temp sandbox
// (-SandboxRoot / -CredentialService), never the real app folders or real tokens.
// Dummy generic credentials named `civitai|huggingface.vjswaptest-<hex>` are created with
// `cmdkey /generic:<target> /user:<u> /pass:<dummy>`; `after` deletes all 6 possible test
// targets (2 credentials x live / .profile-default / .profile-tutorial). The tests are skipped
// while the real vjumpkung-comfyui-desktop.exe is running, because the script refuses then.
import assert from 'node:assert/strict'
import { spawn, spawnSync } from 'node:child_process'
import crypto from 'node:crypto'
import { once } from 'node:events'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import test, { after } from 'node:test'
import { fileURLToPath } from 'node:url'

import { credentialListed } from './dev-reset.mjs'

const systemRoot = process.env.SystemRoot ?? 'C:\\Windows'
const system32 = path.join(systemRoot, 'System32')
const powershell = path.join(system32, 'WindowsPowerShell', 'v1.0', 'powershell.exe')
const cmdkey = path.join(system32, 'cmdkey.exe')
const swapScript = fileURLToPath(new URL('./profile-swap.ps1', import.meta.url))
const service = `vjswaptest-${crypto.randomBytes(6).toString('hex')}`
const kinds = ['civitai', 'huggingface']
const suffixes = ['', '.profile-default', '.profile-tutorial']
const allTargets = kinds.flatMap((kind) => suffixes.map((suffix) => `${kind}.${service}${suffix}`))
const longSegments = Array(7).fill('long-component-abcdefghijklmnopqrstuvwxyz')

function skipReason() {
  if (process.platform !== 'win32') return 'Windows only'
  const listing = spawnSync(path.join(system32, 'tasklist.exe'), ['/FI', 'IMAGENAME eq vjumpkung-comfyui-desktop.exe', '/NH'], { encoding: 'utf8', windowsHide: true })
  if (/vjumpkung-comfyui-desktop\.exe/i.test(listing.stdout ?? '')) {
    return 'close vjumpkung-comfyui-desktop.exe first: the swap script refuses while the app runs'
  }
  return false
}
const windows = { skip: skipReason() }

// ---------------------------------------------------------------- credentials (dummy only)

function targetOf(kind, suffix = '') {
  return `${kind}.${service}${suffix}`
}

function addCredential(kind, pass, suffix = '') {
  const target = targetOf(kind, suffix)
  assert.match(target, /^(civitai|huggingface)\.vjswaptest-[0-9a-f]{12}(\.profile-(default|tutorial))?$/)
  const result = spawnSync(cmdkey, [`/generic:${target}`, `/user:${kind}`, `/pass:${pass}`], { encoding: 'utf8', windowsHide: true })
  assert.equal(result.status, 0, `${result.stdout}${result.stderr}`)
}

function hasCredential(kind, suffix = '') {
  const target = targetOf(kind, suffix)
  const result = spawnSync(cmdkey, [`/list:${target}`], { encoding: 'utf8', windowsHide: true })
  return credentialListed(result.stdout, target)
}

function deleteAllCredentials() {
  for (const target of allTargets) spawnSync(cmdkey, [`/delete:${target}`], { windowsHide: true })
}

const fingerprintSource = String.raw`$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
public static class VjTestCred {
  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
  struct CRED { public uint Flags; public uint Type; public IntPtr TargetName; public IntPtr Comment;
    public System.Runtime.InteropServices.ComTypes.FILETIME LastWritten; public uint Size; public IntPtr Blob;
    public uint Persist; public uint AttrCount; public IntPtr Attrs; public IntPtr Alias; public IntPtr User; }
  [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
  static extern bool CredReadW(string target, uint type, uint flags, out IntPtr credential);
  [DllImport("advapi32.dll")]
  static extern void CredFree(IntPtr buffer);
  public static string Fingerprint(string target) {
    IntPtr p;
    if (!CredReadW(target, 1, 0, out p)) return "absent";
    try {
      CRED c = (CRED)Marshal.PtrToStructure(p, typeof(CRED));
      byte[] b = new byte[c.Size];
      if (c.Size > 0) Marshal.Copy(c.Blob, b, 0, (int)c.Size);
      string h = BitConverter.ToString(SHA256.Create().ComputeHash(b)).Replace("-", "");
      return Marshal.PtrToStringUni(c.User) + "|" + c.Persist + "|" + h;
    } finally { CredFree(p); }
  }
}
'@
foreach ($t in $args) { [Console]::Out.WriteLine([VjTestCred]::Fingerprint($t)) }
`

const helperDirectory = process.platform === 'win32' ? fs.mkdtempSync(path.join(os.tmpdir(), 'vjump-swap-helper-')) : ''
const fingerprintScript = path.join(helperDirectory, 'fingerprint.ps1')
if (helperDirectory) fs.writeFileSync(fingerprintScript, fingerprintSource)

// One "user|persist|sha256(blob)" line per target, "absent" when missing. Dummy secrets only.
function fingerprints(targets) {
  const result = spawnSync(powershell, ['-NoLogo', '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', fingerprintScript, ...targets], { encoding: 'utf8', windowsHide: true })
  assert.equal(result.status, 0, `${result.stdout}${result.stderr}`)
  const lines = result.stdout.trim().split(/\r?\n/)
  assert.equal(lines.length, targets.length)
  return lines
}

function liveFingerprints(suffix = '') {
  return fingerprints(kinds.map((kind) => targetOf(kind, suffix)))
}

after(() => {
  if (process.platform !== 'win32') return
  deleteAllCredentials()
  fs.rmSync(helperDirectory, { recursive: true, force: true, maxRetries: 5 })
})

// ---------------------------------------------------------------- fixtures

function put(file, content = 'fixture') {
  fs.mkdirSync(path.dirname(file), { recursive: true })
  fs.writeFileSync(file, content)
}

function snapshot(directory, root = directory) {
  const entries = []
  function visit(value) {
    const info = fs.lstatSync(value)
    const relative = path.relative(root, value)
    if (info.isDirectory()) {
      entries.push(`${relative}|dir`)
      for (const name of fs.readdirSync(value).sort()) visit(path.join(value, name))
    } else {
      entries.push(`${relative}|file|${crypto.createHash('sha256').update(fs.readFileSync(value)).digest('hex')}`)
    }
  }
  if (fs.existsSync(directory)) visit(directory)
  return entries
}

async function stopChild(child) {
  if (child.exitCode !== null || child.signalCode !== null) return
  const exited = once(child, 'exit')
  child.kill()
  await exited
}

function fixture(t) {
  // Spaces and brackets in the sandbox path prove that no step treats paths as wildcards.
  const base = fs.mkdtempSync(path.join(os.tmpdir(), 'vjump swap [t]-'))
  const fx = {
    base,
    appData: path.join(base, 'Roaming'),
    localData: path.join(base, 'Local'),
    dirs: {
      bootstrap: path.join(base, 'Roaming', 'vjumpkung-comfyui'),
      root: path.join(base, 'Local', 'vjumpkung-comfyui'),
      webview: path.join(base, 'Local', 'dev.vjump.comfyui-desktop'),
    },
    state: path.join(base, 'Local', 'vjumpkung-comfyui.profile-swap-state'),
    children: [],
  }
  fs.mkdirSync(fx.appData)
  fs.mkdirSync(fx.localData)
  t.after(async () => {
    for (const child of fx.children) await stopChild(child)
    deleteAllCredentials()
    fs.rmSync(base, { recursive: true, force: true, maxRetries: 10 })
  })
  return fx
}

function readState(fx) {
  return fs.existsSync(fx.state) ? fs.readFileSync(fx.state, 'utf8').trim() : null
}

function stash(dir, name) {
  return `${dir}.profile-${name}`
}

function seedDefault(fx, installRoot = fx.dirs.root) {
  put(path.join(fx.dirs.bootstrap, 'bootstrap.json'), JSON.stringify({ schema_version: 1, install_root: installRoot }))
  put(path.join(fx.dirs.bootstrap, 'extra.txt'), 'bootstrap extra')
  put(path.join(fx.dirs.root, 'install.json'), '{"default":true}')
  put(path.join(fx.dirs.root, 'ComfyUI', 'output', 'image.png'), 'default-image')
  put(path.join(fx.dirs.root, 'ComfyUI', 'models', 'checkpoint [v1] (copy).safetensors'), 'default-model')
  put(path.join(fx.dirs.root, 'data', 'app.db'), 'default-db')
  put(path.join(fx.dirs.webview, 'EBWebView', 'Default', 'Local Storage', 'leveldb', '000003.log'), 'default-webview')
  addCredential('civitai', 'default-civitai-dummy')
  addCredential('huggingface', 'default-huggingface-dummy')
}

// What the manager app would create on a fresh first run while the tutorial profile is active.
function seedTutorial(fx, { hard = false, installRoot = fx.dirs.root } = {}) {
  put(path.join(fx.dirs.bootstrap, 'bootstrap.json'), JSON.stringify({ schema_version: 1, install_root: installRoot }))
  put(path.join(fx.dirs.root, 'tutorial-marker.txt'), 'tutorial-root')
  put(path.join(fx.dirs.webview, 'tutorial-marker.txt'), 'tutorial-webview')
  addCredential('civitai', 'tutorial-civitai-dummy')
  addCredential('huggingface', 'tutorial-huggingface-dummy')
  if (hard) {
    const readOnly = path.join(fx.dirs.root, 'read-only.txt')
    put(readOnly, 'read only')
    fs.chmodSync(readOnly, 0o444)
    const deep = path.join(fx.dirs.webview, ...longSegments, 'deep.txt')
    assert.ok(deep.length > 260)
    put(deep, 'deep')
  }
}

function defaultTree(fx) {
  return Object.values(fx.dirs).flatMap((directory) => snapshot(directory, fx.base))
}

function leftovers(fx) {
  return [...fs.readdirSync(fx.appData), ...fs.readdirSync(fx.localData)].filter((name) => /\.profile-(default|tutorial|discard)/.test(name))
}

function runScript(fx, to, answers = [], extra = []) {
  const args = ['-NoLogo', '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', swapScript, '-To', to, '-SandboxRoot', fx.base, '-CredentialService', service, '-NoPause', ...extra]
  const result = spawnSync(powershell, args, { input: answers.map((answer) => `${answer}\n`).join(''), encoding: 'utf8', windowsHide: true, timeout: 180000 })
  assert.ifError(result.error)
  return { status: result.status, text: `${result.stdout}${result.stderr}` }
}

function expectOk(result, pattern) {
  assert.equal(result.status, 0, result.text)
  if (pattern) assert.match(result.text, pattern)
}

async function startChild(fx, cwd, argument) {
  const child = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)', ...(argument ? [argument] : [])], { cwd, stdio: 'ignore', windowsHide: true })
  fx.children.push(child)
  await once(child, 'spawn')
  return child
}

// ---------------------------------------------------------------- scenarios

test('swap to tutorial hides the default profile; rerunning is a no-op', windows, (t) => {
  const fx = fixture(t)
  seedDefault(fx)
  expectOk(runScript(fx, 'tutorial', ['y']), /Done\. The tutorial profile is active/)
  for (const directory of Object.values(fx.dirs)) {
    assert.equal(fs.existsSync(directory), false)
    assert.equal(fs.existsSync(stash(directory, 'default')), true)
  }
  for (const kind of kinds) {
    assert.equal(hasCredential(kind), false)
    assert.equal(hasCredential(kind, '.profile-default'), true)
  }
  assert.equal(readState(fx), 'tutorial')
  const before = snapshot(fx.base)
  expectOk(runScript(fx, 'tutorial'), /already active/)
  assert.deepEqual(snapshot(fx.base), before)
})

test('restore returns a byte-identical default profile and keeps the tutorial profile aside', windows, (t) => {
  const fx = fixture(t)
  seedDefault(fx)
  const treeBefore = defaultTree(fx)
  const credentialsBefore = liveFingerprints()
  expectOk(runScript(fx, 'tutorial', ['y']))
  seedTutorial(fx)
  const tutorialCredentials = liveFingerprints()
  assert.notDeepEqual(tutorialCredentials, credentialsBefore)
  expectOk(runScript(fx, 'default', ['y']), /Done\. The default profile is active/)
  assert.deepEqual(defaultTree(fx), treeBefore)
  assert.deepEqual(liveFingerprints(), credentialsBefore)
  assert.equal(readState(fx), null)
  assert.equal(fs.readFileSync(path.join(stash(fx.dirs.root, 'tutorial'), 'tutorial-marker.txt'), 'utf8'), 'tutorial-root')
  assert.deepEqual(liveFingerprints('.profile-tutorial'), tutorialCredentials)
  const before = snapshot(fx.base)
  expectOk(runScript(fx, 'default'), /already active[\s\S]*saved tutorial/)
  assert.deepEqual(snapshot(fx.base), before)
})

test('resume brings the saved tutorial profile back', windows, (t) => {
  const fx = fixture(t)
  seedDefault(fx)
  const treeBefore = defaultTree(fx)
  expectOk(runScript(fx, 'tutorial', ['y']))
  seedTutorial(fx)
  const tutorialCredentials = liveFingerprints()
  expectOk(runScript(fx, 'default', ['y']))
  expectOk(runScript(fx, 'tutorial', ['r']), /saved tutorial profile is active/)
  assert.equal(fs.readFileSync(path.join(fx.dirs.root, 'tutorial-marker.txt'), 'utf8'), 'tutorial-root')
  assert.equal(fs.readFileSync(path.join(fx.dirs.webview, 'tutorial-marker.txt'), 'utf8'), 'tutorial-webview')
  assert.deepEqual(liveFingerprints(), tutorialCredentials)
  assert.deepEqual(leftovers(fx).filter((name) => name.includes('.profile-tutorial')), [])
  assert.equal(hasCredential('civitai', '.profile-tutorial'), false)
  assert.equal(readState(fx), 'tutorial')
  expectOk(runScript(fx, 'default', ['y']))
  assert.deepEqual(defaultTree(fx), treeBefore)
})

test('fresh start permanently deletes read-only and over-260-character saved tutorial content', windows, (t) => {
  const fx = fixture(t)
  const tutorialRoot = path.join(fx.base, 'TutorialInstall')
  put(path.join(tutorialRoot, 'keep.txt'), 'outside the swap')
  seedDefault(fx)
  const treeBefore = defaultTree(fx)
  expectOk(runScript(fx, 'tutorial', ['y']))
  seedTutorial(fx, { hard: true, installRoot: tutorialRoot })
  expectOk(runScript(fx, 'default', ['y']))
  assert.ok(fs.existsSync(path.join(stash(fx.dirs.webview, 'tutorial'), ...longSegments, 'deep.txt')))
  const result = runScript(fx, 'tutorial', ['f', 'DELETE'])
  expectOk(result, /left in place: /)
  assert.ok(result.text.includes(`left in place: ${tutorialRoot}`), result.text)
  assert.equal(fs.readFileSync(path.join(tutorialRoot, 'keep.txt'), 'utf8'), 'outside the swap')
  assert.deepEqual(leftovers(fx).filter((name) => /tutorial|discard/.test(name)), [])
  for (const kind of kinds) assert.equal(hasCredential(kind, '.profile-tutorial'), false)
  for (const directory of Object.values(fx.dirs)) {
    assert.equal(fs.existsSync(directory), false)
    assert.equal(fs.existsSync(stash(directory, 'default')), true)
  }
  assert.equal(readState(fx), 'tutorial')
  expectOk(runScript(fx, 'default', ['y']))
  assert.deepEqual(defaultTree(fx), treeBefore)
})

test('cancelling by EOF, n or a wrong answer changes nothing', windows, (t) => {
  const fx = fixture(t)
  seedDefault(fx)
  const treeBefore = snapshot(fx.base)
  const credentialsBefore = liveFingerprints()
  for (const answers of [[], ['n'], ['']]) {
    const result = runScript(fx, 'tutorial', answers)
    assert.equal(result.status, 1, result.text)
    assert.match(result.text, /Cancelled\. Nothing changed\./)
    assert.deepEqual(snapshot(fx.base), treeBefore)
    assert.deepEqual(liveFingerprints(), credentialsBefore)
  }
  // With a saved tutorial profile present, every non-confirming answer also cancels.
  expectOk(runScript(fx, 'tutorial', ['y']))
  seedTutorial(fx)
  expectOk(runScript(fx, 'default', ['y']))
  const savedTree = snapshot(fx.base)
  const savedCredentials = fingerprints(allTargets)
  for (const answers of [[], ['x'], ['f'], ['f', 'delete'], ['f', 'DELETE ']]) {
    const result = runScript(fx, 'tutorial', answers)
    assert.equal(result.status, 1, result.text)
    assert.match(result.text, /Cancelled\. Nothing changed\./)
    assert.deepEqual(snapshot(fx.base), savedTree)
    assert.deepEqual(fingerprints(allTargets), savedCredentials)
  }
})

test('a program locking the live root stops the swap safely; a rerun finishes it', windows, async (t) => {
  const fx = fixture(t)
  seedDefault(fx)
  const treeBefore = defaultTree(fx)
  const child = await startChild(fx, fx.dirs.root)
  const blocked = runScript(fx, 'tutorial', ['y'])
  // The locked folder is either caught by the process check or by the failed rename.
  assert.equal(blocked.status, 1, blocked.text)
  assert.match(blocked.text, /Nothing is lost|Nothing was changed/)
  assert.ok(fs.existsSync(fx.dirs.root), 'the locked root stays in place')
  const halfDone = readState(fx)
  assert.ok(halfDone === null || halfDone === 'stashing-default', String(halfDone))
  await stopChild(child)
  expectOk(runScript(fx, 'tutorial', ['y']))
  assert.equal(readState(fx), 'tutorial')
  assert.equal(fs.existsSync(fx.dirs.root), false)
  expectOk(runScript(fx, 'default', ['y']))
  assert.deepEqual(defaultTree(fx), treeBefore)
})

test('restore from a half-done swap returns to the default profile', windows, async (t) => {
  const fx = fixture(t)
  seedDefault(fx)
  const treeBefore = defaultTree(fx)
  const credentialsBefore = liveFingerprints()
  const child = await startChild(fx, fx.dirs.root)
  const blocked = runScript(fx, 'tutorial', ['y'])
  assert.equal(blocked.status, 1, blocked.text)
  expectOk(runScript(fx, 'default', ['y']))
  assert.equal(readState(fx), null)
  assert.deepEqual(defaultTree(fx), treeBefore)
  assert.deepEqual(liveFingerprints(), credentialsBefore)
  await stopChild(child)
})

test('a program whose command line uses the WebView folder blocks the swap', windows, async (t) => {
  const fx = fixture(t)
  seedDefault(fx)
  await startChild(fx, os.tmpdir(), fx.dirs.webview)
  const treeBefore = snapshot(fx.base)
  const credentialsBefore = liveFingerprints()
  const result = runScript(fx, 'tutorial', ['y'])
  assert.equal(result.status, 1, result.text)
  assert.match(result.text, /PID \d+\s+node\.exe/i)
  assert.deepEqual(snapshot(fx.base), treeBefore)
  assert.deepEqual(liveFingerprints(), credentialsBefore)
  assert.equal(readState(fx), null)
})

test('an interrupted credential copy completes when identical and is refused when it differs', windows, (t) => {
  const identical = fixture(t)
  seedDefault(identical)
  fs.writeFileSync(identical.state, 'stashing-default')
  addCredential('civitai', 'default-civitai-dummy', '.profile-default')
  const original = liveFingerprints()
  expectOk(runScript(identical, 'tutorial', ['y']))
  assert.equal(hasCredential('civitai'), false)
  assert.equal(hasCredential('huggingface'), false)
  assert.deepEqual(liveFingerprints('.profile-default'), original)
  assert.equal(readState(identical), 'tutorial')
  deleteAllCredentials()

  const differing = fixture(t)
  seedDefault(differing)
  fs.writeFileSync(differing.state, 'stashing-default')
  addCredential('civitai', 'a-different-dummy', '.profile-default')
  const treeBefore = snapshot(differing.base)
  const liveBefore = liveFingerprints()
  const stashBefore = liveFingerprints('.profile-default')
  const result = runScript(differing, 'tutorial', ['y'])
  assert.equal(result.status, 1, result.text)
  assert.match(result.text, /exist and differ; refusing to overwrite/)
  assert.deepEqual(snapshot(differing.base), treeBefore)
  assert.deepEqual(liveFingerprints(), liveBefore)
  assert.deepEqual(liveFingerprints('.profile-default'), stashBefore)
})

test('restore refuses, changing nothing, when a live folder and its stash both exist', windows, (t) => {
  const fx = fixture(t)
  seedDefault(fx)
  expectOk(runScript(fx, 'tutorial', ['y']))
  put(path.join(fx.dirs.bootstrap, 'bootstrap.json'), '{"schema_version":1}')
  put(path.join(fx.dirs.webview, 'live.txt'), 'live')
  put(path.join(stash(fx.dirs.webview, 'tutorial'), 'saved.txt'), 'saved')
  const treeBefore = snapshot(fx.base)
  const result = runScript(fx, 'default', ['y'])
  assert.equal(result.status, 1, result.text)
  assert.ok(result.text.includes(fx.dirs.webview) && result.text.includes(stash(fx.dirs.webview, 'tutorial')), result.text)
  assert.match(result.text, /refusing to overwrite/)
  assert.deepEqual(snapshot(fx.base), treeBefore)
  assert.equal(readState(fx), 'tutorial')
})

test('bad parameter combinations exit 2 without touching anything', windows, (t) => {
  const fx = fixture(t)
  const run = (...extra) => {
    const args = ['-NoLogo', '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', swapScript, '-To', 'tutorial', '-NoPause', ...extra]
    const result = spawnSync(powershell, args, { input: '', encoding: 'utf8', windowsHide: true, timeout: 60000 })
    assert.ifError(result.error)
    return result
  }
  const cases = [
    ['only -SandboxRoot', ['-SandboxRoot', fx.base]],
    ['only -CredentialService', ['-CredentialService', service]],
    ['the real service name', ['-SandboxRoot', fx.base, '-CredentialService', 'vjumpkung-comfyui']],
    ['the real service name in another case', ['-SandboxRoot', fx.base, '-CredentialService', 'VJUMPKUNG-COMFYUI']],
    ['an invalid service name', ['-SandboxRoot', fx.base, '-CredentialService', 'bad name!']],
    ['a relative sandbox root', ['-SandboxRoot', 'relative\\sandbox', '-CredentialService', service]],
  ]
  // A sandbox whose Roaming folder is the real APPDATA must be rejected (stdin is empty, so even a
  // failed guard could only cancel at the first prompt).
  if (process.env.APPDATA && path.basename(process.env.APPDATA).toLowerCase() === 'roaming') {
    cases.push(['a sandbox that resolves to the real folders', ['-SandboxRoot', path.dirname(process.env.APPDATA), '-CredentialService', service]])
  }
  for (const [name, extra] of cases) {
    const result = run(...extra)
    assert.equal(result.status, 2, `${name}: ${result.stdout}${result.stderr}`)
    assert.match(result.stderr, /Usage error/, name)
  }
  assert.deepEqual(snapshot(fx.base), ['|dir', 'Local|dir', 'Roaming|dir'])
})

test('an empty default profile round-trips', windows, (t) => {
  const fx = fixture(t)
  expectOk(runScript(fx, 'tutorial', ['y']))
  assert.equal(readState(fx), 'tutorial')
  expectOk(runScript(fx, 'default', ['y']), /Done\. The default profile is active/)
  assert.equal(readState(fx), null)
  assert.deepEqual(fs.readdirSync(fx.appData), [])
  assert.deepEqual(fs.readdirSync(fx.localData), [])
  for (const target of allTargets) assert.equal(credentialListed(spawnSync(cmdkey, [`/list:${target}`], { encoding: 'utf8', windowsHide: true }).stdout, target), false)
})

test('a custom install root outside the app folder is untouched and reported', windows, (t) => {
  const fx = fixture(t)
  const custom = path.join(fx.base, 'Custom Install [D]')
  put(path.join(custom, 'models', 'keep.bin'), 'real model')
  put(path.join(custom, 'install.json'), '{"custom":true}')
  seedDefault(fx, custom)
  const customBefore = snapshot(custom, fx.base)
  const treeBefore = defaultTree(fx)
  const result = runScript(fx, 'tutorial', ['y'])
  expectOk(result)
  assert.ok(result.text.includes(`Your real install root ${custom} is not hidden. Do not choose or open it during the tutorial.`), result.text)
  assert.match(result.text, /folder picker's recent-folder history is not swapped/)
  assert.deepEqual(snapshot(custom, fx.base), customBefore)
  expectOk(runScript(fx, 'default', ['y']))
  assert.deepEqual(snapshot(custom, fx.base), customBefore)
  assert.deepEqual(defaultTree(fx), treeBefore)
})

for (const [file, comment, to] of [
  ['swap-to-tutorial-profile.cmd', 'Swap app state to the tutorial profile (see README).', 'tutorial'],
  ['restore-default-profile.cmd', 'Restore the default app state profile (see README).', 'default'],
]) {
  test(`${file} is a wrapper containing only the PowerShell launcher`, () => {
    const raw = fs.readFileSync(fileURLToPath(new URL(`./${file}`, import.meta.url)), 'utf8')
    assert.deepEqual(raw.trim().split(/\r?\n/), [
      '@echo off',
      `rem ${comment}`,
      'setlocal',
      `"%SystemRoot%\\System32\\WindowsPowerShell\\v1.0\\powershell.exe" -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0profile-swap.ps1" -To ${to} %*`,
      'exit /b %errorlevel%',
    ])
  })
}
