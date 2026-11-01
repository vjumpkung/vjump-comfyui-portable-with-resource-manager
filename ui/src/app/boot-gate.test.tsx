// @vitest-environment jsdom
import { act, cleanup, render, screen } from '@testing-library/react'
import { StrictMode } from 'react'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'

import { setLanguage } from '@/shared/i18n'
import { gateway } from '@/shared/ipc/gateway'

import { BootGate } from './boot-gate'

vi.mock('@/shared/ipc/gateway', () => ({ gateway: { appReady: vi.fn() } }))

beforeEach(() => {
  vi.useFakeTimers()
  vi.resetAllMocks()
  setLanguage('en')
})

afterEach(() => {
  cleanup()
  vi.useRealTimers()
  setLanguage('en')
})

it('keeps_children_unmounted_until_ready_then_stops_polling', async () => {
  const child = vi.fn(() => <p>Workspace</p>)
  const Child = child
  vi.mocked(gateway.appReady).mockResolvedValueOnce(false).mockResolvedValue(true)
  render(<BootGate><Child /></BootGate>)
  expect(screen.getByRole('status').getAttribute('aria-busy')).toBe('true')
  expect(screen.getByText('Starting…')).toBeTruthy()
  expect(child).not.toHaveBeenCalled()
  await act(() => vi.advanceTimersByTimeAsync(150))
  expect(screen.getByText('Workspace')).toBeTruthy()
  expect(screen.queryByRole('status')).toBeNull()
  await act(() => vi.advanceTimersByTimeAsync(1000))
  expect(gateway.appReady).toHaveBeenCalledTimes(2)
})

it('allows_only_one_pending_request_including_strict_mode_effect_restarts', async () => {
  let release: (ready: boolean) => void = () => undefined
  vi.mocked(gateway.appReady).mockImplementationOnce(() => new Promise((resolve) => { release = resolve }))
    .mockResolvedValue(true)
  render(<StrictMode><BootGate><p>Workspace</p></BootGate></StrictMode>)
  await act(() => vi.advanceTimersByTimeAsync(1500))
  expect(gateway.appReady).toHaveBeenCalledTimes(1)
  await act(async () => { release(false); await Promise.resolve() })
  await act(() => vi.advanceTimersByTimeAsync(149))
  expect(gateway.appReady).toHaveBeenCalledTimes(1)
  await act(() => vi.advanceTimersByTimeAsync(1))
  expect(screen.getByText('Workspace')).toBeTruthy()
  expect(gateway.appReady).toHaveBeenCalledTimes(2)
})

it('keeps_polling_after_an_error_and_translates_the_loading_screen', async () => {
  setLanguage('th')
  vi.mocked(gateway.appReady).mockRejectedValueOnce(new Error('not connected')).mockResolvedValue(true)
  render(<BootGate><p>Workspace</p></BootGate>)
  expect(screen.getByText('กำลังเริ่มต้น…')).toBeTruthy()
  await act(() => vi.advanceTimersByTimeAsync(150))
  expect(screen.getByText('Workspace')).toBeTruthy()
})

it('does_not_poll_again_when_a_pending_request_finishes_after_unmount', async () => {
  let release: (ready: boolean) => void = () => undefined
  vi.mocked(gateway.appReady).mockImplementation(() => new Promise((resolve) => { release = resolve }))
  const view = render(<BootGate><p>Workspace</p></BootGate>)
  view.unmount()
  await act(async () => { release(false); await Promise.resolve() })
  await act(() => vi.advanceTimersByTimeAsync(1000))
  expect(gateway.appReady).toHaveBeenCalledTimes(1)
})

it('clears_the_next_poll_on_unmount', async () => {
  vi.mocked(gateway.appReady).mockResolvedValue(false)
  const view = render(<BootGate><p>Workspace</p></BootGate>)
  await act(() => vi.advanceTimersByTimeAsync(0))
  view.unmount()
  await act(() => vi.advanceTimersByTimeAsync(1000))
  expect(gateway.appReady).toHaveBeenCalledTimes(1)
})
