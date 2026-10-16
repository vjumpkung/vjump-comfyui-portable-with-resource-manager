import { gateway, type ComfyEventDto, type ComfyStatusDto } from '@/shared/ipc/gateway'
import { plainStream } from '@/shared/lib/log-text'

/** The retained terminal text. The backend keeps the authoritative 1 MiB ring. */
const MAX_LOG_CHARS = 256 * 1024
const RETRY_MS = 2000

export interface RuntimeState {
  status: ComfyStatusDto | null
  /** Sanitized stream text with carriage returns preserved. */
  log: string
  connected: boolean
  /** A pending manager close request that waits for the user's answer. */
  closeRequest: string | null
}

type Api = Pick<
  typeof gateway,
  'comfySubscribe' | 'comfyUnsubscribe' | 'comfyStatus'
>

function bounded(text: string): string {
  return text.length > MAX_LOG_CHARS ? text.slice(-MAX_LOG_CHARS) : text
}

/**
 * One shared runtime subscription for the whole UI. Consumers acquire and release it; the
 * subscription lives while at least one consumer exists. Closing it never stops ComfyUI.
 */
export function createRuntimeStore(api: Api = gateway) {
  let state: RuntimeState = {
    status: null,
    log: '',
    connected: false,
    closeRequest: null,
  }
  const listeners = new Set<() => void>()
  let consumers = 0
  let generation = 0
  let subscriptionId: number | null = null
  let retry: ReturnType<typeof setTimeout> | null = null

  function set(patch: Partial<RuntimeState>) {
    state = { ...state, ...patch }
    listeners.forEach((listener) => listener())
  }

  function onEvent(attempt: number, event: ComfyEventDto) {
    if (attempt !== generation) return
    switch (event.kind) {
      case 'snapshot':
        set({
          status: event.status,
          log: bounded(event.logs.map((chunk) => plainStream(chunk.text)).join('')),
          connected: true,
          closeRequest: event.status.pendingCloseRequest,
        })
        break
      case 'comfy:state':
        set({ status: event.status })
        break
      case 'comfy:log':
        set({ log: bounded(state.log + plainStream(event.chunk.text)) })
        break
      case 'gap':
        // The subscriber fell behind: take a fresh authoritative snapshot.
        reconnect()
        break
      case 'closeRequested':
        set({ closeRequest: event.requestId })
        break
    }
  }

  function drop() {
    generation += 1
    if (retry) clearTimeout(retry)
    retry = null
    const id = subscriptionId
    subscriptionId = null
    if (id !== null) void api.comfyUnsubscribe(id).catch(() => undefined)
  }

  function connect() {
    const attempt = ++generation
    api
      .comfySubscribe(null, (event) => onEvent(attempt, event))
      .then((id) => {
        if (attempt !== generation || consumers === 0) {
          void api.comfyUnsubscribe(id).catch(() => undefined)
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
    /** Re-reads the status after a command so the UI never waits for the next event. */
    refresh: async () => {
      const status = await api.comfyStatus()
      set({ status, closeRequest: status.pendingCloseRequest })
    },
    clearLog: () => {
      set({ log: '' })
    },
  }
}

export type RuntimeStore = ReturnType<typeof createRuntimeStore>
export const runtimeStore = createRuntimeStore()
