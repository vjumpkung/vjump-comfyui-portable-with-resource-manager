import { beforeEach, describe, expect, it, vi } from 'vitest'

const commands = vi.hoisted(() => ({
  appInfo: vi.fn(),
  settingsGet: vi.fn(),
  settingsSet: vi.fn(),
  secretSet: vi.fn(),
  secretClear: vi.fn(),
  secretStatus: vi.fn(),
  setupRun: vi.fn(),
  envPipApply: vi.fn(),
  nodeOpenRepository: vi.fn(),
  nodeGitStatus: vi.fn(),
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

import { gateway, IpcClientError } from './gateway'

describe('gateway', () => {
  beforeEach(() => {
    vi.resetAllMocks()
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
