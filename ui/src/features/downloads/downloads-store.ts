import {
  gateway,
  type DownloadCursorDto,
  type DownloadEventDto,
  type DownloadJobDto,
  type ProgressDto,
} from '@/shared/ipc/gateway'

const RETRY_MS = 2000

export interface DownloadsState {
  jobs: Record<string, DownloadJobDto>
  /** Latest throttled sample of each running job. */
  progress: Record<string, ProgressDto>
  connected: boolean
}

type Api = Pick<typeof gateway, 'dlSubscribe' | 'dlUnsubscribe'>

/**
 * One shared download subscription for the whole UI. Consumers acquire and release it; it lives
 * while at least one consumer exists. Closing it never stops a download.
 */
export function createDownloadsStore(api: Api = gateway) {
  let state: DownloadsState = { jobs: {}, progress: {}, connected: false }
  const listeners = new Set<() => void>()
  let consumers = 0
  let generation = 0
  let subscriptionId: number | null = null
  let cursor: DownloadCursorDto | null = null
  let retry: ReturnType<typeof setTimeout> | null = null

  function set(patch: Partial<DownloadsState>) {
    state = { ...state, ...patch }
    listeners.forEach((listener) => listener())
  }

  function onEvent(attempt: number, event: DownloadEventDto) {
    if (attempt !== generation) return
    cursor = event.cursor
    switch (event.kind) {
      case 'snapshot':
        set({
          jobs: Object.fromEntries(event.jobs.map((job) => [job.id, job])),
          progress: {},
          connected: true,
        })
        break
      case 'job:state': {
        const { job } = event
        const known = state.jobs[job.id]
        // A late state of an older attempt must not overwrite a newer one.
        if (known && (known.generation ?? 0) > (job.generation ?? 0)) break
        const progress = { ...state.progress }
        if (job.state !== 'downloading') delete progress[job.id]
        set({ jobs: { ...state.jobs, [job.id]: job }, progress })
        break
      }
      case 'job:progress': {
        const { progress } = event
        const job = state.jobs[progress.id]
        if (!job || job.generation !== progress.generation) break
        set({ progress: { ...state.progress, [progress.id]: progress } })
        break
      }
      case 'gap':
        // The subscriber fell behind: take a fresh authoritative snapshot.
        reconnect()
        break
    }
  }

  function drop() {
    generation += 1
    if (retry) clearTimeout(retry)
    retry = null
    const id = subscriptionId
    subscriptionId = null
    if (id !== null) void api.dlUnsubscribe(id).catch(() => undefined)
  }

  function connect() {
    const attempt = ++generation
    api
      .dlSubscribe(cursor, (event) => onEvent(attempt, event))
      .then((id) => {
        if (attempt !== generation || consumers === 0) {
          void api.dlUnsubscribe(id).catch(() => undefined)
          return
        }
        subscriptionId = id
      })
      .catch(() => {
        if (attempt !== generation || consumers === 0) return
        set({ connected: false })
        retry = setTimeout(() => {
          if (consumers > 0 && attempt === generation) connect()
        }, RETRY_MS)
      })
  }

  function reconnect() {
    drop()
    if (consumers > 0) connect()
  }

  return {
    subscribe: (listener: () => void) => {
      listeners.add(listener)
      return () => {
        listeners.delete(listener)
      }
    },
    getSnapshot: () => state,
    acquire: () => {
      consumers += 1
      if (consumers === 1) connect()
      let released = false
      return () => {
        if (released) return
        released = true
        consumers -= 1
        if (consumers === 0) drop()
      }
    },
  }
}

export type DownloadsStore = ReturnType<typeof createDownloadsStore>
export const downloadsStore = createDownloadsStore()
