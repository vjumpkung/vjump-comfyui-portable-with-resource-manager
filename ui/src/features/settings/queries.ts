import { useIsMutating, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import {
  gateway,
  type CredentialKindDto,
  type UpdateSettingsDto,
} from '@/shared/ipc/gateway'

export const settingsKey = ['settings'] as const
const settingsUpdateKey = ['settings', 'update'] as const
export const credentialStatusKey = (kind: CredentialKindDto) =>
  ['credential-status', kind] as const

export function useSettings() {
  return useQuery({ queryKey: settingsKey, queryFn: () => gateway.settingsGet() })
}

export function useUpdateSettings() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationKey: settingsUpdateKey,
    mutationFn: (input: UpdateSettingsDto) => gateway.settingsSet(input),
    onSuccess: (settings) => queryClient.setQueryData(settingsKey, settings),
  })
}

export function useSettingsSavePending() {
  return useIsMutating({ mutationKey: settingsUpdateKey }) > 0
}

export function useCredentialStatus(kind: CredentialKindDto) {
  return useQuery({
    queryKey: credentialStatusKey(kind),
    queryFn: () => gateway.secretStatus(kind),
  })
}

export function useClearCredential() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (kind: CredentialKindDto) => gateway.secretClear(kind),
    onSettled: (_data, _error, kind) =>
      queryClient.invalidateQueries({ queryKey: credentialStatusKey(kind) }),
  })
}
