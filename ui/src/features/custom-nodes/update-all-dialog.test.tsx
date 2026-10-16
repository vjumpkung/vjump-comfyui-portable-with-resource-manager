// @vitest-environment jsdom
import { cleanup, configure, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { ComponentProps } from 'react'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'

import {
  gateway,
  IpcClientError,
  type BatchOutcomeDto,
  type CommitInput,
  type InstalledNodeDto,
  type PlanReviewDto,
} from '@/shared/ipc/gateway'

import { gitStatus, manyNodes, node, outcome, plan, registryInstalled } from './fixtures'
import { UpdateAllDialog } from './update-all-dialog'

// Rendering many rows in jsdom is slow when the whole suite runs in parallel.
vi.setConfig({ testTimeout: 30_000 })
configure({ asyncUtilTimeout: 5000 })

vi.mock('@/shared/ipc/gateway', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/ipc/gateway')>()),
  gateway: {
    nodeGitStatus: vi.fn(),
    nodePrepareUpdate: vi.fn(),
    nodeUpdateAll: vi.fn(),
    nodeCancel: vi.fn(),
  },
}))

// The consent rules have their own tests; here the review is a thin stand-in that accepts everything.
vi.mock('./review-dialog', async () => {
  const model = await import('./model')
  type Props = ComponentProps<typeof import('./review-dialog').ReviewDialog>
  return {
    ReviewDialog: (props: Props) => (
      <div role="dialog" aria-label="Review">
        <p>Review of {props.plans.length} plans</p>
        {props.notice}
        <button disabled={props.busy} onClick={() => props.onCommit(
          props.plans.map((item) => model.decision(item, { ...model.EMPTY_CONSENT, execute: true })),
        )}>Apply</button>
        <button disabled={props.busy} onClick={props.onClose}>Discard</button>
        {props.onCancel && <button onClick={props.onCancel}>Request cancellation</button>}
      </div>
    ),
  }
})

const onSettled = vi.fn()
const onClose = vi.fn()
const onStopComfy = vi.fn()

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (error: unknown) => void
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail })
  return { promise, resolve, reject }
}

const planFor = (nodeId: string): PlanReviewDto =>
  plan({ id: `plan-${nodeId}`, binding: `binding-${nodeId}`, name: nodeId, kind: 'update' })

const batchOf = (items: CommitInput[], stoppedForRepair = false): BatchOutcomeDto => ({
  items: items.map((item) => ({ planId: item.planId, outcome: outcome(), error: null })),
  stoppedForRepair,
})

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
  vi.mocked(gateway.nodePrepareUpdate).mockImplementation(({ nodeId }) =>
    Promise.resolve(planFor(nodeId)))
  vi.mocked(gateway.nodeCancel).mockResolvedValue(null)
  vi.mocked(gateway.nodeUpdateAll).mockImplementation((items) => Promise.resolve(batchOf(items)))
  onStopComfy.mockResolvedValue(undefined)
})
afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

function mount(nodes: readonly InstalledNodeDto[]) {
  return render(
    <UpdateAllDialog nodes={nodes} onStopComfy={onStopComfy} onSettled={onSettled} onClose={onClose} />,
  )
}

const start = (count: number) =>
  fireEvent.click(screen.getByRole('button', { name: new RegExp(`Prepare ${count} update`) }))
const apply = () => fireEvent.click(screen.getByRole('button', { name: 'Apply' }))
const rowOf = (name: string) => screen.getByText(name).closest('li') as HTMLElement

it('prepares_and_commits_sequentially', async () => {
  const first = deferred<PlanReviewDto>()
  vi.mocked(gateway.nodePrepareUpdate).mockImplementationOnce(() => first.promise)
  const a = registryInstalled({ id: 'a', name: 'a' })
  const b = registryInstalled({ id: 'b', name: 'b' })
  mount([a, b])
  start(2)
  await waitFor(() => expect(gateway.nodePrepareUpdate).toHaveBeenCalledTimes(1))
  // The second node waits for the first preparation to finish.
  await new Promise((resolve) => setTimeout(resolve, 30))
  expect(gateway.nodePrepareUpdate).toHaveBeenCalledTimes(1)
  first.resolve(planFor('a'))
  await waitFor(() => expect(gateway.nodePrepareUpdate).toHaveBeenCalledTimes(2))
  expect(vi.mocked(gateway.nodePrepareUpdate).mock.calls.map(([input]) => input.nodeId)).toEqual(['a', 'b'])
  await screen.findByText('Review of 2 plans')
  expect(gateway.nodeUpdateAll).not.toHaveBeenCalled()
  apply()
  await waitFor(() => expect(gateway.nodeUpdateAll).toHaveBeenCalledTimes(1))
  const items = vi.mocked(gateway.nodeUpdateAll).mock.calls[0][0]
  expect(items.map((item) => item.planId)).toEqual(['plan-a', 'plan-b'])
  expect(items.every((item) => item.executeNodeCode)).toBe(true)
  await screen.findByText('Update results')
  expect(onSettled).toHaveBeenCalledWith(2)
  expect(within(rowOf('a')).getByText('Updated')).toBeTruthy()
  expect(within(rowOf('b')).getByText('Updated')).toBeTruthy()
})

it('reports_preparation_and_commit_results_per_node', async () => {
  const names = ['refused', 'current', 'cleanup', 'broken', 'detached', 'dirty-git']
  const [refused, current, cleanup_, broken] = names.map((name) => registryInstalled({ id: name, name }))
  const detached = node({ id: 'detached', name: 'detached' })
  const dirtyGit = node({ id: 'dirty-git', name: 'dirty-git' })
  const manual = node({ id: 'manual', name: 'manual', source: { kind: 'manual' } })
  vi.mocked(gateway.nodeGitStatus).mockImplementation((id) =>
    Promise.resolve(id === 'detached'
      ? gitStatus({ branch: null })
      : gitStatus({ dirty: true, status: ' M a.py' })))
  vi.mocked(gateway.nodePrepareUpdate).mockImplementation(({ nodeId }) => {
    if (nodeId === 'refused') return Promise.reject(new IpcClientError('node_resolution_failed', 'Could not resolve.'))
    if (nodeId === 'current') return Promise.reject(new IpcClientError('node_up_to_date', 'The node is already up to date.'))
    return Promise.resolve(planFor(nodeId))
  })
  vi.mocked(gateway.nodeUpdateAll).mockImplementation((items) => Promise.resolve({
    items: items.map((item) => item.planId === 'plan-broken'
      ? { planId: item.planId, outcome: null, error: { code: 'node_dependencies_failed', message: 'Installing the dependencies failed.' } }
      : { planId: item.planId, outcome: outcome({ cleanupPending: true }), error: null }),
    stoppedForRepair: false,
  }))
  mount([refused, current, cleanup_, broken, detached, dirtyGit, manual])
  start(6)
  await screen.findByText('Review of 2 plans')
  apply()
  await screen.findByText('Update results')
  expect(within(rowOf('refused')).getByText('Preparation failed')).toBeTruthy()
  expect(within(rowOf('refused')).getByText('Could not resolve.')).toBeTruthy()
  expect(within(rowOf('current')).getByText('Up to date')).toBeTruthy()
  expect(within(rowOf('cleanup')).getByText('Updated')).toBeTruthy()
  expect(within(rowOf('cleanup')).getByText(/could not be moved to the Recycle Bin or Trash/)).toBeTruthy()
  expect(within(rowOf('broken')).getByText('Failed')).toBeTruthy()
  expect(within(rowOf('broken')).getByText('Installing the dependencies failed.')).toBeTruthy()
  expect(within(rowOf('detached')).getByText('Skipped')).toBeTruthy()
  expect(within(rowOf('detached')).getByText('Update individually with an explicit target.')).toBeTruthy()
  expect(within(rowOf('dirty-git')).getByText('The repository has local changes.')).toBeTruthy()
  expect(within(rowOf('manual')).getByText('Skipped')).toBeTruthy()
  expect(gateway.nodePrepareUpdate).not.toHaveBeenCalledWith(expect.objectContaining({ nodeId: 'detached' }))
})

it('stops_remaining_groups_for_repair', async () => {
  const nodes = manyNodes(130)
  vi.mocked(gateway.nodeUpdateAll).mockImplementation((items) => Promise.resolve(batchOf(items, true)))
  mount(nodes)
  start(130)
  await screen.findByText('Review of 128 plans')
  apply()
  await screen.findByText('Update results')
  expect(gateway.nodePrepareUpdate).toHaveBeenCalledTimes(128)
  expect(gateway.nodeUpdateAll).toHaveBeenCalledTimes(1)
  expect(screen.getByText('The run stopped early')).toBeTruthy()
  expect(screen.getByText(/needs repair/)).toBeTruthy()
  expect(screen.getAllByText('Not run')).toHaveLength(2)
  expect(onSettled).toHaveBeenCalledWith(128)
})

it('splits_more_than_128_targets_into_reviewed_groups', async () => {
  mount(manyNodes(130))
  expect(screen.getByText(/groups of at most 128/)).toBeTruthy()
  start(130)
  await screen.findByText('Review of 128 plans')
  // The second group is not prepared before the first one was reviewed and applied.
  expect(gateway.nodePrepareUpdate).toHaveBeenCalledTimes(128)
  apply()
  await screen.findByText('Review of 2 plans')
  expect(gateway.nodePrepareUpdate).toHaveBeenCalledTimes(130)
  apply()
  await screen.findByText('Update results')
  expect(vi.mocked(gateway.nodeUpdateAll).mock.calls.map(([items]) => items.length)).toEqual([128, 2])
  expect(onSettled).toHaveBeenCalledWith(130)
})

it('never_replays_successful_items_after_partial_failure', async () => {
  const a = registryInstalled({ id: 'a', name: 'a' })
  const b = registryInstalled({ id: 'b', name: 'b' })
  vi.mocked(gateway.nodeUpdateAll).mockImplementationOnce((items) => Promise.resolve({
    items: items.map((item) => item.planId === 'plan-a'
      ? { planId: item.planId, outcome: outcome(), error: null }
      : { planId: item.planId, outcome: null, error: { code: 'node_dependencies_failed', message: 'Dependencies failed.' } }),
    stoppedForRepair: false,
  }))
  mount([a, b])
  start(2)
  await screen.findByText('Review of 2 plans')
  apply()
  await screen.findByText('Update results')
  expect(gateway.nodeUpdateAll).toHaveBeenCalledTimes(1)
  expect(within(rowOf('a')).getByText('Updated')).toBeTruthy()
  expect(within(rowOf('b')).getByText('Failed')).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: /Prepare failed and unrun nodes again \(1\)/ }))
  await screen.findByText('Review of 1 plans')
  const prepared = vi.mocked(gateway.nodePrepareUpdate).mock.calls.map(([input]) => input.nodeId)
  expect(prepared).toEqual(['a', 'b', 'b'])
  // The successful item is not part of the new review and nothing is applied without it.
  expect(gateway.nodeUpdateAll).toHaveBeenCalledTimes(1)
  apply()
  await screen.findByText('Update results')
  expect(vi.mocked(gateway.nodeUpdateAll).mock.calls[1][0].map((item) => item.planId)).toEqual(['plan-b'])
  expect(within(rowOf('a')).getByText('Updated')).toBeTruthy()
  expect(within(rowOf('b')).getByText('Updated')).toBeTruthy()
})

it('cancelling_preparation_uses_the_request_key_and_discards_prepared_plans', async () => {
  const second = deferred<PlanReviewDto>()
  vi.mocked(gateway.nodePrepareUpdate)
    .mockImplementationOnce(({ nodeId }) => Promise.resolve(planFor(nodeId)))
    .mockImplementationOnce(() => second.promise)
  const a = registryInstalled({ id: 'a', name: 'a' })
  const b = registryInstalled({ id: 'b', name: 'b' })
  mount([a, b])
  start(2)
  await waitFor(() => expect(gateway.nodePrepareUpdate).toHaveBeenCalledTimes(2))
  const pendingRequest = vi.mocked(gateway.nodePrepareUpdate).mock.calls[1][0].requestId
  fireEvent.click(screen.getByRole('button', { name: 'Cancel preparation' }))
  expect(gateway.nodeCancel).toHaveBeenCalledWith(pendingRequest)
  second.resolve(planFor('b'))
  await screen.findByText('Update results')
  // The raced plan and the already prepared plan are discarded; nothing was applied.
  expect(gateway.nodeCancel).toHaveBeenCalledWith('plan-b')
  expect(gateway.nodeCancel).toHaveBeenCalledWith('plan-a')
  expect(gateway.nodeUpdateAll).not.toHaveBeenCalled()
  expect(within(rowOf('b')).getByText('Cancelled')).toBeTruthy()
  expect(within(rowOf('a')).getByText('Discarded')).toBeTruthy()
})

it('requests batch cancellation with the update-all key while applying', async () => {
  const batch = deferred<BatchOutcomeDto>()
  vi.mocked(gateway.nodeUpdateAll).mockImplementationOnce(() => batch.promise)
  mount([registryInstalled({ id: 'a', name: 'a' })])
  start(1)
  await screen.findByText('Review of 1 plans')
  apply()
  fireEvent.click(await screen.findByRole('button', { name: 'Request cancellation' }))
  expect(gateway.nodeCancel).toHaveBeenCalledWith('update-all')
  batch.resolve({
    items: [{ planId: 'plan-a', outcome: null, error: { code: 'cancelled', message: 'The operation was cancelled.' } }],
    stoppedForRepair: false,
  })
  await screen.findByText('Update results')
  expect(within(rowOf('a')).getByText('Cancelled')).toBeTruthy()
})

it('keeps the review open when ComfyUI is running and applies nothing', async () => {
  vi.mocked(gateway.nodeUpdateAll).mockRejectedValueOnce(
    new IpcClientError('comfyui_running', 'ComfyUI must be stopped before changing custom nodes.'),
  )
  mount([registryInstalled({ id: 'a', name: 'a' })])
  start(1)
  await screen.findByText('Review of 1 plans')
  apply()
  await screen.findByText('The batch was not applied')
  fireEvent.click(screen.getByRole('button', { name: 'Stop ComfyUI' }))
  await waitFor(() => expect(onStopComfy).toHaveBeenCalledTimes(1))
  await waitFor(() => expect(screen.queryByText('The batch was not applied')).toBeNull())
  apply()
  await screen.findByText('Update results')
  expect(gateway.nodeUpdateAll).toHaveBeenCalledTimes(2)
})
