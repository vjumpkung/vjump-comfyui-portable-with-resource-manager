import { afterEach, beforeEach, expect, it, vi } from 'vitest'

import type { ModelEventDto } from '@/shared/ipc/gateway'

import { snapshot, status } from './fixtures'
import { createLibraryStore } from './library-store'
import { model } from './fixtures'

let emit: (event: ModelEventDto) => void = () => undefined
const api = {
  modelSubscribe: vi.fn(),
  modelUnsubscribe: vi.fn(),
  modelList: vi.fn(),
  modelRescan: vi.fn(),
}

beforeEach(() => {
  vi.resetAllMocks()
  api.modelSubscribe.mockImplementation(
    (handler: (event: ModelEventDto) => void) => {
      emit = handler
      return Promise.resolve(7)
    },
  )
  api.modelUnsubscribe.mockResolvedValue(null)
  api.modelList.mockResolvedValue(snapshot([model()]))
})
afterEach(() => vi.useRealTimers())

const flush = () => new Promise((resolve) => setTimeout(resolve, 0))

it('loads the list on acquire and again on every change notice', async () => {
  const store = createLibraryStore(api)
  const release = store.acquire()
  await flush()
  expect(store.getSnapshot().snapshot?.models).toHaveLength(1)
  api.modelList.mockResolvedValue(snapshot([model(), model({ name: 'b.pt' })]))
  emit({ kind: 'models:changed', epoch: 'e', status: status({ sequence: 2 }) })
  await flush()
  expect(store.getSnapshot().snapshot?.models).toHaveLength(2)
  expect(store.getSnapshot().connected).toBe(true)
  release()
  await flush()
  expect(api.modelUnsubscribe).toHaveBeenCalledWith(7)
})

it('collapses a burst of notices during a fetch into one more fetch', async () => {
  const store = createLibraryStore(api)
  let finish: (value: ReturnType<typeof snapshot>) => void = () => undefined
  api.modelList.mockReturnValueOnce(
    new Promise((resolve) => {
      finish = resolve
    }),
  )
  store.acquire()
  await flush()
  for (let sequence = 2; sequence < 12; sequence += 1) {
    emit({ kind: 'models:changed', epoch: 'e', status: status({ sequence }) })
  }
  finish(snapshot([model()]))
  await flush()
  await flush()
  // The first fetch plus exactly one follow-up, not eleven.
  expect(api.modelList).toHaveBeenCalledTimes(2)
})

it('ignores events of a dropped subscription and retries a failed one', async () => {
  vi.useFakeTimers()
  api.modelSubscribe.mockRejectedValueOnce(new Error('offline'))
  const store = createLibraryStore(api)
  store.acquire()
  await vi.advanceTimersByTimeAsync(10)
  expect(store.getSnapshot().connected).toBe(false)
  await vi.advanceTimersByTimeAsync(2100)
  expect(api.modelSubscribe).toHaveBeenCalledTimes(2)
})

it('rescan replaces the list with the scanned snapshot', async () => {
  const store = createLibraryStore(api)
  api.modelRescan.mockResolvedValue(snapshot([model({ name: 'fresh.pt' })]))
  await store.rescan()
  expect(store.getSnapshot().snapshot?.models[0].name).toBe('fresh.pt')
  expect(store.getSnapshot().rescanning).toBe(false)
})
