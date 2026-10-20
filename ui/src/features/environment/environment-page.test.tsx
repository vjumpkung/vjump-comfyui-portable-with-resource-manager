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
import { afterEach, beforeEach, expect, it, vi } from 'vitest'

import { gateway } from '@/shared/ipc/gateway'
import { environment, setupStatus } from '@/shared/testing/environment'
import { EnvironmentPage } from './environment-page'

vi.mock('@/shared/ipc/gateway', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/ipc/gateway')>()),
  gateway: {
    envInfo: vi.fn(),
    envPackages: vi.fn(),
    envDelete: vi.fn(),
    envPipPreview: vi.fn(),
    envPipApply: vi.fn(),
  },
}))
beforeEach(() => {
  vi.resetAllMocks()
  vi.mocked(gateway.envInfo).mockResolvedValue({
    env: environment,
    profileLabel: 'Default',
    cudaVariant: 'cu130',
    torchVersion: '2.14.1',
    tritonVersion: '3.8.0',
    comfyuiReference: 'v1',
    comfyuiCommit: 'abcdef',
    accelerators: [],
    components: { manager: null, accelerators: [] },
  })
  vi.mocked(gateway.envPackages).mockResolvedValue(
    Array.from({ length: 250 }, (_, index) => ({
      name: `package-${index}`,
      version: '1.0',
    })),
  )
  vi.mocked(gateway.envDelete).mockResolvedValue(null)
})
afterEach(cleanup)

function mount(repair = false) {
  const onSetup = vi.fn()
  const view = render(
    <QueryClientProvider client={new QueryClient()}>
      <EnvironmentPage
        status={{
          ...setupStatus,
          phase: repair ? 'repair_required' : 'ready',
          activeEnv: {
            ...environment,
            state: repair ? 'repair_required' : 'verified',
          },
        }}
        onSetup={onSetup}
        onResume={vi.fn()}
      />
    </QueryClientProvider>,
  )
  return { ...view, onSetup }
}

it('shows selected env metadata and limits packages to 100 rows with search', async () => {
  mount()
  await screen.findByText('package-0')
  expect(screen.getByText('abcdef')).toBeTruthy()
  expect(screen.getAllByRole('row')).toHaveLength(101)
  fireEvent.change(screen.getByLabelText('Search packages'), {
    target: { value: 'package-249' },
  })
  expect(screen.getAllByRole('row')).toHaveLength(2)
  expect(screen.getByText('package-249')).toBeTruthy()
})

it('keeps RepairRequired honest even when environment detail cache is older', async () => {
  const { onSetup } = mount(true)
  await screen.findByText(
    'RepairRequired — selected environment is not runnable',
  )
  fireEvent.click(screen.getByRole('button', { name: 'Repair environment' }))
  expect(onSetup).toHaveBeenCalledWith('repair')
})

it('environment_page_shows_stored_accelerator_results', async () => {
  const info = await gateway.envInfo()
  vi.mocked(gateway.envInfo).mockResolvedValue({
    ...info,
    accelerators: ['reported-summary'],
    components: {
      manager: null,
      accelerators: [
        { id: 'installed-option', selected: true, status: 'installed', importable: true, message: 'Import check passed.' },
        { id: 'failed-option', selected: true, status: 'failed', importable: false, message: 'Missing dependency.' },
        { id: 'skipped-option', selected: false, status: 'skipped', importable: null, message: 'Not selected.' },
      ],
    },
  })
  const { unmount } = mount()
  await screen.findByText('installed-option')
  expect(screen.getByText('reported-summary')).toBeTruthy()
  const result = (id: string) => within(screen.getByText(id).closest('li')!)
  expect(result('installed-option').getByText('Yes')).toBeTruthy()
  expect(result('installed-option').getByText('installed')).toBeTruthy()
  expect(result('installed-option').getByText('Passed')).toBeTruthy()
  expect(result('installed-option').getByText('Import check passed.')).toBeTruthy()
  expect(result('failed-option').getByText('failed')).toBeTruthy()
  expect(result('failed-option').getByText('Failed')).toBeTruthy()
  expect(result('failed-option').getByText('Missing dependency.')).toBeTruthy()
  expect(result('skipped-option').getByText('No')).toBeTruthy()
  expect(result('skipped-option').getByText('skipped')).toBeTruthy()
  expect(result('skipped-option').getByText('Untested')).toBeTruthy()
  expect(result('skipped-option').getByText('Not selected.')).toBeTruthy()
  unmount()

  vi.mocked(gateway.envInfo).mockResolvedValue(info)
  mount()
  await screen.findByText('No recorded accelerator outcomes')
  expect(screen.getByText(/This does not establish which accelerators are installed/)).toBeTruthy()
})

it('blocks selected-env deletion and requires exact ID confirmation for inactive deletion', async () => {
  mount()
  await screen.findByText('package-0')
  const input = screen.getByLabelText('Inactive app-owned environment ID')
  fireEvent.change(input, { target: { value: environment.id } })
  expect(
    screen.getByRole('button', { name: 'Review environment deletion' }),
  ).toHaveProperty('disabled', true)
  fireEvent.change(input, { target: { value: 'env-old' } })
  fireEvent.click(
    screen.getByRole('button', { name: 'Review environment deletion' }),
  )
  expect(
    screen.getByRole('button', { name: 'Delete environment' }),
  ).toHaveProperty('disabled', true)
  fireEvent.change(screen.getByLabelText('Confirm environment ID'), {
    target: { value: 'env-old' },
  })
  fireEvent.click(screen.getByRole('button', { name: 'Delete environment' }))
  await waitFor(() =>
    expect(gateway.envDelete).toHaveBeenCalledWith(
      'env-old',
      expect.anything(),
    ),
  )
  await screen.findByText('Inactive app-owned environment deleted.')
})
