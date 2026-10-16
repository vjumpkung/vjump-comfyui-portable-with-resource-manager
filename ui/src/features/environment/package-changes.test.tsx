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

import { gateway, type PipPlanDto } from '@/shared/ipc/gateway'
import { PackageChanges } from './package-changes'

vi.mock('@/shared/ipc/gateway', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/ipc/gateway')>()),
  gateway: { envPipPreview: vi.fn(), envPipApply: vi.fn() },
}))
const plan: PipPlanDto = {
  planId: 'plan-1',
  envId: 'env-1',
  operation: { kind: 'install', specs: ['torch==2.14.2'] },
  resolution: 'unconstrained',
  installs: [{ name: 'torch', version: '2.14.2' }],
  removals: [],
  protectedChanges: [{ name: 'torch', from: '2.14.1', to: '2.14.2' }],
  requiresConfirmation: true,
}
function mount() {
  return render(
    <QueryClientProvider client={new QueryClient()}>
      <PackageChanges envId="env-1" maintenanceBusy={false} />
    </QueryClientProvider>,
  )
}
async function preview() {
  fireEvent.change(screen.getByLabelText('Package requirement'), {
    target: { value: 'torch==2.14.2' },
  })
  fireEvent.click(
    screen.getByRole('button', { name: 'Preview package change' }),
  )
  await screen.findByRole('dialog')
}
beforeEach(() => {
  vi.resetAllMocks()
  vi.mocked(gateway.envPipPreview).mockResolvedValue(plan)
})
afterEach(cleanup)

it('requires exact protected-change approval and sends only that plan’s changes', async () => {
  vi.mocked(gateway.envPipApply).mockResolvedValue({
    envState: 'verified',
    protectedChanges: plan.protectedChanges,
  })
  mount()
  await preview()
  expect(
    screen.getByRole('button', { name: 'Apply this plan' }),
  ).toHaveProperty('disabled', true)
  expect(screen.getByText('torch: 2.14.1 → 2.14.2')).toBeTruthy()
  fireEvent.click(screen.getByRole('checkbox'))
  fireEvent.click(screen.getByRole('button', { name: 'Apply this plan' }))
  await waitFor(() =>
    expect(gateway.envPipApply).toHaveBeenCalledWith({
      planId: 'plan-1',
      confirmedChanges: plan.protectedChanges,
    }),
  )
  await screen.findByText(
    'Packages and protected pins were rechecked successfully.',
  )
})

it('requires a fresh preview after an apply failure and hides raw transport errors', async () => {
  vi.mocked(gateway.envPipApply).mockRejectedValue(
    new Error('SECRET raw subprocess detail'),
  )
  mount()
  await preview()
  fireEvent.click(screen.getByRole('checkbox'))
  fireEvent.click(screen.getByRole('button', { name: 'Apply this plan' }))
  await screen.findByText(
    'Request a fresh preview before another apply attempt.',
  )
  expect(
    screen.getByRole('button', { name: 'Apply this plan' }),
  ).toHaveProperty('disabled', true)
  expect(screen.queryByText(/SECRET/)).toBeNull()
})

it('reports RepairRequired rather than success after a core-package removal', async () => {
  vi.mocked(gateway.envPipApply).mockResolvedValue({
    envState: 'repair_required',
    protectedChanges: plan.protectedChanges,
  })
  mount()
  await preview()
  fireEvent.click(screen.getByRole('checkbox'))
  fireEvent.click(screen.getByRole('button', { name: 'Apply this plan' }))
  await screen.findByText(
    'The environment remains selected but requires repair. It is not ready to launch.',
  )
  expect(
    screen.queryByText(
      'Packages and protected pins were rechecked successfully.',
    ),
  ).toBeNull()
})

it('invalidates consent when a new preview is requested', async () => {
  mount()
  await preview()
  fireEvent.click(screen.getByRole('checkbox'))
  fireEvent.click(screen.getByRole('button', { name: 'Close preview' }))
  vi.mocked(gateway.envPipPreview).mockResolvedValue({
    ...plan,
    planId: 'plan-2',
  })
  fireEvent.click(
    screen.getByRole('button', { name: 'Preview package change' }),
  )
  await screen.findByRole('dialog')
  expect(
    screen.getByRole('button', { name: 'Apply this plan' }),
  ).toHaveProperty('disabled', true)
})
