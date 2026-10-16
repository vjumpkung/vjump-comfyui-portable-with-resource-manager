import {
  keepPreviousData,
  useMutation,
  useQuery,
  useQueryClient,
} from '@tanstack/react-query'
import { useEffect, useState } from 'react'

import { gateway, type RegistrySearchInput } from '@/shared/ipc/gateway'

export const REGISTRY_KEY = ['node-registry'] as const

/** The backend allows at most 200 per page. */
export const REGISTRY_LIMITS = [25, 50, 100, 200] as const
export const DEFAULT_REGISTRY_LIMIT = 50
export const SEARCH_DEBOUNCE_MS = 300

export type RegistryQuery = Omit<RegistrySearchInput, 'refresh'>

const sameQuery = (a: RegistryQuery, b: RegistryQuery) =>
  a.search === b.search && a.page === b.page && a.limit === b.limit

/** Registry browse state: a debounced search, and a page that resets when the query changes. */
export function useRegistryControls() {
  const [draft, setDraft] = useState('')
  const [query, setQuery] = useState<RegistryQuery>({
    search: '',
    page: 1,
    limit: DEFAULT_REGISTRY_LIMIT,
  })
  useEffect(() => {
    const timer = setTimeout(() => {
      const search = draft.trim()
      setQuery((current) =>
        current.search === search ? current : { ...current, search, page: 1 })
    }, SEARCH_DEBOUNCE_MS)
    return () => clearTimeout(timer)
  }, [draft])
  return {
    draft,
    setDraft,
    query,
    setPage: (page: number) =>
      setQuery((current) => ({ ...current, page: Math.max(1, page) })),
    setLimit: (limit: number) =>
      setQuery((current) =>
        current.limit === limit ? current : { ...current, limit, page: 1 }),
  }
}

/**
 * One registry page. Search, page and limit key the cache; `refresh` never does. A manual refresh
 * is a one-shot request: ordinary queries always send `refresh: false`.
 */
export function useRegistry(input: RegistryQuery) {
  const client = useQueryClient()
  const key = [...REGISTRY_KEY, input.search, input.page, input.limit] as const
  const page = useQuery({
    queryKey: key,
    queryFn: () => gateway.nodeRegistrySearch({ ...input, refresh: false }),
    placeholderData: keepPreviousData,
    staleTime: 30_000,
  })
  const refresh = useMutation({
    mutationFn: (request: RegistryQuery) =>
      gateway.nodeRegistrySearch({ ...request, refresh: true }),
    onSuccess: (data, request) => {
      client.setQueryData(
        [...REGISTRY_KEY, request.search, request.page, request.limit],
        data,
      )
    },
  })
  const refreshed = refresh.variables !== undefined && sameQuery(refresh.variables, input)
  return {
    data: page.data,
    error: page.error,
    isPending: page.isPending,
    isFetching: page.isFetching,
    /** The data belongs to an earlier query while the new one loads: it must not be installed. */
    isPlaceholderData: page.isPlaceholderData,
    /** Backend-confirmed: served from the cache because the registry was unreachable. */
    stale: page.data?.stale ?? false,
    fetchedAt: page.data?.fetchedAt ?? null,
    refresh: () => refresh.mutate(input),
    refreshing: refresh.isPending,
    /** A failed manual refresh of this query; distinct from a stale cache. */
    refreshError: refreshed ? refresh.error : null,
    retry: () => void page.refetch(),
  }
}
