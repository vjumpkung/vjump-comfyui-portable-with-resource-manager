// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, cleanup, configure, fireEvent, render, renderHook, screen, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'

import { gateway, IpcClientError, type RegistryPageDto } from '@/shared/ipc/gateway'

import { registryNode, registryPage } from './fixtures'
import { formatCount, formatList, unknownText } from './model'
import { RegistryBrowser } from './registry-browser'
import { useRegistry, useRegistryControls } from './use-registry'

// Rendering many rows in jsdom is slow when the whole suite runs in parallel.
vi.setConfig({ testTimeout: 30_000 })
configure({ asyncUtilTimeout: 5000 })

vi.mock('@/shared/ipc/gateway', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/ipc/gateway')>()),
  gateway: {
    nodeRegistrySearch: vi.fn(),
    nodeOpenRepository: vi.fn(),
  },
}))
vi.mock('sonner', () => ({ toast: { error: vi.fn(), success: vi.fn(), info: vi.fn() } }))

function wrapper() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  )
}

beforeEach(() => {
  vi.resetAllMocks()
  vi.stubGlobal(
    'ResizeObserver',
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  )
  vi.mocked(gateway.nodeRegistrySearch).mockResolvedValue(registryPage())
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

it('search_resets_page_after_debounce', async () => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval'] })
  const { result } = renderHook(() => useRegistryControls())
  expect(result.current.query).toEqual({ search: '', page: 1, limit: 50 })
  act(() => result.current.setPage(4))
  expect(result.current.query.page).toBe(4)
  act(() => result.current.setDraft('  sdxl '))
  await act(async () => { await vi.advanceTimersByTimeAsync(299) })
  expect(result.current.query).toEqual({ search: '', page: 4, limit: 50 })
  await act(async () => { await vi.advanceTimersByTimeAsync(1) })
  expect(result.current.query).toEqual({ search: 'sdxl', page: 1, limit: 50 })
  // The same committed text keeps the page; a new limit resets it.
  act(() => result.current.setPage(3))
  act(() => result.current.setDraft('sdxl'))
  await act(async () => { await vi.advanceTimersByTimeAsync(300) })
  expect(result.current.query.page).toBe(3)
  act(() => result.current.setLimit(200))
  expect(result.current.query).toEqual({ search: 'sdxl', page: 1, limit: 200 })
})

it('manual_refresh_is_one_shot', async () => {
  const { result, rerender } = renderHook(
    ({ page }: { page: number }) => useRegistry({ search: '', page, limit: 50 }),
    { wrapper: wrapper(), initialProps: { page: 1 } },
  )
  await waitFor(() => expect(result.current.data).toBeDefined())
  expect(gateway.nodeRegistrySearch).toHaveBeenLastCalledWith({
    search: '', page: 1, limit: 50, refresh: false,
  })
  act(() => result.current.refresh())
  await waitFor(() => expect(gateway.nodeRegistrySearch).toHaveBeenCalledTimes(2))
  expect(gateway.nodeRegistrySearch).toHaveBeenLastCalledWith({
    search: '', page: 1, limit: 50, refresh: true,
  })
  await waitFor(() => expect(result.current.refreshing).toBe(false))
  rerender({ page: 2 })
  await waitFor(() => expect(gateway.nodeRegistrySearch).toHaveBeenCalledTimes(3))
  expect(gateway.nodeRegistrySearch).toHaveBeenLastCalledWith({
    search: '', page: 2, limit: 50, refresh: false,
  })
})

it('separates a failed refresh from a backend-confirmed stale cache', async () => {
  vi.mocked(gateway.nodeRegistrySearch)
    .mockResolvedValueOnce(registryPage([registryNode()], { stale: true }))
    .mockRejectedValueOnce(new IpcClientError('registry_refused', 'The registry answered with HTTP 403.'))
  const { result } = renderHook(() => useRegistry({ search: '', page: 1, limit: 50 }), {
    wrapper: wrapper(),
  })
  await waitFor(() => expect(result.current.stale).toBe(true))
  expect(result.current.refreshError).toBeNull()
  act(() => result.current.refresh())
  await waitFor(() => expect(result.current.refreshError).toBeInstanceOf(IpcClientError))
  // The earlier cached page stays visible and still reports the backend's own stale flag.
  expect(result.current.data?.stale).toBe(true)
})

it('previous_query_results_cannot_be_installed', async () => {
  let release: (page: RegistryPageDto) => void = () => undefined
  vi.mocked(gateway.nodeRegistrySearch).mockImplementation((input) =>
    input.search === 'next'
      ? new Promise<RegistryPageDto>((resolve) => { release = resolve })
      : Promise.resolve(registryPage([registryNode({ name: 'Old Result' })])),
  )
  const onInstall = vi.fn()
  render(
    <QueryClientProvider client={new QueryClient()}>
      <RegistryBrowser installDisabledReason={null} onInstall={onInstall} />
    </QueryClientProvider>,
  )
  const install = await screen.findByRole<HTMLButtonElement>('button', { name: 'Install Old Result' })
  expect(install.disabled).toBe(false)
  fireEvent.change(screen.getByLabelText('Search the Comfy Registry'), { target: { value: 'next' } })
  await waitFor(() => expect(gateway.nodeRegistrySearch).toHaveBeenCalledTimes(2))
  const stale = screen.getByRole<HTMLButtonElement>('button', { name: 'Install Old Result' })
  expect(stale.disabled).toBe(true)
  fireEvent.click(stale)
  expect(onInstall).not.toHaveBeenCalled()
  await act(async () => { release(registryPage([registryNode({ name: 'New Result' })])); await Promise.resolve() })
  const fresh = await screen.findByRole<HTMLButtonElement>('button', { name: 'Install New Result' })
  expect(fresh.disabled).toBe(false)
})

it('null_metadata_remains_unknown_and_zero_remains_zero', async () => {
  expect(formatCount(0)).toBe('0')
  expect(formatCount(null)).toBe('—')
  expect(unknownText(null)).toBe('Unknown')
  expect(formatList(null)).toBe('Unknown')
  vi.mocked(gateway.nodeRegistrySearch).mockResolvedValue(registryPage([registryNode({
    name: 'Sparse',
    downloads: 0,
    githubStars: null,
    latestVersion: null,
    publisher: null,
    description: null,
    supportedOs: null,
    supportedAccelerators: null,
    supportedComfyuiVersion: null,
    status: null,
    os: 'unknown',
    accelerator: 'unknown',
    comfyui: 'unknown',
  })]))
  render(
    <QueryClientProvider client={new QueryClient()}>
      <RegistryBrowser installDisabledReason={null} onInstall={vi.fn()} />
    </QueryClientProvider>,
  )
  await screen.findByText('Sparse')
  const row = screen.getByText('Sparse').closest('li') as HTMLElement
  const field = (name: string) => [...row.querySelectorAll('dt')]
    .find((dt) => dt.textContent === name)?.nextElementSibling?.textContent
  expect(field('Downloads')).toBe('0')
  expect(field('GitHub stars')).toBe('—')
  expect(field('Latest version')).toBe('Unknown')
  expect(field('Registry status')).toBe('Unknown')
  expect(field('Supported OS')).toBe('Unknown')
  expect(field('Supported ComfyUI')).toBe('Unknown')
  expect(screen.getByText('No description')).toBeTruthy()
})
