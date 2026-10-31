// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, configure, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { toast } from 'sonner'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'

import {
  gateway,
  IpcClientError,
  type BatchOutcomeDto,
  type NodeListDto,
  type NodeOutcomeDto,
  type PlanReviewDto,
  type StartResultDto,
} from '@/shared/ipc/gateway'

import { CustomNodesPage, type CustomNodesPageProps } from './custom-nodes-page'
import {
  gitStatus,
  node,
  nodeList,
  operation,
  outcome,
  plan,
  registryInstalled,
  registryPage,
} from './fixtures'

// Rendering many rows in jsdom is slow when the whole suite runs in parallel.
vi.setConfig({ testTimeout: 30_000 })
configure({ asyncUtilTimeout: 5000 })

vi.mock('@/shared/ipc/gateway', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/ipc/gateway')>()),
  gateway: {
    nodeList: vi.fn(),
    nodeSubscribe: vi.fn(),
    nodeUnsubscribe: vi.fn(),
    nodeOperationStatus: vi.fn(),
    nodeRegistrySearch: vi.fn(),
    nodeOpenRepository: vi.fn(),
    nodeReveal: vi.fn(),
    nodeGitStatus: vi.fn(),
    nodePrepareRegistry: vi.fn(),
    nodePrepareGit: vi.fn(),
    nodePrepareUpdate: vi.fn(),
    nodePrepareRetryDeps: vi.fn(),
    nodeCommit: vi.fn(),
    nodeSetEnabled: vi.fn(),
    nodeUninstall: vi.fn(),
    nodeUpdateAll: vi.fn(),
    nodeCancel: vi.fn(),
    nodeRescan: vi.fn(),
    nodeResolveOperation: vi.fn(),
  },
}))
vi.mock('sonner', () => ({ toast: { error: vi.fn(), success: vi.fn(), info: vi.fn() } }))
vi.mock('@/shared/ui/log-view', () => ({
  LogView: ({ lines }: { lines: string[] }) => <pre data-testid="log">{lines.join('\n')}</pre>,
}))

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (error: unknown) => void
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail })
  return { promise, resolve, reject }
}

const props: CustomNodesPageProps = {
  maintenanceBusy: false,
  runtime: null,
  runtimeBusy: false,
  onRepair: vi.fn(),
  onStopComfy: vi.fn(),
  onRestartComfy: vi.fn(),
  onOpenComfy: vi.fn(),
}

beforeEach(() => {
  vi.resetAllMocks()
  vi.stubGlobal(
    'ResizeObserver',
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  )
  vi.mocked(gateway.nodeList).mockResolvedValue(nodeList())
  vi.mocked(gateway.nodeSubscribe).mockResolvedValue(1)
  vi.mocked(gateway.nodeUnsubscribe).mockResolvedValue(null)
  vi.mocked(gateway.nodeCancel).mockResolvedValue(null)
  vi.mocked(gateway.nodeRegistrySearch).mockResolvedValue(registryPage())
  vi.mocked(gateway.nodeOperationStatus).mockResolvedValue(operation())
  vi.mocked(gateway.nodeGitStatus).mockResolvedValue(gitStatus())
  vi.mocked(gateway.nodePrepareRegistry).mockResolvedValue(plan())
  vi.mocked(gateway.nodePrepareGit).mockResolvedValue(plan({
    name: 'pack',
    source: { kind: 'git', repository: 'https://github.com/example/pack', commit: 'e'.repeat(40), branch: 'main' },
  }))
  vi.mocked(gateway.nodePrepareUpdate).mockResolvedValue(plan({ kind: 'update' }))
  vi.mocked(gateway.nodePrepareRetryDeps).mockResolvedValue(plan({ kind: 'retry_dependencies' }))
  vi.mocked(gateway.nodeCommit).mockResolvedValue(outcome())
  vi.mocked(props.onStopComfy).mockResolvedValue(undefined)
})
afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

function ui(overrides: Partial<CustomNodesPageProps> = {}) {
  return (
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <CustomNodesPage {...props} {...overrides} />
    </QueryClientProvider>
  )
}
const mount = (overrides: Partial<CustomNodesPageProps> = {}) => render(ui(overrides))

const button = (name: string) => screen.getByRole<HTMLButtonElement>('button', { name })
const executeBox = () => screen.getByRole('checkbox', { name: /execute third-party code/ })
const isChecked = (element: HTMLElement) => element.getAttribute('aria-checked') === 'true'

async function loaded() {
  await screen.findByText('ComfyUI-Example', { selector: 'p.truncate' })
}

async function prepareFromGit(url = 'https://github.com/example/pack') {
  fireEvent.click(await screen.findByRole('button', { name: 'Install from Git' }))
  fireEvent.change(await screen.findByLabelText('Repository URL'), { target: { value: url } })
  fireEvent.click(screen.getByRole('button', { name: 'Prepare review' }))
}

async function installAndApply() {
  await prepareFromGit()
  await screen.findByText('Review custom node plan')
  fireEvent.click(executeBox())
  fireEvent.click(button('Apply reviewed plans'))
}

it('registry_install_requires_exact_review_before_commit', async () => {
  mount()
  await loaded()
  fireEvent.click(button('Browse Registry'))
  fireEvent.click(await screen.findByRole('button', { name: 'Install Example Pack' }))
  fireEvent.change(await screen.findByLabelText('Version (optional)'), { target: { value: ' 1.2.3 ' } })
  fireEvent.click(screen.getByRole('button', { name: 'Prepare review' }))
  await screen.findByText('Review custom node plan')
  expect(gateway.nodePrepareRegistry).toHaveBeenCalledWith({
    requestId: expect.any(String) as string, id: 'example-pack', version: '1.2.3',
  })
  // Preparation alone never commits.
  expect(gateway.nodeCommit).not.toHaveBeenCalled()
  expect(screen.getByText('example-pack@1.2.3')).toBeTruthy()
  expect(screen.getByText('Tree identity')).toBeTruthy()
  expect(button('Apply reviewed plans').disabled).toBe(true)
  fireEvent.click(executeBox())
  fireEvent.click(button('Apply reviewed plans'))
  await waitFor(() => expect(gateway.nodeCommit).toHaveBeenCalledTimes(1))
  expect(gateway.nodeCommit).toHaveBeenCalledWith(expect.objectContaining({
    planId: 'plan-1', binding: 'binding-1', executeNodeCode: true, runInstallPy: false,
  }))
  expect(await screen.findByText('Installed example-pack.')).toBeTruthy()
  expect(screen.getByRole('button', { name: 'Restart ComfyUI' })).toBeTruthy()
})

it('reload_rediscovers_review_without_consent', async () => {
  vi.mocked(gateway.nodeList).mockResolvedValue(nodeList({
    pendingReviews: [plan({ trustedSource: true, trustedInstallPy: true, hasInstallPy: true })],
  }))
  mount()
  expect(await screen.findByText('A prepared plan is waiting for review')).toBeTruthy()
  fireEvent.click(button('Resume review'))
  await screen.findByText('Review custom node plan')
  expect(isChecked(executeBox())).toBe(false)
  expect(isChecked(screen.getByRole('checkbox', { name: /install\.py script/ }))).toBe(false)
  expect(button('Apply reviewed plans').disabled).toBe(true)
  expect(gateway.nodeCommit).not.toHaveBeenCalled()
  // An idle review is discarded explicitly, by plan ID.
  fireEvent.click(button('Discard plans'))
  await waitFor(() => expect(gateway.nodeCancel).toHaveBeenCalledWith('plan-1'))
})

it('failed_operation_remains_visible_after_remount', async () => {
  vi.mocked(gateway.nodeList).mockResolvedValue(nodeList({
    recent: [operation({ id: 'op-9', phase: 'failed', nodeName: 'Broken Pack', failure: 'Installing the dependencies failed.' })],
    unresolved: [operation({ id: 'op-10', phase: 'published', nodeName: 'Half Done', blocksStart: true })],
  }))
  const first = mount()
  expect(await screen.findByText('Installing the dependencies failed.')).toBeTruthy()
  expect(screen.getByText('Broken Pack')).toBeTruthy()
  expect(screen.getByText(/blocks ComfyUI from starting/)).toBeTruthy()
  first.unmount()
  mount()
  expect(await screen.findByText('Installing the dependencies failed.')).toBeTruthy()
  expect(screen.getByRole('button', { name: 'Rescan / finish recovery' })).toBeTruthy()
  // The persisted failure shows even when no in-memory log tail exists.
  fireEvent.click(screen.getByRole('button', { name: /Show details of the install of Broken Pack/ }))
  expect(await screen.findByText(/No log lines are available/)).toBeTruthy()
})

it('shows the authoritative log tail of the selected operation', async () => {
  vi.mocked(gateway.nodeList).mockResolvedValue(nodeList({
    recent: [operation({ id: 'op-9', phase: 'failed', nodeName: 'Broken Pack', failure: 'x' })],
  }))
  vi.mocked(gateway.nodeOperationStatus).mockResolvedValue(
    operation({ id: 'op-9', phase: 'failed', nodeName: 'Broken Pack', failure: 'x', logTail: ['pip failed', 'exit 1'] }),
  )
  mount()
  fireEvent.click(await screen.findByRole('button', { name: /Show details of the install of Broken Pack/ }))
  const log = await screen.findByTestId('log')
  expect(log.textContent).toBe('pip failed\nexit 1')
})

it('git_refusal_displays_status', async () => {
  vi.mocked(gateway.nodePrepareUpdate).mockRejectedValue(new IpcClientError(
    'node_git_refused', 'The repository has local changes, an unfinished operation, a diverged branch or no selected upstream, so it was not changed.',
  ))
  vi.mocked(gateway.nodeGitStatus)
    .mockResolvedValueOnce(gitStatus())
    .mockResolvedValue(gitStatus({ dirty: true, status: ' M requirements.txt\n?? scratch.py' }))
  mount()
  await loaded()
  fireEvent.click(button('Update'))
  const prepare = await screen.findByRole('button', { name: 'Prepare update' })
  await waitFor(() => expect((prepare as HTMLButtonElement).disabled).toBe(false))
  fireEvent.click(prepare)
  expect(await screen.findByText(/so it was not changed/)).toBeTruthy()
  expect(await screen.findByText(/M requirements\.txt/)).toBeTruthy()
  expect(screen.getByText('Git status')).toBeTruthy()
  expect(gateway.nodeGitStatus).toHaveBeenCalledWith('node-1')
})

it('refuses a dirty repository before preparing and asks for a target when detached', async () => {
  vi.mocked(gateway.nodeGitStatus).mockResolvedValueOnce(gitStatus({ dirty: true, status: ' M a.py' }))
  mount()
  await loaded()
  fireEvent.click(button('Update'))
  expect(await screen.findByText('This repository will not be updated')).toBeTruthy()
  expect(screen.getByText(/M a\.py/)).toBeTruthy()
  expect(button('Prepare update').disabled).toBe(true)
  vi.mocked(gateway.nodeGitStatus).mockResolvedValue(gitStatus({ branch: null, hasUpstream: false, ahead: null, behind: null }))
  fireEvent.click(screen.getAllByRole('button', { name: 'Inspect again' })[0])
  const reference = await screen.findByLabelText('Branch, tag or commit (required)')
  expect(button('Prepare update').disabled).toBe(true)
  fireEvent.change(reference, { target: { value: 'v1.4.0' } })
  fireEvent.click(button('Prepare update'))
  await waitFor(() => expect(gateway.nodePrepareUpdate).toHaveBeenCalledWith({
    requestId: expect.any(String) as string, nodeId: 'node-1', reference: 'v1.4.0',
  }))
})

it('stop_is_awaited_before_retry', async () => {
  const stop = deferred<void>()
  vi.mocked(props.onStopComfy).mockReturnValue(stop.promise)
  vi.mocked(gateway.nodePrepareGit).mockRejectedValueOnce(new IpcClientError(
    'comfyui_running', 'ComfyUI must be stopped before changing custom nodes. If it is already stopped, close other programs that use the ComfyUI folder, such as a terminal opened there, and retry.',
  ))
  mount()
  await loaded()
  await prepareFromGit()
  const retry = await screen.findByRole('button', { name: 'Stop ComfyUI and retry' })
  expect(gateway.nodePrepareGit).toHaveBeenCalledTimes(1)
  fireEvent.click(retry)
  await waitFor(() => expect(props.onStopComfy).toHaveBeenCalledTimes(1))
  await new Promise((resolve) => setTimeout(resolve, 30))
  // The refused request is not repeated until the stop has finished.
  expect(gateway.nodePrepareGit).toHaveBeenCalledTimes(1)
  stop.resolve()
  await waitFor(() => expect(gateway.nodePrepareGit).toHaveBeenCalledTimes(2))
  await screen.findByText('Review custom node plan')
})

it('stale_commit_requires_new_review', async () => {
  vi.mocked(gateway.nodeCommit).mockRejectedValueOnce(new IpcClientError(
    'node_plan_stale', 'The reviewed plan is no longer current. Prepare it again.',
  ))
  vi.mocked(gateway.nodePrepareGit)
    .mockResolvedValueOnce(plan({ id: 'old', binding: 'old-binding', name: 'pack' }))
    .mockResolvedValueOnce(plan({ id: 'new', binding: 'new-binding', name: 'pack' }))
  mount()
  await loaded()
  await installAndApply()
  expect((await screen.findAllByText(/no longer current/)).length).toBeGreaterThan(0)
  expect(screen.queryByText('Review custom node plan')).toBeNull()
  expect(gateway.nodeCommit).toHaveBeenCalledTimes(1)
  fireEvent.click(button('Prepare again'))
  await screen.findByText('Review custom node plan')
  expect(gateway.nodePrepareGit).toHaveBeenCalledTimes(2)
  // The old confirmation is gone and nothing is applied until the new plan is confirmed.
  expect(isChecked(executeBox())).toBe(false)
  expect(button('Apply reviewed plans').disabled).toBe(true)
  expect(gateway.nodeCommit).toHaveBeenCalledTimes(1)
})

it('uninstall_confirms_trash_and_shared_packages', async () => {
  vi.mocked(gateway.nodeUninstall).mockResolvedValue(outcome({ node: null }))
  mount()
  await loaded()
  fireEvent.click(button('Uninstall'))
  const dialog = await screen.findByRole('dialog')
  expect(within(dialog).getByText(/Recycle Bin|Trash/)).toBeTruthy()
  expect(within(dialog).getByText(/Python packages.*remain in the shared environment/)).toBeTruthy()
  expect(within(dialog).getByText(/Nothing is deleted permanently/)).toBeTruthy()
  expect(gateway.nodeUninstall).not.toHaveBeenCalled()
  fireEvent.click(within(dialog).getByRole('button', { name: 'Uninstall node' }))
  await waitFor(() => expect(gateway.nodeUninstall).toHaveBeenCalledWith('node-1'))
  expect(await screen.findByText('Uninstalled ComfyUI-Example.')).toBeTruthy()
})

it('confirms_the_move_and_resulting_state_before_disabling', async () => {
  vi.mocked(gateway.nodeSetEnabled).mockResolvedValue(outcome())
  mount()
  await loaded()
  fireEvent.click(button('Disable'))
  const dialog = await screen.findByRole('dialog')
  expect(within(dialog).getByText(/custom_nodes\/\.disabled/)).toBeTruthy()
  expect(within(dialog).getByText(/Resulting state: disabled/)).toBeTruthy()
  fireEvent.click(within(dialog).getByRole('button', { name: 'Disable node' }))
  await waitFor(() => expect(gateway.nodeSetEnabled).toHaveBeenCalledWith({ nodeId: 'node-1', enabled: false }))
})

it('repair_mode_keeps_supported_recovery_actions', async () => {
  vi.mocked(gateway.nodeList).mockResolvedValue(nodeList({
    repairRequired: true,
    nodes: [node({ dependencies: 'failed' })],
  }))
  mount()
  await loaded()
  expect(await screen.findByText('This environment needs repair')).toBeTruthy()
  expect(button('Update').disabled).toBe(true)
  expect(button('Install from Git').disabled).toBe(true)
  expect(button('Update all (1)').disabled).toBe(true)
  expect(button('Retry dependencies').disabled).toBe(false)
  expect(button('Disable').disabled).toBe(false)
  expect(button('Uninstall').disabled).toBe(false)
  fireEvent.click(button('Browse Registry'))
  expect((await screen.findByRole<HTMLButtonElement>('button', { name: 'Install Example Pack' })).disabled).toBe(true)
  fireEvent.click(button('Repair environment'))
  expect(props.onRepair).toHaveBeenCalledTimes(1)
})

it('successful_retry_does_not_clear_repair_banner', async () => {
  vi.mocked(gateway.nodeList).mockResolvedValue(nodeList({
    repairRequired: true,
    nodes: [node({ dependencies: 'failed' })],
  }))
  mount()
  await loaded()
  fireEvent.click(button('Retry dependencies'))
  await screen.findByText('Review custom node plan')
  fireEvent.click(executeBox())
  fireEvent.click(button('Apply reviewed plans'))
  expect(await screen.findByText('Retried the dependencies of example-pack.')).toBeTruthy()
  await waitFor(() => expect(vi.mocked(gateway.nodeList).mock.calls.length).toBeGreaterThan(1))
  expect(screen.getByText('This environment needs repair')).toBeTruthy()
  // Restart is not offered while the environment still needs repair.
  expect(screen.queryByRole('button', { name: 'Restart ComfyUI' })).toBeNull()
  expect(props.onRepair).not.toHaveBeenCalled()
})

it('cancel_uses_request_plan_and_batch_keys', async () => {
  // 1. Preparation: the client request ID, and a plan that races with the cancellation is discarded.
  const preparing = deferred<PlanReviewDto>()
  vi.mocked(gateway.nodePrepareGit).mockReturnValueOnce(preparing.promise)
  const first = mount()
  await loaded()
  await prepareFromGit()
  await waitFor(() => expect(gateway.nodePrepareGit).toHaveBeenCalledTimes(1))
  const requestId = vi.mocked(gateway.nodePrepareGit).mock.calls[0][0].requestId
  fireEvent.click(await screen.findByRole('button', { name: 'Cancel preparation' }))
  expect(gateway.nodeCancel).toHaveBeenCalledWith(requestId)
  expect(await screen.findByText(/Cancellation requested/)).toBeTruthy()
  preparing.resolve(plan({ id: 'raced', name: 'pack' }))
  await waitFor(() => expect(gateway.nodeCancel).toHaveBeenCalledWith('raced'))
  expect(screen.queryByText('Review custom node plan')).toBeNull()
  first.unmount()

  // 2. A single commit is cancelled by its plan ID.
  vi.mocked(gateway.nodeCancel).mockClear()
  const commit = deferred<NodeOutcomeDto>()
  vi.mocked(gateway.nodeCommit).mockReturnValueOnce(commit.promise)
  vi.mocked(gateway.nodePrepareGit).mockResolvedValue(plan({ id: 'plan-7', binding: 'b7', name: 'pack' }))
  const second = mount()
  await loaded()
  await installAndApply()
  fireEvent.click(await screen.findByRole('button', { name: 'Request cancellation' }))
  expect(gateway.nodeCancel).toHaveBeenCalledWith('plan-7')
  commit.resolve(outcome())
  await screen.findByText('Installed pack.')
  second.unmount()

  // 3. A batch is cancelled by the literal update-all key.
  vi.mocked(gateway.nodeCancel).mockClear()
  const batch = deferred<BatchOutcomeDto>()
  vi.mocked(gateway.nodeUpdateAll).mockReturnValueOnce(batch.promise)
  vi.mocked(gateway.nodeList).mockResolvedValue(nodeList({ nodes: [registryInstalled({ name: 'Reg Pack' })] }))
  vi.mocked(gateway.nodePrepareUpdate).mockResolvedValue(plan({ id: 'plan-b', binding: 'bb', kind: 'update', name: 'Reg Pack' }))
  mount()
  fireEvent.click(await screen.findByRole('button', { name: 'Update all (1)' }))
  fireEvent.click(await screen.findByRole('button', { name: 'Prepare 1 update' }))
  await screen.findByText('Review custom node plan')
  fireEvent.click(executeBox())
  fireEvent.click(button('Apply reviewed plans'))
  fireEvent.click(await screen.findByRole('button', { name: 'Request cancellation' }))
  expect(gateway.nodeCancel).toHaveBeenCalledWith('update-all')
  batch.resolve({ items: [{ planId: 'plan-b', outcome: outcome(), error: null }], stoppedForRepair: false })
  await screen.findByText('Update results')
})

it('unmount_does_not_cancel_inflight_work', async () => {
  const preparing = deferred<PlanReviewDto>()
  vi.mocked(gateway.nodePrepareGit).mockReturnValueOnce(preparing.promise)
  const view = mount()
  await loaded()
  await prepareFromGit()
  await screen.findByRole('button', { name: 'Cancel preparation' })
  view.unmount()
  preparing.resolve(plan({ id: 'late', name: 'pack' }))
  await new Promise((resolve) => setTimeout(resolve, 30))
  expect(gateway.nodeCancel).not.toHaveBeenCalled()
  expect(gateway.nodeUnsubscribe).toHaveBeenCalled()

  // An open review is not discarded by unmounting either.
  vi.mocked(gateway.nodeList).mockResolvedValue(nodeList({ pendingReviews: [plan()] }))
  const second = mount()
  fireEvent.click(await screen.findByRole('button', { name: 'Resume review' }))
  await screen.findByText('Review custom node plan')
  second.unmount()
  await new Promise((resolve) => setTimeout(resolve, 30))
  expect(gateway.nodeCancel).not.toHaveBeenCalled()
})

async function succeedAndRestart(result: Promise<StartResultDto>) {
  vi.mocked(props.onRestartComfy).mockReturnValue(result)
  mount()
  await loaded()
  await installAndApply()
  fireEvent.click(await screen.findByRole('button', { name: 'Restart ComfyUI' }))
}

it('restart_port_conflict_is_not_reported_as_started', async () => {
  await succeedAndRestart(Promise.resolve({ kind: 'port_conflict', requested: 8188, suggested: 8189 }))
  expect(await screen.findByText('Port 8188 is in use, so ComfyUI was not started')).toBeTruthy()
  expect(screen.queryByText('ComfyUI was restarted')).toBeNull()
  expect(screen.getByText(/Port 8189 is free, but it is not used automatically/)).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: 'Open ComfyUI launch options' }))
  expect(props.onOpenComfy).toHaveBeenCalledTimes(1)
  expect(props.onRestartComfy).toHaveBeenCalledTimes(1)
})

it('reports_a_started_restart_only_when_the_runtime_started', async () => {
  await succeedAndRestart(Promise.resolve({
    kind: 'started',
    status: { lifecycle: 'running', session: null, failure: null, pendingOperation: null, recovery: null, origin: null, pendingCloseRequest: null },
  }))
  expect(await screen.findByText('ComfyUI was restarted')).toBeTruthy()
})

it('does_not_offer_a_restart_while_an_operation_blocks_launch', async () => {
  vi.mocked(gateway.nodeList).mockResolvedValue(nodeList({
    unresolved: [operation({ id: 'op-3', phase: 'published', blocksStart: true, nodeName: 'Half Done' })],
  }))
  mount()
  await loaded()
  await installAndApply()
  expect(await screen.findByText('Restart ComfyUI to load the changes')).toBeTruthy()
  expect(screen.queryByRole('button', { name: 'Restart ComfyUI' })).toBeNull()
  expect(screen.getByText(/Restart is unavailable/)).toBeTruthy()
})

it('offers_rescan_and_shows_its_failure', async () => {
  vi.mocked(gateway.nodeRescan).mockRejectedValue(new IpcClientError('node_rescan_failed', 'The rescan failed.'))
  mount()
  await loaded()
  fireEvent.click(screen.getByRole('button', { name: 'Rescan / finish recovery' }))
  expect(await screen.findByText('The rescan failed')).toBeTruthy()
  expect(gateway.nodeRescan).toHaveBeenCalledTimes(1)
})

it('confirms_keeping_files_and_moves_the_closed_operation_out_of_unfinished', async () => {
  const interrupted = operation({ id: 'op-interrupted', phase: 'prepared', blocksStart: true, nodeName: 'Interrupted Pack' })
  const closed = nodeList({ recent: [operation({ ...interrupted, phase: 'failed', blocksStart: false })] })
  vi.mocked(gateway.nodeList).mockResolvedValue(nodeList({ unresolved: [interrupted] }))
  vi.mocked(gateway.nodeResolveOperation).mockImplementation(() => {
    vi.mocked(gateway.nodeList).mockResolvedValue(closed)
    return Promise.resolve(closed)
  })
  mount()
  fireEvent.click(await screen.findByRole('button', { name: 'Keep the files of Interrupted Pack as they are' }))
  const dialog = await screen.findByRole('dialog', { name: 'Close the interrupted install of Interrupted Pack?' })
  expect(within(dialog).getByText(/custom_nodes\/\.disabled/)).toBeTruthy()
  expect(within(dialog).getByText(/Dependencies are not installed or repaired/)).toBeTruthy()
  expect(gateway.nodeResolveOperation).not.toHaveBeenCalled()
  fireEvent.click(within(dialog).getByRole('button', { name: 'Keep files as they are' }))
  await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  expect(gateway.nodeResolveOperation).toHaveBeenCalledExactlyOnceWith('op-interrupted')
  expect(screen.queryByRole('heading', { name: 'Unfinished' })).toBeNull()
  expect(screen.getByRole('heading', { name: 'Recent results (latest 16)' })).toBeTruthy()
  expect(screen.getByText('Interrupted Pack')).toBeTruthy()
  expect(screen.queryByRole('button', { name: /Keep the files of/ })).toBeNull()
})

it('keeping_files_clears_a_previous_rescan_failure', async () => {
  const interrupted = operation({ id: 'op-interrupted', phase: 'prepared', blocksStart: true })
  const resolved = nodeList()
  vi.mocked(gateway.nodeList).mockResolvedValue(nodeList({ unresolved: [interrupted] }))
  vi.mocked(gateway.nodeRescan).mockRejectedValue(new IpcClientError('node_rescan_failed', 'The rescan failed.'))
  vi.mocked(gateway.nodeResolveOperation).mockImplementation(() => {
    vi.mocked(gateway.nodeList).mockResolvedValue(resolved)
    return Promise.resolve(resolved)
  })
  mount()
  await loaded()
  fireEvent.click(button('Rescan / finish recovery'))
  expect((await screen.findByText('The rescan failed')).closest('[role="alert"]')).toBeTruthy()
  fireEvent.click(button('Keep the files of ComfyUI-Example as they are'))
  fireEvent.click(within(await screen.findByRole('dialog')).getByRole('button', { name: 'Keep files as they are' }))
  await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  expect(gateway.nodeResolveOperation).toHaveBeenCalledExactlyOnceWith('op-interrupted')
  expect(screen.queryByRole('heading', { name: 'Unfinished' })).toBeNull()
  expect(screen.queryByText('The rescan failed')).toBeNull()
})

it('cancelling_keep_files_does_not_resolve_the_operation', async () => {
  vi.mocked(gateway.nodeList).mockResolvedValue(nodeList({
    unresolved: [operation({ phase: 'published', blocksStart: true })],
  }))
  mount()
  fireEvent.click(await screen.findByRole('button', { name: 'Keep the files of ComfyUI-Example as they are' }))
  fireEvent.click(within(await screen.findByRole('dialog')).getByRole('button', { name: 'Cancel' }))
  await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  expect(gateway.nodeResolveOperation).not.toHaveBeenCalled()
  expect(screen.getByRole('heading', { name: 'Unfinished' })).toBeTruthy()
})

it('keep_files_failure_keeps_the_dialog_open_and_shows_the_failed_alert', async () => {
  vi.mocked(gateway.nodeList).mockResolvedValue(nodeList({
    unresolved: [operation({ phase: 'published', blocksStart: true })],
  }))
  vi.mocked(gateway.nodeResolveOperation).mockRejectedValue(new IpcClientError('node_storage', 'The journal is unavailable.'))
  mount()
  fireEvent.click(await screen.findByRole('button', { name: 'Keep the files of ComfyUI-Example as they are' }))
  fireEvent.click(within(await screen.findByRole('dialog')).getByRole('button', { name: 'Keep files as they are' }))
  const title = await screen.findByText('The operation could not be closed')
  expect(title.closest('[role="alert"]')?.textContent).toContain('The journal is unavailable.')
  expect(screen.getByRole('dialog')).toBeTruthy()
  expect(within(screen.getByRole('dialog')).getByRole<HTMLButtonElement>('button', { name: 'Keep files as they are' }).disabled).toBe(false)
})

it('disables_recovery_buttons_and_prevents_closing_while_resolving', async () => {
  const pending = deferred<NodeListDto>()
  vi.mocked(gateway.nodeList).mockResolvedValue(nodeList({
    unresolved: [operation({ id: 'op-interrupted', phase: 'published', blocksStart: true })],
  }))
  vi.mocked(gateway.nodeResolveOperation).mockReturnValue(pending.promise)
  mount()
  const keep = await screen.findByRole<HTMLButtonElement>('button', { name: 'Keep the files of ComfyUI-Example as they are' })
  const rescan = button('Rescan / finish recovery')
  fireEvent.click(keep)
  const dialog = await screen.findByRole('dialog')
  fireEvent.click(within(dialog).getByRole('button', { name: 'Keep files as they are' }))
  expect(within(dialog).getByRole<HTMLButtonElement>('button', { name: 'Closing…' }).disabled).toBe(true)
  expect(within(dialog).getByRole<HTMLButtonElement>('button', { name: 'Cancel' }).disabled).toBe(true)
  expect(within(dialog).queryByRole('button', { name: 'Close' })).toBeNull()
  expect(keep.disabled).toBe(true)
  expect(rescan.disabled).toBe(true)
  fireEvent.keyDown(dialog, { key: 'Escape' })
  expect(screen.getByRole('dialog')).toBeTruthy()
  expect(gateway.nodeResolveOperation).toHaveBeenCalledExactlyOnceWith('op-interrupted')
  vi.mocked(gateway.nodeList).mockResolvedValue(nodeList())
  pending.resolve(nodeList())
  await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
})

it('shows_a_node_recovery_failure_from_a_commit_with_a_rescan_action', async () => {
  vi.mocked(gateway.nodePrepareGit).mockRejectedValueOnce(new IpcClientError(
    'node_recovery_required', 'An interrupted custom node operation must be resolved first. Rescan the custom nodes.',
  ))
  vi.mocked(gateway.nodeRescan).mockResolvedValue(nodeList())
  mount()
  await loaded()
  await prepareFromGit()
  fireEvent.click(await screen.findByRole('button', { name: 'Rescan nodes' }))
  await waitFor(() => expect(gateway.nodeRescan).toHaveBeenCalledTimes(1))
})

it('opens_the_custom_nodes_folder_from_the_toolbar', async () => {
  vi.mocked(gateway.nodeReveal).mockResolvedValue(null)
  mount()
  await loaded()
  fireEvent.click(button('Open folder'))
  await waitFor(() => expect(gateway.nodeReveal).toHaveBeenCalledTimes(1))
  expect(gateway.nodeReveal).toHaveBeenCalledWith(null)
})

it('shows_the_active_node_in_the_system_file_manager_and_toasts_a_failure', async () => {
  vi.mocked(gateway.nodeReveal).mockRejectedValue(
    new IpcClientError('explorer_failed', 'The file manager could not be opened.'),
  )
  mount()
  await loaded()
  fireEvent.click(button('Show in Explorer'))
  expect(gateway.nodeReveal).toHaveBeenCalledWith('node-1')
  await waitFor(() => expect(toast.error).toHaveBeenCalledWith('The file manager could not be opened.'))
})
