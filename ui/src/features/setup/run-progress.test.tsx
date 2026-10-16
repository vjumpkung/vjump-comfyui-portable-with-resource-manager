// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from '@testing-library/react'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'

import { gateway, type RunSnapshotDto } from '@/shared/ipc/gateway'
import { run } from '@/shared/testing/environment'
import { RunProgress } from './run-progress'
import { useSetupRun } from './use-setup-run'

vi.mock('@/shared/ui/log-view', () => ({ LogView: () => <div>Live log</div> }))
vi.mock('@/shared/ipc/gateway', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/ipc/gateway')>()),
  gateway: { setupRun: vi.fn(), setupCancel: vi.fn() },
}))

function Probe({ snapshot }: { snapshot: RunSnapshotDto }) {
  const controller = useSetupRun()
  return (
    <RunProgress run={snapshot} controller={controller} refresh={vi.fn()} />
  )
}
function mount(snapshot: RunSnapshotDto) {
  return render(
    <QueryClientProvider client={new QueryClient()}>
      <Probe snapshot={snapshot} />
    </QueryClientProvider>,
  )
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
      { kind: 'retry', runId: run.id, managerConsent: 'a'.repeat(40) },
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
