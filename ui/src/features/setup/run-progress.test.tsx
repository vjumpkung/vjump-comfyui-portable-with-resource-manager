// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react'
import { useState } from 'react'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'

import { gateway, type PlanReviewDto, type RunSnapshotDto } from '@/shared/ipc/gateway'
import { run } from '@/shared/testing/environment'
import { RunProgress } from './run-progress'
import { useSetupRun } from './use-setup-run'
import type { CuratedReviewProps } from './model'

vi.mock('@/shared/ui/log-view', () => ({ LogView: () => <div>Live log</div> }))
vi.mock('@/shared/ipc/gateway', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/ipc/gateway')>()),
  gateway: { setupRun: vi.fn(), setupCancel: vi.fn() },
}))

function TestReview({ plans, busy, onCommit, onClose, notice }: CuratedReviewProps) {
  const [execution, setExecution] = useState(false)
  const [warnings, setWarnings] = useState(false)
  return <div role="dialog" aria-label="Test curated review">
    {plans.map((plan) => <p key={plan.id}>{plan.name}: {plan.warnings.join(', ')}</p>)}
    <label><input type="checkbox" checked={execution} onChange={(event) => setExecution(event.target.checked)} />Execute reviewed sources</label>
    <label><input type="checkbox" checked={warnings} onChange={(event) => setWarnings(event.target.checked)} />Acknowledge warnings</label>
    {notice}
    <button disabled={busy} onClick={onClose}>Dismiss review</button>
    <button disabled={busy || !execution || !warnings} onClick={() => onCommit(plans.map((plan) => ({
      planId: plan.id, binding: plan.binding, executeNodeCode: execution,
      acknowledgeWarnings: warnings, runInstallPy: false, rememberSource: false,
      rememberInstallPy: false, confirmedChanges: [],
    })))}>Approve batch</button>
  </div>
}

function Probe({ snapshot }: { snapshot: RunSnapshotDto }) {
  const controller = useSetupRun()
  return (
    <RunProgress run={snapshot} controller={controller} refresh={vi.fn()}
      renderCuratedReview={(props) => <TestReview {...props} />} />
  )
}
function mount(snapshot: RunSnapshotDto) {
  const client = new QueryClient()
  const ui = (next: RunSnapshotDto) => <QueryClientProvider client={client}>
    <Probe snapshot={next} />
  </QueryClientProvider>
  const view = render(ui(snapshot))
  return { ...view, update: (next: RunSnapshotDto) => view.rerender(ui(next)) }
}
beforeEach(() => {
  vi.resetAllMocks()
  vi.mocked(gateway.setupRun).mockResolvedValue({ runId: run.id })
  vi.mocked(gateway.setupCancel).mockResolvedValue(null)
})
afterEach(cleanup)

it('reconstructs failure details and requires exact-source consent before retry', async () => {
  mount(run)
  expect(screen.getByText('sanitized tail')).toBeTruthy()
  expect(screen.getByRole('button', { name: 'Retry same run' })).toHaveProperty(
    'disabled',
    true,
  )
  fireEvent.click(screen.getByRole('checkbox'))
  fireEvent.click(screen.getByRole('button', { name: 'Retry same run' }))
  await waitFor(() =>
    expect(gateway.setupRun).toHaveBeenCalledWith(
      {
        kind: 'retry',
        runId: run.id,
        managerConsent: 'a'.repeat(40),
        curatedDecisions: [],
      },
      expect.any(Function),
    ),
  )
})

it('skips an optional failed step using the same run', async () => {
  mount(run)
  fireEvent.click(screen.getByRole('button', { name: 'Skip optional step' }))
  await waitFor(() =>
    expect(gateway.setupRun).toHaveBeenCalledWith(
      { kind: 'skipStep', runId: run.id, step: 'manager' },
      expect.any(Function),
    ),
  )
})

it('does not expose Skip for required failures', () => {
  mount({
    ...run,
    steps: run.steps.map((step) => ({ ...step, policy: 'required' })),
  })
  expect(
    screen.queryByRole('button', { name: 'Skip optional step' }),
  ).toBeNull()
})

it('damaged_optional_step_cannot_be_skipped', () => {
  mount({ ...run, failure: { ...run.failure!, canSkip: false } })
  expect(screen.queryByRole('button', { name: 'Skip optional step' })).toBeNull()
})

const plan: PlanReviewDto = {
  id: 'plan-one', binding: 'binding-one', kind: 'install', name: 'First curated node',
  source: { kind: 'git', repository: 'https://example.com/one.git', commit: 'a'.repeat(40), branch: null },
  priorSource: null, requirements: ['fixture==1'], artifactDigest: null,
  treeDigest: 'tree', hookDigest: null, protectedChanges: [],
  warnings: ['Detached at the pinned commit.'], trustedSource: false,
  trustedInstallPy: false, hasRequirements: true, hasInstallPy: false,
}

it('live_curated_reviews_guard_every_retryable_snapshot', () => {
  mount({ ...run, status: 'interrupted', failure: null, curatedReviews: [plan] })
  expect(screen.getByRole('dialog', { name: 'Test curated review' })).toBeTruthy()
  expect(screen.getByRole('button', { name: 'Retry same run' })).toHaveProperty('disabled', true)
  expect(gateway.setupRun).not.toHaveBeenCalled()
})

it('curated_retry_resends_decisions_and_rereviews_after_restart', async () => {
  const plans = [plan, { ...plan, id: 'plan-two', binding: 'binding-two', name: 'Second curated node' }]
  const paused: RunSnapshotDto = {
    ...run,
    failure: { ...run.failure!, step: 'curated_nodes', consent: null, canSkip: false },
    curatedReviews: plans,
    curated: [
      { id: 'pending-node', status: 'pending', message: null },
      { id: 'prepared-node', status: 'prepared', message: 'Review required' },
      { id: 'failed-node', status: 'failed', message: 'Dependencies damaged' },
      { id: 'installed-node', status: 'installed', message: 'install.py was not run; reinstall with approval if needed.' },
      { id: 'skipped-node', status: 'skipped', message: null },
    ],
  }
  const view = mount(paused)
  expect(screen.getByText('install.py was not run; reinstall with approval if needed.')).toBeTruthy()
  for (const status of ['pending', 'prepared', 'failed', 'installed', 'skipped'])
    expect(within(screen.getByRole('region', { name: 'Curated node progress' })).getByText(status)).toBeTruthy()
  expect(screen.getByRole('button', { name: 'Retry same run' })).toHaveProperty('disabled', true)
  fireEvent.click(screen.getByRole('button', { name: 'Dismiss review' }))
  expect(gateway.setupRun).not.toHaveBeenCalled()
  expect(gateway.setupCancel).not.toHaveBeenCalled()
  fireEvent.click(screen.getByRole('button', { name: 'Review curated plans' }))
  const approve = () => {
    fireEvent.click(screen.getByRole('checkbox', { name: 'Execute reviewed sources' }))
    expect(screen.getByRole('button', { name: 'Approve batch' })).toHaveProperty('disabled', true)
    fireEvent.click(screen.getByRole('checkbox', { name: 'Acknowledge warnings' }))
    fireEvent.click(screen.getByRole('button', { name: 'Approve batch' }))
  }
  approve()
  const decisions = plans.map((entry) => ({
    planId: entry.id, binding: entry.binding, executeNodeCode: true,
    acknowledgeWarnings: true, runInstallPy: false, rememberSource: false,
    rememberInstallPy: false, confirmedChanges: [],
  }))
  const exactRetry = { kind: 'retry', runId: run.id, managerConsent: null, curatedDecisions: decisions }
  await waitFor(() => expect(gateway.setupRun).toHaveBeenCalledWith(exactRetry, expect.any(Function)))
  await waitFor(() => expect(screen.getByRole('button', { name: 'Dismiss review' })).toHaveProperty('disabled', false))
  fireEvent.click(screen.getByRole('button', { name: 'Dismiss review' }))
  fireEvent.click(screen.getByRole('button', { name: 'Retry same run' }))
  await waitFor(() => expect(gateway.setupRun).toHaveBeenCalledTimes(2))
  expect(vi.mocked(gateway.setupRun).mock.calls[1][0]).toEqual(exactRetry)

  const changed = { ...paused, curatedReviews: [{ ...plan, binding: 'changed' }, plans[1]] }
  view.update(changed)
  expect(screen.getByRole('checkbox', { name: 'Execute reviewed sources' })).toHaveProperty('checked', false)
  expect(screen.getByRole('button', { name: 'Retry same run' })).toHaveProperty('disabled', true)
  view.update(paused)
  expect(screen.getByRole('checkbox', { name: 'Execute reviewed sources' })).toHaveProperty('checked', false)

  view.update({ ...paused, curatedReviews: [] })
  expect(screen.queryByRole('dialog')).toBeNull()
  expect(screen.getByText('Fresh curated reviews are required')).toBeTruthy()
  await waitFor(() => expect(screen.getByRole('button', { name: 'Retry same run' })).toHaveProperty('disabled', false))
  fireEvent.click(screen.getByRole('button', { name: 'Retry same run' }))
  await waitFor(() => expect(gateway.setupRun).toHaveBeenCalledTimes(3))
  expect(vi.mocked(gateway.setupRun).mock.calls[2][0]).toEqual({ ...exactRetry, curatedDecisions: [] })
  view.update(paused)
  expect(screen.getByRole('checkbox', { name: 'Acknowledge warnings' })).toHaveProperty('checked', false)
  expect(screen.getByRole('button', { name: 'Retry same run' })).toHaveProperty('disabled', true)
  vi.mocked(gateway.setupRun).mockRejectedValueOnce(new Error('Retry refused'))
  approve()
  await waitFor(() => expect(within(screen.getByRole('dialog')).getByText('Unable to complete the request')).toBeTruthy())
  view.unmount()
  mount(paused)
  expect(screen.getByRole('checkbox', { name: 'Execute reviewed sources' })).toHaveProperty('checked', false)
})

it('waits for backend cleanup after requesting cancellation', async () => {
  const view = mount({ ...run, status: 'running', failure: null })
  fireEvent.click(screen.getByRole('button', { name: 'Abort setup' }))
  await waitFor(() =>
    expect(gateway.setupCancel).toHaveBeenCalledWith(run.id, expect.anything()),
  )
  await screen.findByRole('button', { name: 'Cancelling and cleaning up…' })
  expect(screen.queryByText(/Setup was cancelled/)).toBeNull()
  view.unmount()
})
