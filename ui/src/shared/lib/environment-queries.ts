import { useQuery } from '@tanstack/react-query'

import { gateway } from '@/shared/ipc/gateway'

export const environmentKeys = {
  status: ['setup-status'] as const,
  info: ['environment-info'] as const,
  packages: ['environment-packages'] as const,
}

/** Polling is also the reconnect path: backend work survives a closed Channel. */
export function useSetupStatus() {
  return useQuery({
    queryKey: environmentKeys.status,
    queryFn: gateway.setupStatus,
    refetchInterval: (query) =>
      query.state.data?.phase === 'run_in_progress' ? 1500 : 5000,
  })
}
