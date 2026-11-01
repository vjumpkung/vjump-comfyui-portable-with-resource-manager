import { useEffect, useRef, useState, type ReactNode } from 'react'

import { gateway } from '@/shared/ipc/gateway'
import { LoadingScreen } from '@/shared/ui/loading-screen'

export function BootGate({ children }: { children: ReactNode }) {
  const [ready, setReady] = useState(false)
  const request = useRef<Promise<boolean> | null>(null)

  useEffect(() => {
    let cancelled = false
    let timer: ReturnType<typeof setTimeout> | undefined
    async function poll() {
      try {
        request.current ??= gateway.appReady().finally(() => { request.current = null })
        const isReady = await request.current
        if (cancelled) return
        if (isReady) {
          setReady(true)
          return
        }
      } catch {
        // Startup IPC may be temporarily unavailable; keep the loading screen.
      }
      if (!cancelled) timer = setTimeout(() => { void poll() }, 150)
    }
    void poll()
    return () => {
      cancelled = true
      clearTimeout(timer)
    }
  }, [])

  return ready ? children : <LoadingScreen />
}
