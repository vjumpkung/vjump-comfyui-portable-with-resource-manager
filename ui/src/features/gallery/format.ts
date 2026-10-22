import { t } from '@/shared/i18n'
import type {
  GalleryEntryDto,
  GalleryEntryRefDto,
  GalleryLocationDto,
} from '@/shared/ipc/gateway'

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

export function formatModified(ms: number | null): string {
  if (ms === null || ms <= 0) return t('common.unknownValue')
  return new Date(ms).toLocaleString()
}

/** A stable identity that survives refetches, scrolling and navigation. */
export function keyOf(location: GalleryLocationDto): string {
  return `${location.area}:${location.path}`
}

/** The references the backend needs to refuse an entry that changed since it was listed. */
export function entryRefs(entries: readonly GalleryEntryDto[]): GalleryEntryRefDto[] {
  return entries.map(({ location, revision }) => ({ location, revision }))
}

/** The components of an area-relative path; the root has none. */
export function segmentsOf(path: string): string[] {
  return path.split('/').filter(Boolean)
}

export function joinPath(parent: string, name: string): string {
  return [...segmentsOf(parent), name].join('/')
}

/** The last component of a path, or an empty string for the root. */
export function baseName(path: string): string {
  return segmentsOf(path).at(-1) ?? ''
}

/** The extension including its dot, or an empty string (a leading dot alone is not one). */
export function extensionOf(name: string): string {
  const dot = name.lastIndexOf('.')
  return dot <= 0 ? '' : name.slice(dot)
}
