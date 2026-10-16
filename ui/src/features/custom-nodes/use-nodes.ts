import { useQuery, useQueryClient, type QueryClient } from '@tanstack/react-query'
import { useEffect, useState } from 'react'

import { gateway, type NodeEventDto } from '@/shared/ipc/gateway'
import { environmentKeys } from '@/shared/lib/environment-queries'

import { isTerminalPhase } from './model'

export const NODE_KEY = ['custom-nodes'] as const
export const NODE_OPERATION_KEY = ['node-operation'] as const

const MAX_LOGS = 300
const RETRY_MS = 2000
const LOG_REFRESH_MS = 250
const LIST_POLL_MS = 5000
const OPERATION_POLL_MS = 1000

/** One live log line. Operation identity is a field, never a prefix of the text. */
export interface NodeLogEntry {
  epoch: string
  sequence: number | null
  operationId: string
  line: string
}

/**
 * Refreshes everything a node mutation can change, including after a failure or cancellation:
 * the node list, operation state and the environment health that Repair banners read.
 */
export function refreshNodeState(client: QueryClient): void {
  void client.invalidateQueries({ queryKey: NODE_KEY })
  void client.invalidateQueries({ queryKey: NODE_OPERATION_KEY })
  void client.invalidateQueries({ queryKey: environmentKeys.status })
  void client.invalidateQueries({ queryKey: environmentKeys.info })
  void client.invalidateQueries({ queryKey: environmentKeys.packages })
}

/**
 * Events are hints; list and status reads stay authoritative. Polling continues when the
 * channel is silent or its subscription fails. Closing the subscription never cancels work.
 */
export function useNodes() {
  const client = useQueryClient()
  const [logs, setLogs] = useState<NodeLogEntry[]>([])
  const nodes = useQuery({
    queryKey: NODE_KEY,
    queryFn: gateway.nodeList,
    refetchInterval: LIST_POLL_MS,
  })
  useEffect(() => {
    let alive = true
    // Every subscription attempt owns a generation; callbacks of retired attempts are ignored.
    let generation = 0
    let subscription: number | null = null
    let retry: ReturnType<typeof setTimeout> | undefined
    let statusTimer: ReturnType<typeof setTimeout> | undefined
    let cursor: { epoch: string; sequence: number | null } | null = null
    let reconnecting = false

    const refreshAll = () => {
      void client.invalidateQueries({ queryKey: NODE_KEY })
      void client.invalidateQueries({ queryKey: NODE_OPERATION_KEY })
    }
    // Log bursts refresh the selected operation at a bounded rate.
    const refreshStatus = () => {
      if (statusTimer !== undefined) return
      statusTimer = setTimeout(() => {
        statusTimer = undefined
        void client.invalidateQueries({ queryKey: NODE_OPERATION_KEY })
      }, LOG_REFRESH_MS)
    }

    function onEvent(attempt: number, event: NodeEventDto) {
      if (!alive || attempt !== generation) return
      // Markers always recover, even when their sequence equals the previous one.
      if (event.kind === 'snapshot' || event.kind === 'gap') {
        cursor = { epoch: event.epoch, sequence: event.sequence }
        refreshAll()
        return
      }
      const sameEpoch = cursor !== null && cursor.epoch === event.epoch
      if (sameEpoch && cursor !== null && cursor.sequence !== null &&
        event.sequence !== null && event.sequence <= cursor.sequence) return
      const continuous = sameEpoch && cursor !== null && cursor.sequence !== null &&
        event.sequence !== null && event.sequence === cursor.sequence + 1
      cursor = { epoch: event.epoch, sequence: event.sequence }
      if (event.kind === 'changed' || !continuous) refreshAll()
      if (event.kind === 'log') {
        const entry: NodeLogEntry = {
          epoch: event.epoch,
          sequence: event.sequence,
          operationId: event.operationId,
          line: event.line,
        }
        setLogs((current) => [...current, entry].slice(-MAX_LOGS))
        refreshStatus()
      }
    }

    function connect() {
      generation += 1
      const attempt = generation
      gateway.nodeSubscribe((event) => onEvent(attempt, event)).then((id) => {
        if (!alive || attempt !== generation) {
          void gateway.nodeUnsubscribe(id).catch(() => undefined)
          return
        }
        subscription = id
        if (reconnecting) {
          reconnecting = false
          refreshAll()
        }
      }).catch(() => {
        if (!alive || attempt !== generation) return
        reconnecting = true
        retry = setTimeout(connect, RETRY_MS)
      })
    }
    connect()
    return () => {
      alive = false
      generation += 1
      clearTimeout(retry)
      clearTimeout(statusTimer)
      // Only the event stream is released; running backend work is never cancelled here.
      if (subscription !== null) void gateway.nodeUnsubscribe(subscription).catch(() => undefined)
    }
  }, [client])
  return { ...nodes, logs }
}

/** The authoritative status of one operation; polled every second until it is final. */
export function useNodeOperation(operationId: string | null) {
  return useQuery({
    queryKey: [...NODE_OPERATION_KEY, operationId],
    queryFn: () => gateway.nodeOperationStatus(operationId ?? ''),
    enabled: operationId !== null,
    refetchInterval: (query) => {
      if (query.state.status === 'error') return LIST_POLL_MS
      const phase = query.state.data?.phase
      return phase !== undefined && isTerminalPhase(phase) ? false : OPERATION_POLL_MS
    },
  })
}
