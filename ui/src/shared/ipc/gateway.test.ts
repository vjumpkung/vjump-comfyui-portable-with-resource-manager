import { beforeEach, describe, expect, it, vi } from 'vitest'

const commands = vi.hoisted(() => ({
  appInfo: vi.fn(),
  settingsGet: vi.fn(),
  settingsSet: vi.fn(),
  secretSet: vi.fn(),
  secretClear: vi.fn(),
  secretStatus: vi.fn(),
  setupRun: vi.fn(),
  setupListCuratedNodes: vi.fn(),
  setupOpenInstall: vi.fn(),
  setupNewInstall: vi.fn(),
  envPipApply: vi.fn(),
  nodeOpenRepository: vi.fn(),
  nodeReveal: vi.fn(),
  modelReveal: vi.fn(),
  galleryList: vi.fn(),
  galleryRename: vi.fn(),
  galleryMove: vi.fn(),
  galleryCopy: vi.fn(),
  galleryDeletePlan: vi.fn(),
  galleryDelete: vi.fn(),
  galleryReveal: vi.fn(),
  nodeGitStatus: vi.fn(),
  nodeSubscribe: vi.fn(),
  appUpdateStatus: vi.fn(),
  appUpdateCheck: vi.fn(),
  appUpdateInstall: vi.fn(),
  appCopyDiagnostics: vi.fn(),
}))

vi.mock('./bindings', () => ({ commands }))
vi.mock('@tauri-apps/api/core', () => ({
  Channel: class {
    onmessage?: (event: unknown) => void
  },
  // The Windows form of Tauri's helper; a plain function so `resetAllMocks` keeps it.
  convertFileSrc: (path: string, protocol = 'asset') =>
    `http://${protocol}.localhost/${encodeURIComponent(path)}`,
}))

import { gateway, IpcClientError, type CommitInput, type SetupActionDto } from './gateway'

describe('gateway', () => {
  beforeEach(() => {
    vi.resetAllMocks()
  })

  it('setup_curated_catalog_takes_no_arguments', async () => {
    const catalog = [{
      id: 'curated', name: 'Curated node', repository: 'https://example.com/node',
      provenance: 'Latest default-branch commit', unavailable: null,
    }]
    commands.setupListCuratedNodes.mockResolvedValue({ status: 'ok', data: catalog })
    await expect(gateway.setupListCuratedNodes()).resolves.toBe(catalog)
    expect(commands.setupListCuratedNodes).toHaveBeenCalledWith()
  })

  it('setup_root_recovery_sends_exact_paths', async () => {
    const response = { restartRequired: true }
    commands.setupOpenInstall.mockResolvedValue({ status: 'ok', data: response })
    commands.setupNewInstall.mockResolvedValue({ status: 'ok', data: response })
    await expect(gateway.setupOpenInstall('D:/Existing install')).resolves.toBe(response)
    expect(commands.setupOpenInstall).toHaveBeenCalledWith('D:/Existing install')
    await expect(gateway.setupNewInstall('E:/New install')).resolves.toBe(response)
    expect(commands.setupNewInstall).toHaveBeenCalledWith('E:/New install')

    commands.setupOpenInstall.mockResolvedValue({
      status: 'error', error: { code: 'root_invalid', message: 'Choose a valid installation.' },
    })
    await expect(gateway.setupOpenInstall('D:/Invalid')).rejects.toMatchObject({
      name: 'IpcClientError', code: 'root_invalid', message: 'Choose a valid installation.',
    })
  })

  it('setup_run_forwards_accelerators_curated_selection_and_retry_decisions', async () => {
    const started = { runId: 'run-1' }
    commands.setupRun.mockResolvedValue({ status: 'ok', data: started })
    const request = {
      mode: 'first_run' as const,
      pythonSource: { kind: 'managed' as const },
      profileId: 'profile-1', comfyuiRef: { kind: 'latest' as const },
      installManager: false, accelerators: ['sage', 'flash'], curatedNodes: ['curated'],
      cleanCache: false, confirmAdoptedChanges: false, allowSourceBuilds: false,
      thirdPartyConsent: false,
    }
    await expect(gateway.setupRun({ kind: 'new', request }, vi.fn())).resolves.toBe(started)
    expect(commands.setupRun).toHaveBeenLastCalledWith({ kind: 'new', request }, expect.anything())
    const empty = { ...request, accelerators: [], curatedNodes: [] }
    await gateway.setupRun({ kind: 'new', request: empty }, vi.fn())
    expect(commands.setupRun).toHaveBeenLastCalledWith({ kind: 'new', request: empty }, expect.anything())

    const decision: CommitInput = {
      planId: 'plan-1', binding: 'exact-binding', executeNodeCode: true,
      runInstallPy: false, acknowledgeWarnings: true, rememberSource: false,
      rememberInstallPy: false, confirmedChanges: [],
    }
    const retry = {
      kind: 'retry', runId: 'run-1', managerConsent: 'exact-manager-commit',
      curatedDecisions: [decision],
    } satisfies SetupActionDto
    await gateway.setupRun(retry, vi.fn())
    expect(commands.setupRun).toHaveBeenLastCalledWith(retry, expect.anything())
  })

  it('node_subscription_keeps_tauri_channel_inside_gateway', async () => {
    commands.nodeSubscribe.mockResolvedValue({ status: 'ok', data: 42 })
    const receive = vi.fn()
    await expect(gateway.nodeSubscribe(receive)).resolves.toBe(42)
    expect(commands.nodeSubscribe).toHaveBeenCalledTimes(1)
    const [channel] = commands.nodeSubscribe.mock.calls[0] as [{ onmessage: (event: unknown) => void }]
    expect(channel).not.toBe(receive)
    const event = { kind: 'snapshot', operations: [] }
    channel.onmessage(event)
    expect(receive).toHaveBeenCalledWith(event)
  })

  it('node_repository_open_uses_generated_command', async () => {
    commands.nodeOpenRepository.mockResolvedValue({ status: 'ok', data: null })
    await expect(gateway.nodeOpenRepository('https://github.com/example/node'))
      .resolves.toBeNull()
    expect(commands.nodeOpenRepository)
      .toHaveBeenCalledWith('https://github.com/example/node')
  })

  it('node_git_status_wraps_node_id', async () => {
    const status = {
      commit: 'a'.repeat(40), branch: 'main', dirty: true,
      unfinishedOperation: false, ahead: 0, behind: 1,
      hasUpstream: true, status: ' M requirements.txt\n',
    }
    commands.nodeGitStatus.mockResolvedValue({ status: 'ok', data: status })
    await expect(gateway.nodeGitStatus('node-1')).resolves.toEqual(status)
    expect(commands.nodeGitStatus).toHaveBeenCalledWith({ nodeId: 'node-1' })
  })

  it('reveal_commands_send_the_location_or_null', async () => {
    commands.modelReveal.mockResolvedValue({ status: 'ok', data: null })
    commands.nodeReveal.mockResolvedValue({ status: 'ok', data: null })
    const location = { rootId: 'models', path: 'loras/a.safetensors' }
    await expect(gateway.modelReveal(location)).resolves.toBeNull()
    expect(commands.modelReveal).toHaveBeenLastCalledWith(location)
    await gateway.modelReveal(null)
    expect(commands.modelReveal).toHaveBeenLastCalledWith(null)
    await expect(gateway.nodeReveal('node-1')).resolves.toBeNull()
    expect(commands.nodeReveal).toHaveBeenLastCalledWith('node-1')
    await gateway.nodeReveal(null)
    expect(commands.nodeReveal).toHaveBeenLastCalledWith(null)
  })

  it('gallery_commands_send_exact_payloads_and_keep_stable_error_codes', async () => {
    const directory = { area: 'output' as const, path: 'a/b' }
    const entry = { location: { area: 'output' as const, path: 'a/b/c.png' }, revision: 'r1' }
    const listing = { directory, entries: [], warnings: [] }
    const report = { results: [] }
    commands.galleryList.mockResolvedValue({ status: 'ok', data: listing })
    commands.galleryRename.mockResolvedValue({ status: 'ok', data: entry.location })
    commands.galleryMove.mockResolvedValue({ status: 'ok', data: report })
    commands.galleryCopy.mockResolvedValue({ status: 'ok', data: report })
    commands.galleryDeletePlan.mockResolvedValue({ status: 'ok', data: { planId: 'p', items: [], totalBytes: 0 } })
    commands.galleryDelete.mockResolvedValue({ status: 'ok', data: report })
    commands.galleryReveal.mockResolvedValue({ status: 'ok', data: null })

    await expect(gateway.galleryList(directory)).resolves.toBe(listing)
    expect(commands.galleryList).toHaveBeenCalledWith(directory)
    await gateway.galleryRename({ entry, newName: 'd.png' })
    expect(commands.galleryRename).toHaveBeenCalledWith({ entry, newName: 'd.png' })
    const transfer = { items: [entry], destination: { area: 'input' as const, path: '' } }
    await expect(gateway.galleryMove(transfer)).resolves.toBe(report)
    expect(commands.galleryMove).toHaveBeenCalledWith(transfer)
    await gateway.galleryCopy(transfer)
    expect(commands.galleryCopy).toHaveBeenCalledWith(transfer)
    await gateway.galleryDeletePlan([entry])
    expect(commands.galleryDeletePlan).toHaveBeenCalledWith([entry])
    await gateway.galleryDelete('p')
    expect(commands.galleryDelete).toHaveBeenCalledWith('p')
    await expect(gateway.galleryReveal(directory)).resolves.toBeNull()
    expect(commands.galleryReveal).toHaveBeenCalledWith(directory)

    commands.galleryList.mockResolvedValue({
      status: 'error',
      error: { code: 'gallery_not_installed', message: 'Install ComfyUI first.' },
    })
    await expect(gateway.galleryList(directory)).rejects.toMatchObject({
      name: 'IpcClientError', code: 'gallery_not_installed', message: 'Install ComfyUI first.',
    })
  })

  it('gallery_preview_url_is_hierarchical_and_encodes_every_segment', () => {
    const base = gateway.galleryPreviewBase()
    expect(base).toBe('http://gallery.localhost/')
    const url = (path: string, revision = 'r1') =>
      gateway.galleryPreviewUrl({ area: 'output', path }, revision)

    expect(url('cat.png')).toBe(`${base}output/cat.png?v=r1`)
    // Nested folders keep their slashes so glTF-relative resources resolve beside the model.
    expect(url('models/robot/robot.gltf')).toBe(`${base}output/models/robot/robot.gltf?v=r1`)
    expect(url('my folder/my photo.png')).toBe(`${base}output/my%20folder/my%20photo.png?v=r1`)
    expect(url('a#b/c?d.png')).toBe(`${base}output/a%23b/c%3Fd.png?v=r1`)
    expect(url('รูป/แมว ส้ม.png')).toBe(
      `${base}output/%E0%B8%A3%E0%B8%B9%E0%B8%9B/%E0%B9%81%E0%B8%A1%E0%B8%A7%20%E0%B8%AA%E0%B9%89%E0%B8%A1.png?v=r1`,
    )
    expect(url('100%/x.png')).toBe(`${base}output/100%25/x.png?v=r1`)
    // The revision is a cache buster and may contain reserved characters.
    expect(url('a.png', 'v 1&x=2#')).toBe(`${base}output/a.png?v=v%201%26x%3D2%23`)
    // Redundant slashes never produce empty segments, and the area root has no path.
    expect(url('/a//b/')).toBe(`${base}output/a/b?v=r1`)
    expect(url('')).toBe(`${base}output/?v=r1`)
    expect(gateway.galleryPreviewUrl({ area: 'input', path: 'x.png' }, 'r')).toBe(
      `${base}input/x.png?v=r`,
    )
  })

  it('app_update_commands_send_exact_payloads_and_keep_the_channel_inside', async () => {
    commands.appUpdateStatus.mockResolvedValue({
      status: 'ok',
      data: { configured: true, currentVersion: '0.1.0' },
    })
    await expect(gateway.appUpdateStatus()).resolves.toEqual({
      configured: true,
      currentVersion: '0.1.0',
    })
    expect(commands.appUpdateStatus).toHaveBeenCalledWith()

    const offer = {
      updateId: 'update-1',
      version: '0.2.0',
      date: '2026-10-17',
      notes: 'plain notes',
    }
    commands.appUpdateCheck.mockResolvedValue({ status: 'ok', data: offer })
    await expect(gateway.appUpdateCheck()).resolves.toEqual(offer)
    expect(commands.appUpdateCheck).toHaveBeenCalledWith()

    commands.appUpdateInstall.mockResolvedValue({ status: 'ok', data: null })
    const receive = vi.fn()
    await expect(
      gateway.appUpdateInstall('update-1', receive),
    ).resolves.toBeNull()
    expect(commands.appUpdateInstall).toHaveBeenCalledTimes(1)
    const [updateId, channel] = commands.appUpdateInstall.mock.calls[0] as [
      string,
      { onmessage: (event: unknown) => void },
    ]
    expect(updateId).toBe('update-1')
    const event = { kind: 'downloading', downloadedBytes: 5, totalBytes: 10 }
    channel.onmessage(event)
    expect(receive).toHaveBeenCalledWith(event)
  })

  it('app_update_errors_keep_their_stable_codes', async () => {
    commands.appUpdateInstall.mockResolvedValue({
      status: 'error',
      error: { code: 'update_stale', message: 'Check again.' },
    })
    const failure = await gateway
      .appUpdateInstall('update-1', vi.fn())
      .catch((error: unknown) => error)
    expect(failure).toBeInstanceOf(IpcClientError)
    expect(failure).toMatchObject({ code: 'update_stale' })
  })

  it('app_diagnostics_commands_take_no_arguments', async () => {
    commands.appCopyDiagnostics.mockResolvedValue({
      status: 'ok',
      data: { text: 'report', copied: false },
    })
    await expect(gateway.appCopyDiagnostics()).resolves.toEqual({
      text: 'report',
      copied: false,
    })
    expect(commands.appCopyDiagnostics).toHaveBeenCalledWith()
  })

  it('unwraps ok results', async () => {
    commands.settingsGet.mockResolvedValue({
      status: 'ok',
      data: { downloadConcurrency: 3 },
    })
    await expect(gateway.settingsGet()).resolves.toEqual({
      downloadConcurrency: 3,
    })
  })

  it('maps error results to a typed IpcClientError', async () => {
    commands.settingsSet.mockResolvedValue({
      status: 'error',
      error: {
        code: 'validation',
        message: 'download concurrency must be positive',
      },
    })
    const failure = await gateway
      .settingsSet({
        downloadConcurrency: 0,
        hfEngine: 'hf_cli',
        closeBehavior: 'ask',
        theme: 'system',
        language: 'en',
        registryCacheTtl: 6,
        bandwidthCapKibS: null,
      })
      .catch((error: unknown) => error)
    expect(failure).toBeInstanceOf(IpcClientError)
    expect(failure).toMatchObject({
      code: 'validation',
      message: 'download concurrency must be positive',
    })
  })

  it('hides transport failure details behind a fixed message', async () => {
    commands.appInfo.mockRejectedValue(
      new Error('C:\\Users\\someone\\secret path failure'),
    )
    const failure = await gateway.appInfo().catch((error: unknown) => error)
    expect(failure).toBeInstanceOf(IpcClientError)
    expect(failure).toMatchObject({ code: 'transport' })
    expect((failure as IpcClientError).message).not.toContain('someone')
  })

  it('passes the secret to the binding without wrapping it in other state', async () => {
    commands.secretSet.mockResolvedValue({ status: 'ok', data: null })
    await gateway.secretSet('civitAi', 'placeholder-value')
    expect(commands.secretSet).toHaveBeenCalledWith({
      kind: 'civitAi',
      value: 'placeholder-value',
    })
  })

  it('creates the typed Channel behind the gateway and forwards events', async () => {
    commands.setupRun.mockResolvedValue({
      status: 'ok',
      data: { runId: 'run-1' },
    })
    const receive = vi.fn()
    await expect(
      gateway.setupRun(
        {
          kind: 'retry',
          runId: 'run-1',
          managerConsent: null,
          curatedDecisions: [],
        },
        receive,
      ),
    ).resolves.toEqual({ runId: 'run-1' })
    const channel = commands.setupRun.mock.calls[0][1] as {
      onmessage: (event: unknown) => void
    }
    const event = { kind: 'finished', runId: 'run-1', outcome: 'succeeded' }
    channel.onmessage(event)
    expect(receive).toHaveBeenCalledWith(event)
  })

  it('forwards only the exact protected changes tied to a plan', async () => {
    commands.envPipApply.mockResolvedValue({
      status: 'ok',
      data: { envState: 'verified', protectedChanges: [] },
    })
    const input = {
      planId: 'plan-1',
      confirmedChanges: [{ name: 'torch', from: '2.14.1', to: '2.14.2' }],
    }
    await gateway.envPipApply(input)
    expect(commands.envPipApply).toHaveBeenCalledWith(input)
  })
})
