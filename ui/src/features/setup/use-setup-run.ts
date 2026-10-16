import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useEffect, useRef, useState } from 'react'

import {
  gateway,
  type SetupActionDto,
  type SetupEventDto,
} from '@/shared/ipc/gateway'
import { environmentKeys } from '@/shared/lib/environment-queries'

export function useSetupRun() {
  const client = useQueryClient()
  const mounted = useRef(true)
  const [lines, setLines] = useState<string[]>([])
  const [progress, setProgress] = useState<Extract<
    SetupEventDto,
    { kind: 'progress' }
  > | null>(null)
  const [finished, setFinished] = useState<Extract<
    SetupEventDto,
    { kind: 'finished' }
  > | null>(null)
  useEffect(() => {
    mounted.current = true
    return () => {
      mounted.current = false
    }
  }, [])
  const refresh = () => {
    void client.invalidateQueries({ queryKey: environmentKeys.status })
    void client.invalidateQueries({ queryKey: environmentKeys.info })
    void client.invalidateQueries({ queryKey: environmentKeys.packages })
  }
  const cancel = useMutation({
    mutationFn: gateway.setupCancel,
    onSuccess: refresh,
  })
  const action = useMutation({
    mutationFn: (input: SetupActionDto) =>
      gateway.setupRun(input, (event) => {
        if (!mounted.current) return
        if (event.kind === 'log')
          setLines((current) => [
            ...current.slice(-499),
            `[${event.step}] ${event.line}`,
          ])
        if (event.kind === 'progress') setProgress(event)
        if (event.kind === 'finished') setFinished(event)
        if (event.kind !== 'log' && event.kind !== 'progress') refresh()
      }),
    onMutate: () => {
      cancel.reset()
      setFinished(null)
      setProgress(null)
    },
    onSuccess: refresh,
    onError: refresh,
  })
  return { action, cancel, lines, progress, finished }
}
