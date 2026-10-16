import { useEffect, useSyncExternalStore } from 'react'

import {
  libraryStore,
  type LibraryState,
  type LibraryStore,
} from './library-store'

/** The shared live library state; holds the single subscription open while mounted. */
export function useLibrary(store: LibraryStore = libraryStore): LibraryState {
  useEffect(() => store.acquire(), [store])
  return useSyncExternalStore(store.subscribe, store.getSnapshot)
}
