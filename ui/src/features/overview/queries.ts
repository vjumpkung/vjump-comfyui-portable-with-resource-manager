import { useQuery } from '@tanstack/react-query'

import { gateway } from '@/shared/ipc/gateway'

export function useAppInfo() {
  return useQuery({ queryKey: ['app-info'], queryFn: () => gateway.appInfo() })
}
