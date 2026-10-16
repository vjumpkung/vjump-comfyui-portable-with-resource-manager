import { useEffect, useSyncExternalStore } from 'react'

import { downloadsStore, type DownloadsState } from './downloads-store'

/** The shared live download state; holds the single subscription open while mounted. */
export function useDownloads(): DownloadsState {
  useEffect(() => downloadsStore.acquire(), [])
  return useSyncExternalStore(
    downloadsStore.subscribe,
    downloadsStore.getSnapshot,
  )
}
