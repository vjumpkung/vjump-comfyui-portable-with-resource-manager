// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, cleanup, configure, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'

import { gateway, IpcClientError, type NodeEventDto } from '@/shared/ipc/gateway'

import { nodeList, operation } from './fixtures'
import { useNodeOperation, useNodes } from './use-nodes'

// Rendering many rows in jsdom is slow when the whole suite runs in parallel.
vi.setConfig({ testTimeout: 30_000 })
configure({ asyncUtilTimeout: 5000 })

vi.mock('@/shared/ipc/gateway', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/ipc/gateway')>()),
  gateway: {
    nodeList: vi.fn(),
    nodeSubscribe: vi.fn(),
    nodeUnsubscribe: vi.fn(),
    nodeOperationStatus: vi.fn(),
    nodeCancel: vi.fn(),
  },
}))

type Handler = (event: NodeEventDto) => void
let handlers: Handler[] = []
const E = 'epoch-1'
const snapshot = (sequence: number | null): NodeEventDto => ({ kind: 'snapshot', epoch: E, sequence })
const changed = (sequence: number | null, epoch = E): NodeEventDto => ({ kind: 'changed', epoch, sequence })
const gap = (sequence: number | null): NodeEventDto => ({ kind: 'gap', epoch: E, sequence })
const log = (sequence: number, line = `line ${sequence}`, operationId = 'op-1'): NodeEventDto => ({
  kind: 'log', epoch: E, sequence, operationId, line,
})

function wrapper() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  )
}

const settle = () => act(async () => { await Promise.resolve() })

beforeEach(() => {
  vi.resetAllMocks()
  handlers = []
  vi.mocked(gateway.nodeList).mockResolvedValue(nodeList())
  vi.mocked(gateway.nodeOperationStatus).mockResolvedValue(operation({ phase: 'published' }))
  vi.mocked(gateway.nodeSubscribe).mockImplementation((handler) => {
    handlers.push(handler)
    return Promise.resolve(handlers.length)
  })
  vi.mocked(gateway.nodeUnsubscribe).mockResolvedValue(null)
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

const calls = () => vi.mocked(gateway.nodeList).mock.calls.length
const quiet = () => act(async () => { await new Promise((resolve) => setTimeout(resolve, 30)) })

it('older_events_do_not_replace_current_state', async () => {
  const { result } = renderHook(() => useNodes(), { wrapper: wrapper() })
  await waitFor(() => expect(handlers).toHaveLength(1))
  await waitFor(() => expect(calls()).toBe(1))
  act(() => handlers[0](snapshot(5)))
  await waitFor(() => expect(calls()).toBe(2))
  act(() => handlers[0](log(6, 'fresh')))
  await waitFor(() => expect(result.current.logs).toHaveLength(1))
  const before = calls()
  act(() => {
    handlers[0](log(6, 'duplicate'))
    handlers[0](log(4, 'older'))
    handlers[0](changed(5))
  })
  await quiet()
  expect(result.current.logs.map((entry) => entry.line)).toEqual(['fresh'])
  expect(calls()).toBe(before)
})

it('equal_sequence_gap_refetches_list_and_status', async () => {
  vi.mocked(gateway.nodeOperationStatus).mockResolvedValue(operation({ phase: 'completed' }))
  renderHook(() => ({ nodes: useNodes(), op: useNodeOperation('op-1') }), { wrapper: wrapper() })
  await waitFor(() => expect(handlers).toHaveLength(1))
  await waitFor(() => expect(gateway.nodeOperationStatus).toHaveBeenCalledTimes(1))
  act(() => handlers[0](snapshot(5)))
  await waitFor(() => expect(gateway.nodeOperationStatus).toHaveBeenCalledTimes(2))
  const lists = calls()
  act(() => handlers[0](gap(5)))
  await waitFor(() => expect(calls()).toBe(lists + 1))
  await waitFor(() => expect(gateway.nodeOperationStatus).toHaveBeenCalledTimes(3))
  // A snapshot marker with the same sequence recovers as well.
  act(() => handlers[0](snapshot(5)))
  await waitFor(() => expect(calls()).toBe(lists + 2))
})

it('retired_subscription_callbacks_are_ignored', async () => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval'] })
  vi.mocked(gateway.nodeSubscribe)
    .mockImplementationOnce((handler) => {
      handlers.push(handler)
      return Promise.reject(new IpcClientError('transport', 'x'))
    })
  const { result, unmount } = renderHook(() => useNodes(), { wrapper: wrapper() })
  await act(async () => { await vi.advanceTimersByTimeAsync(0) })
  await act(async () => { await vi.advanceTimersByTimeAsync(2000) })
  expect(handlers).toHaveLength(2)
  const [retired, current] = handlers
  act(() => retired(log(1, 'from the retired attempt')))
  expect(result.current.logs).toHaveLength(0)
  act(() => current(log(1, 'from the current attempt')))
  expect(result.current.logs.map((entry) => entry.line)).toEqual(['from the current attempt'])
  unmount()
  const before = calls()
  act(() => current(changed(2)))
  await act(async () => { await vi.advanceTimersByTimeAsync(400) })
  expect(calls()).toBe(before)
})

it('failed_subscription_retries', async () => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval'] })
  vi.mocked(gateway.nodeSubscribe)
    .mockRejectedValueOnce(new IpcClientError('transport', 'x'))
    .mockRejectedValueOnce(new IpcClientError('transport', 'x'))
  renderHook(() => useNodes(), { wrapper: wrapper() })
  await act(async () => { await vi.advanceTimersByTimeAsync(0) })
  expect(gateway.nodeSubscribe).toHaveBeenCalledTimes(1)
  await act(async () => { await vi.advanceTimersByTimeAsync(1999) })
  expect(gateway.nodeSubscribe).toHaveBeenCalledTimes(1)
  await act(async () => { await vi.advanceTimersByTimeAsync(1) })
  expect(gateway.nodeSubscribe).toHaveBeenCalledTimes(2)
  expect(calls()).toBe(1)
  await act(async () => { await vi.advanceTimersByTimeAsync(2000) })
  expect(gateway.nodeSubscribe).toHaveBeenCalledTimes(3)
  // The reconnection refreshes the authoritative list.
  expect(calls()).toBe(2)
  expect(gateway.nodeUnsubscribe).not.toHaveBeenCalled()
})

it('late_subscription_is_unsubscribed', async () => {
  let resolve: (id: number) => void = () => undefined
  vi.mocked(gateway.nodeSubscribe).mockImplementationOnce((handler) => {
    handlers.push(handler)
    return new Promise<number>((done) => { resolve = done })
  })
  const { unmount } = renderHook(() => useNodes(), { wrapper: wrapper() })
  await waitFor(() => expect(handlers).toHaveLength(1))
  unmount()
  expect(gateway.nodeUnsubscribe).not.toHaveBeenCalled()
  await act(async () => { resolve(42); await Promise.resolve() })
  await waitFor(() => expect(gateway.nodeUnsubscribe).toHaveBeenCalledWith(42))
})

it('silent_channel_still_polls', async () => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval'] })
  // The channel resolves but never delivers an event, and the subscription never completes.
  vi.mocked(gateway.nodeSubscribe).mockImplementation(() => new Promise<number>(() => undefined))
  renderHook(() => useNodes(), { wrapper: wrapper() })
  await act(async () => { await vi.advanceTimersByTimeAsync(0) })
  expect(calls()).toBe(1)
  await act(async () => { await vi.advanceTimersByTimeAsync(5000) })
  expect(calls()).toBe(2)
  await act(async () => { await vi.advanceTimersByTimeAsync(5000) })
  expect(calls()).toBe(3)
})

it('unmount_never_cancels_backend_work', async () => {
  const { unmount } = renderHook(() => useNodes(), { wrapper: wrapper() })
  await waitFor(() => expect(handlers).toHaveLength(1))
  await settle()
  unmount()
  await waitFor(() => expect(gateway.nodeUnsubscribe).toHaveBeenCalledWith(1))
  expect(gateway.nodeCancel).not.toHaveBeenCalled()
})

it('logs_are_structured_and_bounded', async () => {
  const { result } = renderHook(() => useNodes(), { wrapper: wrapper() })
  await waitFor(() => expect(handlers).toHaveLength(1))
  act(() => {
    handlers[0](snapshot(0))
    for (let sequence = 1; sequence <= 305; sequence += 1) handlers[0](log(sequence, `line ${sequence}`, 'op-9'))
  })
  expect(result.current.logs).toHaveLength(300)
  expect(result.current.logs[0]).toEqual({
    epoch: E, sequence: 6, operationId: 'op-9', line: 'line 6',
  })
  expect(result.current.logs[299].line).toBe('line 305')
  expect(result.current.logs.every((entry) => !entry.line.includes('op-9'))).toBe(true)
})

it('polls a nonterminal operation every second and stops when it is final', async () => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval'] })
  const { rerender } = renderHook(({ id }: { id: string | null }) => useNodeOperation(id), {
    wrapper: wrapper(),
    initialProps: { id: null as string | null },
  })
  await act(async () => { await vi.advanceTimersByTimeAsync(1500) })
  expect(gateway.nodeOperationStatus).not.toHaveBeenCalled()
  rerender({ id: 'op-1' })
  await act(async () => { await vi.advanceTimersByTimeAsync(0) })
  expect(gateway.nodeOperationStatus).toHaveBeenCalledTimes(1)
  await act(async () => { await vi.advanceTimersByTimeAsync(1000) })
  expect(gateway.nodeOperationStatus).toHaveBeenCalledTimes(2)
  vi.mocked(gateway.nodeOperationStatus).mockResolvedValue(operation({ phase: 'completed' }))
  await act(async () => { await vi.advanceTimersByTimeAsync(1000) })
  const final = vi.mocked(gateway.nodeOperationStatus).mock.calls.length
  await act(async () => { await vi.advanceTimersByTimeAsync(3000) })
  expect(vi.mocked(gateway.nodeOperationStatus).mock.calls.length).toBe(final)
})
