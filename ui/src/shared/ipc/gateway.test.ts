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
  nodeGitStatus: vi.fn(),
  nodeSubscribe: vi.fn(),
  appUpdateStatus: vi.fn(),
  appUpdateCheck: vi.fn(),
  appUpdateInstall: vi.fn(),
  appDiagnostics: vi.fn(),
  appCopyDiagnostics: vi.fn(),
}))

vi.mock('./bindings', () => ({ commands }))
vi.mock('@tauri-apps/api/core', () => ({
  Channel: class {
    onmessage?: (event: unknown) => void
  },
}))

import { gateway, IpcClientError, type CommitInput, type SetupActionDto } from './gateway'

describe('gateway', () => {
  beforeEach(() => {
    vi.resetAllMocks()
  })

  it('setup_curated_catalog_takes_no_arguments', async () => {
    const catalog = [{
      id: 'curated', name: 'Curated node', repository: 'https://example.com/node',
      branch: 'main', commit: 'a'.repeat(40), provenance: 'Pinned catalog entry',
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
    commands.appDiagnostics.mockResolvedValue({ status: 'ok', data: 'report' })
    await expect(gateway.appDiagnostics()).resolves.toBe('report')
    expect(commands.appDiagnostics).toHaveBeenCalledWith()

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
