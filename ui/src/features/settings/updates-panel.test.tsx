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

import {
  gateway,
  IpcClientError,
  type UpdateProgressDto,
} from '@/shared/ipc/gateway'

import { UpdatesPanel } from './updates-panel'

vi.mock('@/shared/ipc/gateway', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/ipc/gateway')>()),
  gateway: {
    appUpdateStatus: vi.fn(),
    appUpdateCheck: vi.fn(),
    appUpdateInstall: vi.fn(),
  },
}))

const offer = {
  updateId: 'update-7',
  version: '0.2.0',
  date: '2026-10-17',
  notes: '<b>Bold</b> notes',
}

beforeEach(() => {
  vi.resetAllMocks()
  vi.mocked(gateway.appUpdateStatus).mockResolvedValue({
    configured: true,
    currentVersion: '0.1.0',
  })
})
afterEach(cleanup)

function mount(maintenanceBusy = false) {
  return render(
    <QueryClientProvider client={new QueryClient()}>
      <UpdatesPanel maintenanceBusy={maintenanceBusy} />
    </QueryClientProvider>,
  )
}

async function review(maintenanceBusy = false) {
  const view = mount(maintenanceBusy)
  fireEvent.click(await screen.findByRole('button', { name: /check for updates/i }))
  await screen.findByText('Version 0.2.0 is available')
  return view
}

it('update_install_echoes_reviewed_update_id', async () => {
  vi.mocked(gateway.appUpdateCheck).mockResolvedValue(offer)
  let report: (event: UpdateProgressDto) => void = () => undefined
  vi.mocked(gateway.appUpdateInstall).mockImplementation((_id, onEvent) => {
    report = onEvent
    return new Promise(() => undefined)
  })
  const { container } = await review()

  // Notes are plain text, never markup.
  expect(screen.getByText('<b>Bold</b> notes')).toBeTruthy()
  expect(container.querySelector('b')).toBeNull()

  fireEvent.click(screen.getByRole('button', { name: /install and restart/i }))
  await waitFor(() => expect(gateway.appUpdateInstall).toHaveBeenCalledTimes(1))
  expect(gateway.appUpdateInstall).toHaveBeenCalledWith(
    'update-7',
    expect.any(Function),
  )
  expect(gateway.appUpdateCheck).toHaveBeenCalledTimes(1)

  report({ kind: 'downloading', downloadedBytes: 5 * 1024 * 1024, totalBytes: 10 * 1024 * 1024 })
  expect(await screen.findByText('Downloading 5.0 of 10.0 MiB')).toBeTruthy()
  report({ kind: 'installing' })
  expect(await screen.findByText('Installing the update')).toBeTruthy()
})

it('shows_no_updater_actions_when_the_build_is_not_configured', async () => {
  vi.mocked(gateway.appUpdateStatus).mockResolvedValue({
    configured: false,
    currentVersion: '0.1.0',
  })
  mount()
  expect(await screen.findByText(/cannot update itself/i)).toBeTruthy()
  expect(screen.queryByRole('button', { name: /check for updates/i })).toBeNull()
  expect(gateway.appUpdateCheck).not.toHaveBeenCalled()
})

it('reports_an_up_to_date_app', async () => {
  vi.mocked(gateway.appUpdateCheck).mockResolvedValue(null)
  mount()
  fireEvent.click(await screen.findByRole('button', { name: /check for updates/i }))
  expect(await screen.findByText('You are up to date.')).toBeTruthy()
})

it('a_failed_install_shows_its_message_and_requires_a_new_check', async () => {
  vi.mocked(gateway.appUpdateCheck).mockResolvedValue(offer)
  vi.mocked(gateway.appUpdateInstall).mockRejectedValue(
    new IpcClientError(
      'update_signature_invalid',
      'The downloaded update failed signature verification and was not installed.',
    ),
  )
  await review()
  fireEvent.click(screen.getByRole('button', { name: /install and restart/i }))

  expect(await screen.findByRole('alert')).toBeTruthy()
  expect(
    screen.getByText(/failed signature verification and was not installed/i),
  ).toBeTruthy()
  await waitFor(() =>
    expect(screen.queryByRole('button', { name: /install and restart/i })).toBeNull(),
  )
  expect(screen.getByRole('button', { name: /check for updates/i })).toBeTruthy()
})

it('install_is_blocked_while_a_setup_run_is_in_progress', async () => {
  vi.mocked(gateway.appUpdateCheck).mockResolvedValue(offer)
  await review(true)
  const install = screen.getByRole<HTMLButtonElement>('button', {
    name: /install and restart/i,
  })
  expect(install.disabled).toBe(true)
  fireEvent.click(install)
  expect(gateway.appUpdateInstall).not.toHaveBeenCalled()
})
