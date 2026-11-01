// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'

import { gateway, type SettingsDto } from '@/shared/ipc/gateway'
import { settingsKey } from '@/shared/lib/settings-queries'

import { LaunchOptionsCard } from './launch-options'

vi.mock('@/shared/ipc/gateway', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/ipc/gateway')>()),
  gateway: { settingsGet: vi.fn(), comfyGetLaunchOptions: vi.fn(), comfySetLaunchOptions: vi.fn() },
}))
beforeEach(() => {
  vi.stubGlobal('ResizeObserver', class {
    observe() {}
    unobserve() {}
    disconnect() {}
  })
})
afterEach(() => { cleanup(); vi.unstubAllGlobals() })
const settings: SettingsDto = {
  downloadConcurrency: 3, hfEngine: 'hf_cli', closeBehavior: 'ask', theme: 'system',
  language: 'en', registryCacheTtl: 6, bandwidthCapKibS: null, skipSecurityConfirmations: true,
}

it('late preference pre-ticks LAN risk acknowledgement and manual uncheck persists', async () => {
  let resolveSettings!: (value: SettingsDto) => void
  vi.mocked(gateway.settingsGet).mockReturnValue(new Promise((resolve) => { resolveSettings = resolve }))
  const options = { port: 8188, vram: 'auto' as const, attention: 'default' as const,
    preview: 'auto' as const, enableManager: false, listenLan: false, extraArgs: [] }
  vi.mocked(gateway.comfyGetLaunchOptions).mockResolvedValue(options)
  vi.mocked(gateway.comfySetLaunchOptions).mockImplementation((value) => Promise.resolve(value))
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(<QueryClientProvider client={client}><LaunchOptionsCard active={null} /></QueryClientProvider>)
  fireEvent.click(await screen.findByLabelText(/Listen on the local network/))
  const checkbox = screen.getByLabelText('I understand the risk')
  const save = screen.getByRole('button', { name: 'Save launch options' })
  expect(checkbox.getAttribute('aria-checked')).toBe('false')
  expect(save).toHaveProperty('disabled', true)
  await act(async () => { resolveSettings(settings); await Promise.resolve() })
  await waitFor(() => expect(checkbox.getAttribute('aria-checked')).toBe('true'))
  expect(save).toHaveProperty('disabled', false)
  fireEvent.click(checkbox)
  await act(async () => { client.setQueryData(settingsKey, { ...settings, skipSecurityConfirmations: false }); await Promise.resolve() })
  await act(async () => { client.setQueryData(settingsKey, settings); await Promise.resolve() })
  expect(checkbox.getAttribute('aria-checked')).toBe('false')
  expect(save).toHaveProperty('disabled', true)
  expect(gateway.comfySetLaunchOptions).not.toHaveBeenCalled()
  fireEvent.click(checkbox)
  fireEvent.click(save)
  await waitFor(() => expect(gateway.comfySetLaunchOptions).toHaveBeenCalledWith({ ...options, listenLan: true }))
})
