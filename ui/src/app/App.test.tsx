// @vitest-environment jsdom
import {
  act,
  cleanup,
  configure,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'

import {
  gateway,
  IpcClientError,
  type PlanReviewDto,
  type RunSnapshotDto,
  type SetupStatusDto,
} from '@/shared/ipc/gateway'
import { environmentKeys } from '@/shared/lib/environment-queries'
import { queryClient } from '@/shared/lib/query'
import { environment, profile, run, setupStatus } from '@/shared/testing/environment'

import { App } from './App'

const restrictedServices = vi.hoisted(() => ({
  settings: vi.fn(() => ({ data: null })),
  themeToggle: vi.fn(() => null),
}))

vi.mock('@/shared/ipc/gateway', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/ipc/gateway')>()),
  gateway: {
    setupStatus: vi.fn(),
    setupDetectHardware: vi.fn(),
    setupListProfiles: vi.fn(),
    setupListCuratedNodes: vi.fn(),
    setupValidatePython: vi.fn(),
    setupSelectRoot: vi.fn(),
    setupOpenInstall: vi.fn(),
    setupNewInstall: vi.fn(),
    setupRun: vi.fn(),
    setupCancel: vi.fn(),
    pickFolder: vi.fn(),
    pickFile: vi.fn(),
    envInfo: vi.fn(),
    nodeCommit: vi.fn(),
    nodeUpdateAll: vi.fn(),
  },
}))
vi.mock('@/features/settings', () => ({ useSettings: restrictedServices.settings }))
vi.mock('@/shared/ui/log-view', () => ({ LogView: () => <p>Live setup log</p> }))
vi.mock('./shell/theme-toggle', () => ({ ThemeToggle: restrictedServices.themeToggle }))
vi.mock('./shell/app-shell', () => ({
  AppShell: () => <p>Normal application shell</p>,
}))

vi.setConfig({ testTimeout: 30_000 })
configure({ asyncUtilTimeout: 5000 })

const repository = 'https://github.com/example/curated-pack'
const detachedWarning = 'Installed detached at the pinned commit; updates need an explicit reference.'
let currentStatus: SetupStatusDto

function plan(binding = 'binding-1'): PlanReviewDto {
  return {
    id: 'curated-plan-1',
    binding,
    kind: 'install',
    name: 'Curated Pack',
    source: { kind: 'git', repository, commit: 'a'.repeat(40), branch: null },
    priorSource: null,
    requirements: ['numpy>=1.26'],
    artifactDigest: null,
    treeDigest: 'b'.repeat(64),
    hookDigest: null,
    protectedChanges: [],
    warnings: [detachedWarning],
    trustedSource: false,
    trustedInstallPy: false,
    hasRequirements: true,
    hasInstallPy: false,
  }
}

function pausedCurated(review = plan()): RunSnapshotDto {
  return {
    ...run,
    id: 'curated-run',
    steps: [{
      step: 'curated_nodes',
      number: 11,
      label: 'Install curated custom nodes',
      policy: 'optional',
      status: 'failed',
      attempts: 1,
      message: null,
    }],
    failure: {
      ...run.failure!,
      step: 'curated_nodes',
      message: 'Review each exact source before installing curated nodes.',
      consent: null,
    },
    curated: [{ id: 'curated-pack', status: 'prepared', message: 'Setup does not run install.py.' }],
    curatedReviews: [review],
  }
}

function rebuildStatus(code: 'moved' | 'interpreter_broken' = 'moved'): SetupStatusDto {
  return {
    ...setupStatus,
    phase: 'rebuild_required',
    activeEnv: environment,
    recovery: { code, message: code === 'moved' ? 'The installation moved.' : 'The selected interpreter cannot run.' },
  }
}

function mount(status: SetupStatusDto) {
  currentStatus = status
  return render(<App />)
}

function updateStatus(status: SetupStatusDto) {
  currentStatus = status
  act(() => { queryClient.setQueryData(environmentKeys.status, status) })
}

function resetScreen() {
  cleanup()
  queryClient.clear()
}

function checkbox(name: RegExp) {
  return screen.getByRole('checkbox', { name })
}

async function startFirstRun() {
  await screen.findByLabelText('Install root')
  fireEvent.click(screen.getByRole('button', { name: 'Continue' }))
  await screen.findByText('Detected')
  fireEvent.click(screen.getByRole('button', { name: 'Continue' }))
  fireEvent.click(screen.getByRole('button', { name: 'Continue' }))
  await screen.findByRole('radio', { name: /Python 3.13/ })
  fireEvent.click(screen.getByRole('button', { name: 'Continue' }))
  expect(checkbox(/Curated Pack/).getAttribute('aria-checked')).toBe('false')
  fireEvent.click(checkbox(/Curated Pack/))
  fireEvent.click(screen.getByRole('button', { name: 'Continue' }))
  fireEvent.click(screen.getByRole('button', { name: 'Start setup' }))
  return screen.findByRole('dialog')
}

beforeEach(() => {
  vi.resetAllMocks()
  restrictedServices.settings.mockReturnValue({ data: null })
  restrictedServices.themeToggle.mockReturnValue(null)
  queryClient.clear()
  queryClient.setDefaultOptions({ queries: { retry: false, refetchOnWindowFocus: false }, mutations: { retry: false } })
  vi.stubGlobal('ResizeObserver', class {
    observe() {}
    unobserve() {}
    disconnect() {}
  })
  vi.mocked(gateway.setupStatus).mockImplementation(() => Promise.resolve(currentStatus))
  vi.mocked(gateway.setupDetectHardware).mockResolvedValue({
    gpuStatus: 'detected',
    gpus: [],
    driverVersion: '591',
    driverCuda: '13.1',
    os: 'windows',
    osVersion: '11',
    installVolumeFreeBytes: 100 * 1024 ** 3,
    envVolumeFreeBytes: null,
    envOnSeparateVolume: false,
    longPathsEnabled: true,
  })
  vi.mocked(gateway.setupListProfiles).mockResolvedValue([profile])
  vi.mocked(gateway.setupListCuratedNodes).mockResolvedValue([{
    id: 'curated-pack',
    name: 'Curated Pack',
    repository,
    provenance: 'Latest default-branch fixture',
    unavailable: null,
  }])
  vi.mocked(gateway.pickFolder).mockResolvedValue(null)
  vi.mocked(gateway.setupRun).mockResolvedValue({ runId: 'curated-run' })
})

afterEach(() => {
  resetScreen()
  vi.unstubAllGlobals()
})

it('curated_selection_does_not_authorize_execution', async () => {
  vi.mocked(gateway.setupRun).mockImplementationOnce(() => {
    currentStatus = { ...setupStatus, run: pausedCurated() }
    return Promise.resolve({ runId: 'curated-run' })
  }).mockRejectedValueOnce(new IpcClientError('maintenance_busy', 'Another maintenance operation is still running.'))
  mount(setupStatus)
  const dialog = await startFirstRun()
  expect(gateway.setupRun).toHaveBeenCalledTimes(1)
  expect(gateway.setupRun).toHaveBeenNthCalledWith(1, {
    kind: 'new',
    request: {
      mode: 'first_run',
      pythonSource: { kind: 'managed' },
      profileId: 'default',
      comfyuiRef: { kind: 'latest' },
      installManager: true,
      accelerators: [],
      curatedNodes: ['curated-pack'],
      cleanCache: false,
      confirmAdoptedChanges: false,
      allowSourceBuilds: false,
      thirdPartyConsent: true,
    },
  }, expect.any(Function))
  expect(within(dialog).getByText(detachedWarning)).toBeTruthy()
  expect(checkbox(/execute third-party code/).getAttribute('aria-checked')).toBe('false')
  expect(checkbox(/acknowledge all displayed warnings/).getAttribute('aria-checked')).toBe('false')
  expect(screen.queryByRole('checkbox', { name: /Remember this source|remember permission|root install\.py/ })).toBeNull()
  const apply = within(dialog).getByRole('button', { name: /reviewed|Approve/i })
  expect(apply).toHaveProperty('disabled', true)
  fireEvent.click(checkbox(/execute third-party code/))
  expect(apply).toHaveProperty('disabled', true)
  expect(gateway.setupRun).toHaveBeenCalledTimes(1)
  fireEvent.click(checkbox(/acknowledge all displayed warnings/))
  fireEvent.click(apply)
  await waitFor(() => expect(gateway.setupRun).toHaveBeenCalledTimes(2))
  expect(gateway.setupRun).toHaveBeenLastCalledWith({
    kind: 'retry',
    runId: 'curated-run',
    managerConsent: null,
    curatedDecisions: [{
      planId: 'curated-plan-1',
      binding: 'binding-1',
      executeNodeCode: true,
      runInstallPy: false,
      acknowledgeWarnings: true,
      rememberSource: false,
      rememberInstallPy: false,
      confirmedChanges: [],
    }],
  }, expect.any(Function))
  expect(gateway.nodeCommit).not.toHaveBeenCalled()
  expect(gateway.nodeUpdateAll).not.toHaveBeenCalled()
  expect(await within(screen.getByRole('dialog')).findByText('Another maintenance operation is still running.')).toBeTruthy()
})

it('curated_binding_change_clears_consent', async () => {
  mount({ ...setupStatus, run: pausedCurated() })
  await screen.findByRole('dialog')
  fireEvent.click(checkbox(/execute third-party code/))
  fireEvent.click(checkbox(/acknowledge all displayed warnings/))
  expect(checkbox(/execute third-party code/).getAttribute('aria-checked')).toBe('true')
  updateStatus({ ...setupStatus, run: pausedCurated(plan('binding-2')) })
  await waitFor(() => expect(checkbox(/execute third-party code/).getAttribute('aria-checked')).toBe('false'))
  expect(checkbox(/acknowledge all displayed warnings/).getAttribute('aria-checked')).toBe('false')
  fireEvent.click(checkbox(/execute third-party code/))
  fireEvent.click(checkbox(/acknowledge all displayed warnings/))
  updateStatus({ ...setupStatus, run: pausedCurated() })
  await waitFor(() => expect(checkbox(/execute third-party code/).getAttribute('aria-checked')).toBe('false'))
  expect(checkbox(/acknowledge all displayed warnings/).getAttribute('aria-checked')).toBe('false')
  expect(within(screen.getByRole('dialog')).getByRole('button', { name: /reviewed|Approve/i })).toHaveProperty('disabled', true)
  expect(gateway.setupRun).not.toHaveBeenCalled()
  fireEvent.click(screen.getByRole('button', { name: 'Back to setup' }))
  expect(screen.queryByRole('dialog')).toBeNull()
  expect(gateway.setupCancel).not.toHaveBeenCalled()
  fireEvent.click(screen.getByRole('button', { name: 'Review curated plans' }))
  expect(checkbox(/execute third-party code/).getAttribute('aria-checked')).toBe('false')
})

it('missing_root_offers_locate_and_new_install', async () => {
  const rootStatus: SetupStatusDto = { ...setupStatus, phase: 'recovery_required', recovery: { code: 'root_missing', message: 'The recorded installation root is missing.' } }
  let loadStatus: (status: SetupStatusDto) => void = () => undefined
  const pendingStatus = new Promise<SetupStatusDto>((resolve) => { loadStatus = resolve })
  vi.mocked(gateway.setupStatus).mockReturnValueOnce(pendingStatus)
  mount(rootStatus)
  expect(restrictedServices.settings).not.toHaveBeenCalled()
  expect(restrictedServices.themeToggle).not.toHaveBeenCalled()
  await act(async () => {
    loadStatus(rootStatus)
    await pendingStatus
  })
  await screen.findByRole('button', { name: 'Locate existing install' })
  expect(screen.getByRole('button', { name: 'Create new install' })).toBeTruthy()
  expect(screen.getByText(/Recorded install root: D:\/Comfy/)).toBeTruthy()
  expect(screen.getByText(/recorded installation root is missing/)).toBeTruthy()
  expect(screen.getByText(/restart/i)).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: 'Locate existing install' }))
  await waitFor(() => expect(gateway.pickFolder).toHaveBeenCalledTimes(1))
  await waitFor(() => expect(screen.getByRole('button', { name: 'Locate existing install' })).toHaveProperty('disabled', false))
  expect(gateway.setupOpenInstall).not.toHaveBeenCalled()
  expect(gateway.setupNewInstall).not.toHaveBeenCalled()
  vi.mocked(gateway.pickFolder).mockResolvedValue('E:/Existing Comfy')
  vi.mocked(gateway.setupOpenInstall).mockRejectedValueOnce(new IpcClientError('root_invalid', 'The selected directory has no installation manifest.'))
  fireEvent.click(screen.getByRole('button', { name: 'Locate existing install' }))
  await screen.findByText(/selected directory has no installation manifest/)
  expect(gateway.setupOpenInstall).toHaveBeenCalledWith('E:/Existing Comfy')
  let release: (value: { restartRequired: boolean }) => void = () => undefined
  const rootSelection = new Promise<{ restartRequired: boolean }>((resolve) => { release = resolve })
  vi.mocked(gateway.setupOpenInstall).mockReturnValue(rootSelection)
  fireEvent.click(screen.getByRole('button', { name: 'Locate existing install' }))
  await waitFor(() => expect(gateway.setupOpenInstall).toHaveBeenCalledTimes(2))
  expect(screen.getByRole('button', { name: /Locate existing install|Locating/ })).toHaveProperty('disabled', true)
  fireEvent.click(screen.getByRole('button', { name: /Locate existing install|Locating/ }))
  expect(gateway.setupOpenInstall).toHaveBeenCalledTimes(2)
  await act(async () => {
    release({ restartRequired: true })
    await rootSelection
  })
  await screen.findByText(/Restart required\./)
  expect(screen.getByRole('button', { name: 'Create new install' })).toHaveProperty('disabled', true)
  expect(gateway.setupSelectRoot).not.toHaveBeenCalled()
  expect(restrictedServices.settings).not.toHaveBeenCalled()
  expect(restrictedServices.themeToggle).not.toHaveBeenCalled()
  expect(gateway.setupDetectHardware).not.toHaveBeenCalled()
  resetScreen()
  mount({ ...setupStatus, phase: 'recovery_required', recovery: { code: 'root_inaccessible', message: 'Access to the saved root was denied.' } })
  await screen.findByRole('button', { name: 'Locate existing install' })
  expect(screen.getByText(/permission/i)).toBeTruthy()
  vi.mocked(gateway.pickFolder).mockResolvedValue('F:/New Comfy')
  vi.mocked(gateway.setupNewInstall).mockResolvedValue({ restartRequired: true })
  fireEvent.click(screen.getByRole('button', { name: 'Create new install' }))
  await screen.findByText(/Restart required\./)
  expect(gateway.setupNewInstall).toHaveBeenCalledWith('F:/New Comfy')
  expect(screen.getByRole('button', { name: 'Locate existing install' })).toHaveProperty('disabled', true)
  expect(gateway.setupSelectRoot).not.toHaveBeenCalled()
  expect(restrictedServices.settings).not.toHaveBeenCalled()
  expect(restrictedServices.themeToggle).not.toHaveBeenCalled()
})

it('rebuild_shows_preserved_data_and_owned_replacement', async () => {
  for (const code of ['moved', 'interpreter_broken'] as const) {
    mount(rebuildStatus(code))
    const rebuild = await screen.findByRole('button', { name: 'Rebuild environment' })
    expect(document.body.textContent).toMatch(code === 'moved' ? /installation.*moved/i : /interpreter.*cannot run|interpreter.*broken/i)
    expect(screen.getByText('D:/Comfy')).toBeTruthy()
    expect(document.body.textContent).toMatch(/default/)
    expect(document.body.textContent).toMatch(/revision\s*1/i)
    expect(document.body.textContent).toMatch(/3\.13\.16/)
    expect(document.body.textContent).toMatch(/models/i)
    expect(document.body.textContent).toMatch(/custom nodes/i)
    expect(document.body.textContent).toMatch(/user.*input.*output/i)
    expect(document.body.textContent).toMatch(/external model roots/i)
    expect(document.body.textContent).toMatch(/launch options/i)
    expect(document.body.textContent).toMatch(/app-owned.*verification|verification.*app-owned/i)
    expect(document.body.textContent).toMatch(/previous.*adopted.*untouched/i)
    expect(document.body.textContent).toMatch(/checkout.*retain|retain.*checkout/i)
    expect(document.body.textContent).toMatch(/node requirements.*reinstall|reinstall.*node requirements/i)
    expect(document.body.textContent).toMatch(/does not run install\.py/i)
    expect(screen.queryByRole('button', { name: 'Locate existing install' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Create new install' })).toBeNull()
    expect(restrictedServices.settings).not.toHaveBeenCalled()
    expect(restrictedServices.themeToggle).not.toHaveBeenCalled()
    fireEvent.click(rebuild)
    await waitFor(() => expect(gateway.setupRun).toHaveBeenCalledWith({
      kind: 'new',
      request: {
        mode: 'rebuild',
        pythonSource: { kind: 'managed' },
        profileId: 'default',
        comfyuiRef: { kind: 'keepCurrent' },
        installManager: false,
        accelerators: [],
        curatedNodes: [],
        cleanCache: false,
        confirmAdoptedChanges: false,
        allowSourceBuilds: false,
        thirdPartyConsent: false,
      },
    }, expect.any(Function)))
    resetScreen()
    vi.mocked(gateway.setupRun).mockClear()
  }
  mount({ ...rebuildStatus(), activeEnv: null })
  await screen.findByText('Recorded environment unavailable')
  expect(screen.getByRole('button', { name: 'Rebuild environment' })).toHaveProperty('disabled', true)
  expect(gateway.setupRun).not.toHaveBeenCalled()
})

it('rebuild_uses_restricted_safe_ui_and_recovers_failed_run', async () => {
  mount({ ...rebuildStatus(), run })
  await screen.findByRole('button', { name: 'Rebuild environment' })
  expect(screen.queryByRole('button', { name: 'Retry same run' })).toBeNull()
  expect(gateway.setupRun).not.toHaveBeenCalled()
  expect(gateway.setupDetectHardware).not.toHaveBeenCalled()
  expect(gateway.setupListProfiles).not.toHaveBeenCalled()
  expect(gateway.setupListCuratedNodes).not.toHaveBeenCalled()
  expect(gateway.envInfo).not.toHaveBeenCalled()
  expect(restrictedServices.settings).not.toHaveBeenCalled()
  expect(restrictedServices.themeToggle).not.toHaveBeenCalled()
  expect(screen.queryByText('Normal application shell')).toBeNull()
  resetScreen()
  mount({ ...rebuildStatus(), run: { ...run, id: 'failed-rebuild', mode: 'rebuild', failure: { ...run.failure!, consent: null } } })
  await screen.findByRole('button', { name: 'Retry same run' })
  expect(gateway.setupRun).not.toHaveBeenCalled()
  vi.mocked(gateway.setupRun).mockRejectedValueOnce(new IpcClientError('relocation_recovery_required', 'This historical run belongs to the previous root. Start a fresh rebuild.'))
  fireEvent.click(screen.getByRole('button', { name: 'Retry same run' }))
  await waitFor(() => expect(screen.getAllByText(/historical run belongs to the previous root/).length).toBeGreaterThan(0))
  expect(gateway.setupRun).toHaveBeenCalledWith({ kind: 'retry', runId: 'failed-rebuild', managerConsent: null, curatedDecisions: [] }, expect.any(Function))
  expect(screen.getByRole('button', { name: 'Start fresh rebuild' })).toBeTruthy()
  expect(gateway.setupDetectHardware).not.toHaveBeenCalled()
  expect(gateway.setupListProfiles).not.toHaveBeenCalled()
  expect(gateway.setupListCuratedNodes).not.toHaveBeenCalled()
  expect(gateway.envInfo).not.toHaveBeenCalled()
  expect(restrictedServices.settings).not.toHaveBeenCalled()
  expect(restrictedServices.themeToggle).not.toHaveBeenCalled()
})

it('rebuild_waits_for_controlled_restart_after_success', async () => {
  mount(rebuildStatus())
  fireEvent.click(await screen.findByRole('button', { name: 'Rebuild environment' }))
  await waitFor(() => expect(gateway.setupRun).toHaveBeenCalledTimes(1))
  updateStatus({ ...rebuildStatus(), phase: 'run_in_progress', run: { ...run, id: 'curated-run', mode: 'rebuild', status: 'running', failure: null } })
  await screen.findByText(/Installation progress/)
  updateStatus({ ...rebuildStatus(), phase: 'ready', recovery: null, run: { ...run, id: 'curated-run', mode: 'rebuild', status: 'succeeded', failure: null } })
  await screen.findByText(/restart.*required|restarting/i)
  expect(screen.queryByText('Normal application shell')).toBeNull()
  expect(screen.queryByRole('button', { name: 'Continue' })).toBeNull()
  expect(screen.queryByRole('button', { name: 'Back to environment' })).toBeNull()
  expect(gateway.setupRun).toHaveBeenCalledTimes(1)
})

it('copied_successful_rebuild_requires_a_fresh_rebuild', async () => {
  mount({ ...rebuildStatus(), run: { ...run, mode: 'rebuild', status: 'succeeded', failure: null } })
  await screen.findByRole('button', { name: 'Rebuild environment' })
  expect(screen.queryByText(/Restart required|restarting/i)).toBeNull()
  expect(screen.queryByRole('button', { name: 'Retry same run' })).toBeNull()
  expect(screen.queryByText(/Installation progress/)).toBeNull()
  expect(screen.queryByText('Normal application shell')).toBeNull()
  expect(gateway.setupRun).not.toHaveBeenCalled()
})

it('rebuild_progress_survives_a_status_reconnect_error', async () => {
  mount({ ...rebuildStatus(), run: { ...run, mode: 'rebuild', failure: { ...run.failure!, consent: null } } })
  const retry = await screen.findByRole('button', { name: 'Retry same run' })
  vi.mocked(gateway.setupStatus).mockRejectedValueOnce(new IpcClientError('maintenance_busy', 'Status could not be refreshed.'))
  fireEvent.click(screen.getByRole('button', { name: 'Reconnect status' }))
  await waitFor(() => expect(queryClient.getQueryState(environmentKeys.status)?.status).toBe('error'))
  expect(screen.getByRole('button', { name: 'Retry same run' })).toBe(retry)
  expect(screen.getByRole('button', { name: 'Start fresh rebuild' })).toBeTruthy()
  expect(screen.queryByText('Normal application shell')).toBeNull()
  expect(gateway.setupRun).not.toHaveBeenCalled()
})

it('fresh_rebuild_keeps_its_run_after_a_rejected_retry', async () => {
  mount(rebuildStatus())
  fireEvent.click(await screen.findByRole('button', { name: 'Rebuild environment' }))
  await waitFor(() => expect(gateway.setupRun).toHaveBeenCalledTimes(1))
  updateStatus({
    ...rebuildStatus(),
    run: {
      ...run,
      id: 'curated-run',
      mode: 'rebuild',
      failure: { ...run.failure!, consent: null, message: 'Fresh rebuild verification failed.' },
    },
  })
  const retry = await screen.findByRole('button', { name: 'Retry same run' })
  let rejectRetry: (error: Error) => void = () => undefined
  const retryResult = new Promise<{ runId: string }>((_resolve, reject) => { rejectRetry = reject })
  vi.mocked(gateway.setupRun).mockReturnValueOnce(retryResult)
  fireEvent.click(retry)
  await waitFor(() => expect(gateway.setupRun).toHaveBeenCalledTimes(2))
  expect(screen.getByRole('button', { name: 'Retry same run' })).toHaveProperty('disabled', true)
  expect(screen.getByText('Fresh rebuild verification failed.')).toBeTruthy()
  await act(async () => {
    rejectRetry(new IpcClientError('maintenance_busy', 'The retry could not acquire maintenance.'))
    await retryResult.catch(() => undefined)
  })
  await waitFor(() => expect(screen.getAllByText('The retry could not acquire maintenance.').length).toBeGreaterThan(0))
  expect(screen.getByText(/Installation progress/)).toBeTruthy()
  expect(screen.getByText('Fresh rebuild verification failed.')).toBeTruthy()
  expect(screen.getByRole('button', { name: 'Retry same run' })).toHaveProperty('disabled', false)
  expect(screen.getByRole('button', { name: 'Start fresh rebuild' })).toHaveProperty('disabled', false)
  expect(gateway.setupRun).toHaveBeenLastCalledWith({
    kind: 'retry',
    runId: 'curated-run',
    managerConsent: null,
    curatedDecisions: [],
  }, expect.any(Function))
  expect(screen.queryByText('Normal application shell')).toBeNull()
  expect(restrictedServices.settings).not.toHaveBeenCalled()
  expect(restrictedServices.themeToggle).not.toHaveBeenCalled()
})

it('manifest_recovery_keeps_the_generic_guidance_route', async () => {
  for (const code of ['manifest_missing', 'manifest_invalid', 'bootstrap_corrupt']) {
    mount({ ...setupStatus, phase: 'recovery_required', recovery: { code, message: 'Restore the recorded manifest before continuing.' } })
    await screen.findByText('Restore the recorded manifest before continuing.')
    expect(screen.getByText(`${code} · D:/Comfy`)).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Recheck installation' })).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Locate existing install' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Create new install' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Rebuild environment' })).toBeNull()
    expect(gateway.setupDetectHardware).not.toHaveBeenCalled()
    expect(gateway.setupListProfiles).not.toHaveBeenCalled()
    expect(gateway.setupListCuratedNodes).not.toHaveBeenCalled()
    expect(gateway.setupRun).not.toHaveBeenCalled()
    resetScreen()
  }
})
