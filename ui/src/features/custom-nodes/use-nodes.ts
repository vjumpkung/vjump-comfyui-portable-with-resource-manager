import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect, useState } from 'react'

import { gateway } from '@/shared/ipc/gateway'

export const NODE_KEY = ['custom-nodes'] as const

/** Events are hints; list/status reads remain authoritative after gaps or reconnects. */
export function useNodes() {
  const client = useQueryClient()
  const [logs, setLogs] = useState<string[]>([])
  const nodes = useQuery({
    queryKey: NODE_KEY,
    queryFn: gateway.nodeList,
    // Also recover from a silently disconnected Channel.
    refetchInterval: 5000,
  })
  useEffect(() => {
    let alive = true
    let subscription: number | null = null
    let retry: ReturnType<typeof setTimeout> | undefined
    let epoch: string | null = null
    let sequence: number | null = null
    const refresh = () => {
      void client.invalidateQueries({ queryKey: NODE_KEY })
      void client.invalidateQueries({ queryKey: ['node-operation'] })
    }
    function connect() {
      gateway.nodeSubscribe((event) => {
        if (!alive) return
        const gap = event.epoch !== epoch || sequence === null ||
          event.sequence === null || event.sequence !== sequence + 1
        if (event.epoch === epoch && event.sequence !== null && sequence !== null &&
          event.sequence <= sequence && event.kind !== 'snapshot') return
        epoch = event.epoch
        sequence = event.sequence
        if (gap || event.kind !== 'log') refresh()
        if (event.kind === 'log') {
          setLogs((current) => [...current, `${event.operationId}: ${event.line}`].slice(-300))
        }
      }).then((id) => {
        if (!alive) void gateway.nodeUnsubscribe(id).catch(() => undefined)
        else subscription = id
      }).catch(() => {
        if (alive) retry = setTimeout(connect, 2000)
      })
    }
    connect()
    return () => {
      alive = false
      clearTimeout(retry)
      if (subscription !== null) void gateway.nodeUnsubscribe(subscription).catch(() => undefined)
    }
  }, [client])
  return { ...nodes, logs }
}
