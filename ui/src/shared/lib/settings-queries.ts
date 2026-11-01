import { useQuery } from '@tanstack/react-query'

import { gateway } from '@/shared/ipc/gateway'

export const settingsKey = ['settings'] as const

export function useSettingsQuery() {
  return useQuery({ queryKey: settingsKey, queryFn: () => gateway.settingsGet() })
}

export function useSkipSecurityConfirmations(): boolean {
  return useSettingsQuery().data?.skipSecurityConfirmations ?? false
}
