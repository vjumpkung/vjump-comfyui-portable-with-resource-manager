import { t } from '@/shared/i18n'
import type { ModelEntryDto, ModelLocationDto } from '@/shared/ipc/gateway'

/** Lists longer than this render only the visible window. */
export const VIRTUALIZE_AFTER = 200

export type SortKey = 'name' | 'category' | 'size' | 'modified'

export interface Filters {
  search: string
  /** `all` or a category name. */
  category: string
  /** `all` or a root id. */
  root: string
  sort: SortKey
  descending: boolean
}

export const DEFAULT_FILTERS: Filters = {
  search: '',
  category: 'all',
  root: 'all',
  sort: 'name',
  descending: false,
}

/** A stable identity that survives refetches, scrolling and filtering. */
export function keyOf(location: ModelLocationDto): string {
  return `${location.rootId}:${location.path}`
}

export function filterModels(
  models: readonly ModelEntryDto[],
  filters: Filters,
): ModelEntryDto[] {
  const needle = filters.search.trim().toLowerCase()
  const rows = models.filter((model) => {
    if (filters.category !== 'all' && model.category !== filters.category)
      return false
    if (filters.root !== 'all' && model.location.rootId !== filters.root)
      return false
    if (needle === '') return true
    return (
      model.name.toLowerCase().includes(needle) ||
      model.subfolder.toLowerCase().includes(needle) ||
      model.category.toLowerCase().includes(needle)
    )
  })
  const direction = filters.descending ? -1 : 1
  const compare = (a: ModelEntryDto, b: ModelEntryDto): number => {
    switch (filters.sort) {
      case 'category':
        return (
          a.category.localeCompare(b.category) || a.name.localeCompare(b.name)
        )
      case 'size':
        return (a.size ?? 0) - (b.size ?? 0)
      case 'modified':
        return (a.modifiedMs ?? 0) - (b.modifiedMs ?? 0)
      default:
        return a.name.localeCompare(b.name, undefined, { numeric: true })
    }
  }
  return rows.sort((a, b) => direction * compare(a, b))
}

export function categoriesOf(models: readonly ModelEntryDto[]): string[] {
  return [...new Set(models.map((model) => model.category))].sort((a, b) =>
    a.localeCompare(b),
  )
}

const UNITS = ['B', 'KiB', 'MiB', 'GiB', 'TiB'] as const

export function formatBytes(value: number | null): string {
  if (value === null || !Number.isFinite(value) || value < 0) return '0 B'
  let unit = 0
  let scaled = value
  while (scaled >= 1024 && unit < UNITS.length - 1) {
    scaled /= 1024
    unit += 1
  }
  return `${unit === 0 ? scaled.toFixed(0) : scaled.toFixed(1)} ${UNITS[unit]}`
}

export function sourceLabel(model: ModelEntryDto): string {
  if (model.civitaiVersionId !== null)
    return `CivitAI v${model.civitaiVersionId}`
  if (model.sourceJobId !== null) return t('library.source.downloaded')
  return t('library.source.local')
}

export function formatModified(ms: number | null): string {
  if (ms === null || ms <= 0) return t('common.unknownValue')
  return new Date(ms).toLocaleString()
}

/** `Recycle Bin` on Windows, `Trash` elsewhere. */
export function trashName(): string {
  return typeof navigator !== 'undefined' && /windows/i.test(navigator.userAgent)
    ? t('library.recycleBin')
    : t('library.trash')
}
