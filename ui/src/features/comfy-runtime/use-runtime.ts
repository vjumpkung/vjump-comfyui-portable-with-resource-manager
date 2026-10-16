import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useEffect, useSyncExternalStore } from 'react'

import { gateway, type StartResultDto } from '@/shared/ipc/gateway'
import { environmentKeys } from '@/shared/lib/environment-queries'

import { runtimeStore, type RuntimeState } from './runtime-store'

export const comfyKeys = {
  launchOptions: ['comfy-launch-options'] as const,
  versions: ['comfy-versions'] as const,
  extraPaths: ['comfy-extra-model-paths'] as const,
}

/** The shared live runtime state; holds the single subscription open while mounted. */
export function useRuntime(): RuntimeState {
  useEffect(() => runtimeStore.acquire(), [])
  return useSyncExternalStore(runtimeStore.subscribe, runtimeStore.getSnapshot)
}

export function useRuntimeActions() {
  const client = useQueryClient()
  const settle = () => {
    void runtimeStore.refresh().catch(() => undefined)
    void client.invalidateQueries({ queryKey: comfyKeys.versions })
    void client.invalidateQueries({ queryKey: environmentKeys.info })
  }
  const start = useMutation<StartResultDto>({
    mutationFn: () => gateway.comfyStart(),
    onSettled: settle,
  })
  const restart = useMutation<StartResultDto>({
    mutationFn: () => gateway.comfyRestart(),
    onSettled: settle,
  })
  const stop = useMutation({
    mutationFn: () => gateway.comfyStop(),
    onSettled: settle,
  })
  return { start, restart, stop }
}
