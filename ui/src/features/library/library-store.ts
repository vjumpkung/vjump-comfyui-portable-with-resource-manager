import {
  gateway,
  type LibrarySnapshotDto,
  type LibraryStatusDto,
  type ModelEventDto,
} from '@/shared/ipc/gateway'

const RETRY_MS = 2000

export interface LibraryState {
  snapshot: LibrarySnapshotDto | null
  /** The newest status, which can be ahead of the loaded list. */
  status: LibraryStatusDto | null
  connected: boolean
  rescanning: boolean
}

type Api = Pick<
  typeof gateway,
  'modelSubscribe' | 'modelUnsubscribe' | 'modelList' | 'modelRescan'
>

/**
 * One shared library subscription. Change notices are coalesced by the backend; every notice
 * triggers one list refetch, and notices that arrive during a fetch trigger exactly one more.
 */
export function createLibraryStore(api: Api = gateway) {
  let state: LibraryState = {
    snapshot: null,
    status: null,
    connected: false,
    rescanning: false,
  }
  const listeners = new Set<() => void>()
  let consumers = 0
  let generation = 0
  let subscriptionId: number | null = null
  let retry: ReturnType<typeof setTimeout> | null = null
  let fetching = false
  let fetchAgain = false

  function set(patch: Partial<LibraryState>) {
    state = { ...state, ...patch }
    listeners.forEach((listener) => listener())
  }

  async function refresh() {
    if (fetching) {
      fetchAgain = true
      return
    }
    fetching = true
    try {
      do {
        fetchAgain = false
        const snapshot = await api.modelList()
        set({ snapshot, status: snapshot.status })
      } while (fetchAgain)
    } catch {
      // The next change notice fetches again.
    } finally {
      fetching = false
    }
  }

  function onEvent(attempt: number, event: ModelEventDto) {
    if (attempt !== generation) return
    set({ status: event.status, connected: true })
    void refresh()
  }

  function drop() {
    generation += 1
    if (retry) clearTimeout(retry)
    retry = null
    const id = subscriptionId
    subscriptionId = null
    if (id !== null) void api.modelUnsubscribe(id).catch(() => undefined)
  }

  function connect() {
    const attempt = ++generation
    api
      .modelSubscribe((event) => onEvent(attempt, event))
      .then((id) => {
        if (attempt !== generation || consumers === 0) {
          void api.modelUnsubscribe(id).catch(() => undefined)
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

  return {
    subscribe: (listener: () => void) => {
      listeners.add(listener)
      return () => {
        listeners.delete(listener)
      }
    },
    getSnapshot: () => state,
    refresh,
    /** Resolves when the backend finished the scan. */
    rescan: async () => {
      set({ rescanning: true })
      try {
        const snapshot = await api.modelRescan()
        set({ snapshot, status: snapshot.status })
      } finally {
        set({ rescanning: false })
      }
    },
    acquire: () => {
      consumers += 1
      if (consumers === 1) {
        connect()
        void refresh()
      }
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

export type LibraryStore = ReturnType<typeof createLibraryStore>
export const libraryStore = createLibraryStore()
