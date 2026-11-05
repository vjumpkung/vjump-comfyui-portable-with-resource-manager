// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'

import { gateway, type SettingsDto } from '@/shared/ipc/gateway'
import { settingsKey } from '@/shared/lib/settings-queries'

import { AddDownloadDialog } from './add-download-dialog'

vi.mock('@/shared/ipc/gateway', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/ipc/gateway')>()),
  gateway: { settingsGet: vi.fn(), dlResolve: vi.fn() },
}))
beforeEach(() => {
  vi.stubGlobal(
    'ResizeObserver',
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  )
})
afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})
const settings: SettingsDto = {
  downloadConcurrency: 3,
  hfEngine: 'hf_cli',
  closeBehavior: 'ask',
  theme: 'system',
  language: 'en',
  registryCacheTtl: 6,
  bandwidthCapKibS: null,
  skipSecurityConfirmations: true,
}

it('late preference pre-ticks HTTP consent, sends it, and deliberate uncheck persists', async () => {
  let resolveSettings!: (value: SettingsDto) => void
  vi.mocked(gateway.settingsGet).mockReturnValue(
    new Promise((resolve) => {
      resolveSettings = resolve
    }),
  )
  vi.mocked(gateway.dlResolve).mockResolvedValue({ id: 1, files: [], selectionRequired: false })
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <AddDownloadDialog onClose={vi.fn()} onSubmit={vi.fn()} onOpenSettings={vi.fn()} />
    </QueryClientProvider>,
  )
  fireEvent.change(screen.getByLabelText('Link'), {
    target: { value: 'http://example.com/model.bin' },
  })
  const checkbox = screen.getByRole('checkbox')
  const lookup = screen.getByRole('button', { name: 'Look up' })
  expect(checkbox.getAttribute('aria-checked')).toBe('false')
  expect(lookup).toHaveProperty('disabled', true)
  await act(async () => {
    resolveSettings(settings)
    await Promise.resolve()
  })
  await waitFor(() => expect(checkbox.getAttribute('aria-checked')).toBe('true'))
  fireEvent.click(lookup)
  await waitFor(() =>
    expect(gateway.dlResolve).toHaveBeenCalledWith({
      url: 'http://example.com/model.bin',
      httpConfirmed: true,
    }),
  )
  fireEvent.click(checkbox)
  await act(async () => {
    client.setQueryData(settingsKey, { ...settings, skipSecurityConfirmations: false })
    await Promise.resolve()
  })
  await act(async () => {
    client.setQueryData(settingsKey, settings)
    await Promise.resolve()
  })
  expect(checkbox.getAttribute('aria-checked')).toBe('false')
  expect(lookup).toHaveProperty('disabled', true)
})
