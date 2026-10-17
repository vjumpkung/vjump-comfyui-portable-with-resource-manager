// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, expect, it, vi } from 'vitest'

import { gateway, type SettingsDto } from '@/shared/ipc/gateway'
import { SettingsForm } from './settings-form'

vi.mock('@/shared/ipc/gateway', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/ipc/gateway')>()),
  gateway: { settingsGet: vi.fn(), settingsSet: vi.fn() },
}))

afterEach(cleanup)

it('bandwidth_empty_means_unlimited_and_zero_is_invalid', async () => {
  const settings: SettingsDto = {
    downloadConcurrency: 2,
    hfEngine: 'hf_cli',
    closeBehavior: 'ask',
    theme: 'system',
    registryCacheTtl: 24,
    bandwidthCapKibS: 128,
  }
  vi.mocked(gateway.settingsGet).mockResolvedValue(settings)
  vi.mocked(gateway.settingsSet).mockImplementation((input) => Promise.resolve(input))
  render(
    <QueryClientProvider client={new QueryClient()}>
      <SettingsForm />
    </QueryClientProvider>,
  )
  const cap = await screen.findByLabelText('Bandwidth cap (KiB/s)')
  expect(cap).toHaveProperty('disabled', false)
  for (const invalid of ['0', '-1', '1.5', '4294967296']) {
    fireEvent.change(cap, { target: { value: invalid } })
    fireEvent.click(screen.getByRole('button', { name: 'Save settings' }))
    expect(screen.getByText('Enter KiB/s as 1 or more, or leave it empty.')).toBeTruthy()
    expect(gateway.settingsSet).not.toHaveBeenCalled()
  }
  fireEvent.change(cap, { target: { value: '' } })
  fireEvent.click(screen.getByRole('button', { name: 'Save settings' }))
  await waitFor(() => expect(gateway.settingsSet).toHaveBeenCalledWith({
    ...settings, bandwidthCapKibS: null,
  }))
  const updated = await screen.findByLabelText('Bandwidth cap (KiB/s)')
  fireEvent.change(updated, { target: { value: '64' } })
  await waitFor(() => expect(screen.getByRole('button', { name: 'Save settings' })).toHaveProperty('disabled', false))
  fireEvent.click(screen.getByRole('button', { name: 'Save settings' }))
  await waitFor(() => expect(gateway.settingsSet).toHaveBeenLastCalledWith({
    ...settings, bandwidthCapKibS: 64,
  }))
})
