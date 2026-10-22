// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'

import { getLanguage, setLanguage } from '@/shared/i18n'
import { gateway, type SettingsDto } from '@/shared/ipc/gateway'
import { SettingsForm } from './settings-form'

vi.mock('@/shared/ipc/gateway', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/ipc/gateway')>()),
  gateway: { settingsGet: vi.fn(), settingsSet: vi.fn() },
}))

const settings: SettingsDto = {
  downloadConcurrency: 2,
  hfEngine: 'hf_cli',
  closeBehavior: 'ask',
  theme: 'system',
  language: 'en',
  registryCacheTtl: 24,
  bandwidthCapKibS: 128,
}

function mount() {
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { mutations: { retry: false } } })}>
      <SettingsForm />
    </QueryClientProvider>,
  )
}

beforeEach(() => {
  vi.resetAllMocks()
  vi.mocked(gateway.settingsGet).mockResolvedValue(settings)
  vi.mocked(gateway.settingsSet).mockImplementation((input) => Promise.resolve(input))
})

afterEach(() => {
  cleanup()
  setLanguage('en')
})

it('bandwidth_empty_means_unlimited_and_zero_is_invalid', async () => {
  mount()
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

it('language_is_a_draft_until_save_succeeds_and_is_included_in_the_payload', async () => {
  let save: (saved: SettingsDto) => void = () => undefined
  const pending = new Promise<SettingsDto>((resolve) => { save = resolve })
  vi.mocked(gateway.settingsSet).mockReturnValue(pending)
  mount()
  const language = await screen.findByLabelText('Language')
  expect(screen.getByRole('option', { name: 'English' })).toBeTruthy()
  expect(screen.getByRole('option', { name: 'ไทย' })).toBeTruthy()
  fireEvent.change(language, { target: { value: 'th' } })
  expect(getLanguage()).toBe('en')
  expect(gateway.settingsSet).not.toHaveBeenCalled()
  fireEvent.click(screen.getByRole('button', { name: 'Save settings' }))
  await waitFor(() => expect(gateway.settingsSet).toHaveBeenCalledWith({ ...settings, language: 'th' }))
  expect(getLanguage()).toBe('en')
  await act(async () => {
    save({ ...settings, language: 'th' })
    await pending
  })
  await screen.findByLabelText('ภาษา')
  expect(getLanguage()).toBe('th')
})

it('successful_save_applies_the_returned_language', async () => {
  vi.mocked(gateway.settingsSet).mockResolvedValue({ ...settings, language: 'th' })
  mount()
  await screen.findByLabelText('Language')
  fireEvent.click(screen.getByRole('button', { name: 'Save settings' }))
  await screen.findByLabelText('ภาษา')
  expect(getLanguage()).toBe('th')
  expect(gateway.settingsSet).toHaveBeenCalledWith(settings)
})

it('failed_save_keeps_the_active_language_and_the_draft', async () => {
  vi.mocked(gateway.settingsSet).mockRejectedValue(new Error('Save failed'))
  mount()
  fireEvent.change(await screen.findByLabelText('Language'), { target: { value: 'th' } })
  fireEvent.click(screen.getByRole('button', { name: 'Save settings' }))
  await waitFor(() => expect(gateway.settingsSet).toHaveBeenCalledTimes(1))
  await waitFor(() => expect(screen.getByRole('button', { name: 'Save settings' })).toHaveProperty('disabled', false))
  expect(getLanguage()).toBe('en')
  expect(screen.getByLabelText('Language')).toHaveProperty('value', 'th')
})
