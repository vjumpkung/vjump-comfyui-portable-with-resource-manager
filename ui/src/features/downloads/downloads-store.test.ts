import { beforeEach, expect, it, vi } from 'vitest'

import type { DownloadEventDto } from '@/shared/ipc/gateway'

import { createDownloadsStore } from './downloads-store'
import { job } from './fixtures'

const cursor = (sequence: number) => ({ epoch: 'e', sequence })
let handlers: ((event: DownloadEventDto) => void)[] = []
const api = { dlSubscribe: vi.fn(), dlUnsubscribe: vi.fn() }

beforeEach(() => {
  handlers = []
  vi.resetAllMocks()
  api.dlSubscribe.mockImplementation(
    (_after: unknown, handler: (event: DownloadEventDto) => void) => {
      handlers.push(handler)
      return Promise.resolve(handlers.length)
    },
  )
  api.dlUnsubscribe.mockResolvedValue(null)
})

it('applies the snapshot, state changes and current-generation progress only', async () => {
  const store = createDownloadsStore(api)
  const release = store.acquire()
  await Promise.resolve()
  handlers[0]({
    kind: 'snapshot',
    cursor: cursor(1),
    jobs: [job({ state: 'downloading' })],
    gap: false,
  })
  const sample = {
    id: 'job-1',
    generation: 1,
    bytes: 100,
    total: { kind: 'exact' as const, bytes: 1000 },
    bytesPerSecond: 10,
    etaSeconds: 90,
    estimated: false,
  }
  handlers[0]({ kind: 'job:progress', cursor: cursor(2), progress: sample })
  handlers[0]({
    kind: 'job:progress',
    cursor: cursor(3),
    progress: { ...sample, generation: 0, bytes: 999 },
  })
  expect(store.getSnapshot().progress['job-1'].bytes).toBe(100)
  handlers[0]({
    kind: 'job:state',
    cursor: cursor(4),
    job: job({ state: 'paused', generation: 2 }),
  })
  const state = store.getSnapshot()
  expect(state.jobs['job-1'].state).toBe('paused')
  expect(state.progress['job-1']).toBeUndefined()
  // A late event of an older attempt cannot overwrite the newer state.
  handlers[0]({
    kind: 'job:state',
    cursor: cursor(5),
    job: job({ state: 'downloading', generation: 1 }),
  })
  expect(store.getSnapshot().jobs['job-1'].state).toBe('paused')
  release()
})

it('shares one subscription and releases it with the last consumer', async () => {
  const store = createDownloadsStore(api)
  const first = store.acquire()
  const second = store.acquire()
  await Promise.resolve()
  expect(api.dlSubscribe).toHaveBeenCalledTimes(1)
  first()
  expect(api.dlUnsubscribe).not.toHaveBeenCalled()
  second()
  expect(api.dlUnsubscribe).toHaveBeenCalledWith(1)
})

it('takes a fresh snapshot after a gap and ignores the old subscription', async () => {
  const store = createDownloadsStore(api)
  const release = store.acquire()
  await Promise.resolve()
  handlers[0]({ kind: 'gap', cursor: cursor(9) })
  await Promise.resolve()
  expect(api.dlSubscribe).toHaveBeenCalledTimes(2)
  expect(api.dlUnsubscribe).toHaveBeenCalledWith(1)
  handlers[0]({
    kind: 'snapshot',
    cursor: cursor(10),
    jobs: [job({ id: 'stale' })],
    gap: false,
  })
  expect(store.getSnapshot().jobs['stale']).toBeUndefined()
  handlers[1]({
    kind: 'snapshot',
    cursor: cursor(11),
    jobs: [job({ id: 'fresh' })],
    gap: false,
  })
  expect(Object.keys(store.getSnapshot().jobs)).toEqual(['fresh'])
  release()
})
