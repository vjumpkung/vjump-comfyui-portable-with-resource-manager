import { beforeEach, expect, it, vi } from 'vitest'

import type { ComfyEventDto, ComfyStatusDto } from '@/shared/ipc/gateway'

import { createRuntimeStore } from './runtime-store'

const status = (
  lifecycle: ComfyStatusDto['lifecycle'],
  pendingCloseRequest: string | null = null,
): ComfyStatusDto => ({
  lifecycle,
  session: null,
  failure: null,
  pendingOperation: null,
  recovery: null,
  origin: null,
  pendingCloseRequest,
})

let handlers: ((event: ComfyEventDto) => void)[] = []
const api = {
  comfySubscribe: vi.fn(),
  comfyUnsubscribe: vi.fn(),
  comfyStatus: vi.fn(),
}

beforeEach(() => {
  handlers = []
  vi.resetAllMocks()
  api.comfySubscribe.mockImplementation(
    (_after: number | null, handler: (event: ComfyEventDto) => void) => {
      handlers.push(handler)
      return Promise.resolve(handlers.length)
    },
  )
  api.comfyUnsubscribe.mockResolvedValue(null)
})

it('applies the snapshot, appends sanitized log chunks and keeps carriage returns', async () => {
  const store = createRuntimeStore(api)
  const release = store.acquire()
  await Promise.resolve()
  handlers[0]({
    kind: 'snapshot',
    cursor: 3,
    status: status('starting'),
    logs: [{ cursor: 2, sessionId: 's', text: 'one\n' }],
    gap: false,
  })
  handlers[0]({
    kind: 'comfy:log',
    chunk: { cursor: 4, sessionId: 's', text: '\x1b[31m10%\rdone\n' },
  })
  handlers[0]({ kind: 'comfy:state', cursor: 5, status: status('running') })
  const state = store.getSnapshot()
  expect(state.status?.lifecycle).toBe('running')
  expect(state.log).toBe('one\n10%\rdone\n')
  expect(state.connected).toBe(true)
  release()
})

it('shares one subscription between consumers and releases it with the last one', async () => {
  const store = createRuntimeStore(api)
  const first = store.acquire()
  const second = store.acquire()
  await Promise.resolve()
  expect(api.comfySubscribe).toHaveBeenCalledTimes(1)
  first()
  first()
  expect(api.comfyUnsubscribe).not.toHaveBeenCalled()
  second()
  expect(api.comfyUnsubscribe).toHaveBeenCalledWith(1)
})

it('takes a fresh snapshot after a gap and ignores events of the old subscription', async () => {
  const store = createRuntimeStore(api)
  const release = store.acquire()
  await Promise.resolve()
  handlers[0]({ kind: 'gap', cursor: 9 })
  await Promise.resolve()
  expect(api.comfySubscribe).toHaveBeenCalledTimes(2)
  expect(api.comfyUnsubscribe).toHaveBeenCalledWith(1)
  handlers[0]({ kind: 'comfy:state', cursor: 10, status: status('crashed') })
  expect(store.getSnapshot().status).toBeNull()
  handlers[1]({ kind: 'comfy:state', cursor: 11, status: status('running') })
  expect(store.getSnapshot().status?.lifecycle).toBe('running')
  release()
})

it('tracks a pending close request and its resolution', async () => {
  const store = createRuntimeStore(api)
  const release = store.acquire()
  await Promise.resolve()
  handlers[0]({
    kind: 'snapshot',
    cursor: 1,
    status: status('running', 'close-1'),
    logs: [],
    gap: false,
  })
  expect(store.getSnapshot().closeRequest).toBe('close-1')
  handlers[0]({ kind: 'closeRequested', requestId: null })
  expect(store.getSnapshot().closeRequest).toBeNull()
  release()
})

it('bounds the retained log text', async () => {
  const store = createRuntimeStore(api)
  const release = store.acquire()
  await Promise.resolve()
  const text = 'x'.repeat(200 * 1024)
  for (const cursor of [1, 2]) {
    handlers[0]({
      kind: 'comfy:log',
      chunk: { cursor, sessionId: 's', text },
    })
  }
  expect(store.getSnapshot().log.length).toBe(256 * 1024)
  release()
})
