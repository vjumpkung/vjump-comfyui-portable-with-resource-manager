// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'

import { getLanguage, setLanguage } from '@/shared/i18n'
import { gateway, type SettingsDto } from '@/shared/ipc/gateway'
import { ThemeProvider } from '@/shared/lib/theme'
import { setupStatus } from '@/shared/testing/environment'
import { TooltipProvider } from '@/shared/ui/tooltip'

import { SetupWizardPreferences } from './setup-wizard-preferences'
import { ThemeToggle } from './shell/theme-toggle'

vi.mock('@/shared/ui/log-view', () => ({ LogView: () => null }))
vi.mock('@/shared/ipc/gateway', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/ipc/gateway')>()),
  gateway: {
    settingsGet: vi.fn(),
    settingsSet: vi.fn(),
    setupSelectRoot: vi.fn(),
    setupListCuratedNodes: vi.fn(),
    setupCancel: vi.fn(),
  },
}))

const settings: SettingsDto = {
  downloadConcurrency: 3,
  hfEngine: 'native',
  closeBehavior: 'stop_comfy',
  theme: 'light',
  language: 'en',
  registryCacheTtl: 6,
  bandwidthCapKibS: 256,
}

function deferred<T>() {
  let resolve: (value: T) => void = () => undefined
  let reject: (error: Error) => void = () => undefined
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, resolve, reject }
}

function mount() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })
  return { client, ...render(
    <QueryClientProvider client={client}>
      <ThemeProvider>
        <TooltipProvider>
          <ThemeToggle />
          <SetupWizardPreferences status={setupStatus} mode="first_run"
            onClose={vi.fn()} refresh={vi.fn()} renderCuratedReview={() => null} />
        </TooltipProvider>
      </ThemeProvider>
    </QueryClientProvider>,
  ) }
}

beforeEach(() => {
  vi.resetAllMocks()
  window.localStorage.clear()
  vi.mocked(gateway.settingsGet).mockResolvedValue(settings)
  vi.mocked(gateway.settingsSet).mockImplementation((input) => Promise.resolve(input))
  vi.mocked(gateway.setupListCuratedNodes).mockResolvedValue([])
})

afterEach(() => {
  cleanup()
  setLanguage('en')
})

it('renders_the_endonym_selector_at_the_top_of_stage_zero', async () => {
  mount()
  const language = await screen.findByRole('combobox', { name: 'Language' })
  await waitFor(() => expect(language).toHaveProperty('disabled', false))
  const root = screen.getByLabelText('Install root')
  const body = root.closest('[data-slot="card-content"]')!
  expect(body.contains(language)).toBe(true)
  expect(within(body as HTMLElement).getAllByRole('group')[0].contains(language)).toBe(true)
  expect(language.compareDocumentPosition(root) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
  expect(screen.getByRole('option', { name: 'English' })).toHaveProperty('value', 'en')
  expect(screen.getByRole('option', { name: 'ไทย' })).toHaveProperty('value', 'th')
})

it('switches_to_thai_immediately_saves_full_settings_and_blocks_root_and_theme_until_saved', async () => {
  const save = deferred<SettingsDto>()
  vi.mocked(gateway.settingsSet).mockReturnValue(save.promise)
  const { client } = mount()
  const language = await screen.findByLabelText('Language')
  await waitFor(() => expect(language).toHaveProperty('disabled', false))
  fireEvent.change(language, { target: { value: 'th' } })
  expect(getLanguage()).toBe('th')
  expect(screen.getByLabelText('ภาษา')).toHaveProperty('value', 'th')
  expect(screen.getByLabelText('โฟลเดอร์ติดตั้ง')).toBeTruthy()
  expect(screen.getByText('1. โฟลเดอร์ติดตั้ง')).toBeTruthy()
  await waitFor(() => expect(gateway.settingsSet).toHaveBeenCalledWith({ ...settings, language: 'th' }))
  await waitFor(() => expect(screen.getByRole('button', { name: 'ถัดไป' })).toHaveProperty('disabled', true))
  expect(language).toHaveProperty('disabled', true)
  expect(screen.getByRole('button', { name: 'เปลี่ยนเป็นธีมมืด' })).toHaveProperty('disabled', true)
  fireEvent.click(screen.getByRole('button', { name: 'ถัดไป' }))
  fireEvent.change(screen.getByLabelText('โฟลเดอร์ติดตั้ง'), { target: { value: 'E:/Comfy' } })
  expect(screen.getByRole('button', { name: 'บันทึกโฟลเดอร์ติดตั้ง' })).toHaveProperty('disabled', true)
  fireEvent.click(screen.getByRole('button', { name: 'บันทึกโฟลเดอร์ติดตั้ง' }))
  expect(gateway.setupSelectRoot).not.toHaveBeenCalled()
  await act(async () => {
    save.resolve({ ...settings, language: 'th' })
    await save.promise
  })
  await waitFor(() => expect(language).toHaveProperty('disabled', false))
  expect(screen.getByRole('button', { name: 'บันทึกโฟลเดอร์ติดตั้ง' })).toHaveProperty('disabled', false)
  expect(screen.getByRole('button', { name: 'เปลี่ยนเป็นธีมมืด' })).toHaveProperty('disabled', false)
  expect(client.getQueryData(['settings'])).toEqual({ ...settings, language: 'th' })
  expect(getLanguage()).toBe('th')
})

it('failed_language_save_restores_the_previous_language', async () => {
  const save = deferred<SettingsDto>()
  vi.mocked(gateway.settingsSet).mockReturnValue(save.promise)
  mount()
  const language = await screen.findByLabelText('Language')
  await waitFor(() => expect(language).toHaveProperty('disabled', false))
  fireEvent.change(language, { target: { value: 'th' } })
  expect(screen.getByLabelText('ภาษา')).toBeTruthy()
  await waitFor(() => expect(gateway.settingsSet).toHaveBeenCalledTimes(1))
  await act(async () => {
    save.reject(new Error('Save failed'))
    await save.promise.catch(() => undefined)
  })
  await waitFor(() => expect(getLanguage()).toBe('en'))
  expect(screen.getByLabelText('Language')).toHaveProperty('value', 'en')
  expect(screen.getByText('1. Install root')).toBeTruthy()
  expect(screen.getByRole('button', { name: 'Continue' })).toHaveProperty('disabled', false)
})

it('successful_language_save_reconciles_with_the_returned_language', async () => {
  vi.mocked(gateway.settingsSet).mockResolvedValue(settings)
  mount()
  const language = await screen.findByLabelText('Language')
  await waitFor(() => expect(language).toHaveProperty('disabled', false))
  fireEvent.change(language, { target: { value: 'th' } })
  expect(getLanguage()).toBe('th')
  await waitFor(() => expect(gateway.settingsSet).toHaveBeenCalledTimes(1))
  await waitFor(() => expect(getLanguage()).toBe('en'))
  expect(screen.getByLabelText('Language')).toHaveProperty('value', 'en')
})

it('disables_language_while_root_transfer_is_pending_and_after_it_succeeds', async () => {
  const transfer = deferred<{ restartRequired: boolean }>()
  vi.mocked(gateway.setupSelectRoot).mockReturnValue(transfer.promise)
  mount()
  const language = await screen.findByLabelText('Language')
  await waitFor(() => expect(language).toHaveProperty('disabled', false))
  fireEvent.change(screen.getByLabelText('Install root'), { target: { value: 'E:/Comfy' } })
  fireEvent.click(screen.getByRole('button', { name: 'Save install root' }))
  await waitFor(() => expect(gateway.setupSelectRoot).toHaveBeenCalledTimes(1))
  expect(language).toHaveProperty('disabled', true)
  await act(async () => {
    transfer.resolve({ restartRequired: true })
    await transfer.promise
  })
  await screen.findByText('Restart required')
  expect(language).toHaveProperty('disabled', true)
  expect(gateway.settingsSet).not.toHaveBeenCalled()
})

it('a_pending_theme_save_disables_the_wizard_preference_controls_and_continue', async () => {
  const save = deferred<SettingsDto>()
  vi.mocked(gateway.settingsSet).mockReturnValue(save.promise)
  mount()
  const language = await screen.findByLabelText('Language')
  const theme = screen.getByRole('button', { name: 'Switch to dark theme' })
  await waitFor(() => expect(theme).toHaveProperty('disabled', false))
  fireEvent.click(theme)
  await waitFor(() => expect(gateway.settingsSet).toHaveBeenCalledWith({ ...settings, theme: 'dark' }))
  expect(language).toHaveProperty('disabled', true)
  expect(theme).toHaveProperty('disabled', true)
  expect(screen.getByRole('button', { name: 'Continue' })).toHaveProperty('disabled', true)
  await act(async () => {
    save.resolve({ ...settings, theme: 'dark' })
    await save.promise
  })
  await waitFor(() => expect(language).toHaveProperty('disabled', false))
})
