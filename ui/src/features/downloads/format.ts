import { t } from '@/shared/i18n'
import type { DownloadJobDto, ProgressDto, SizeDto } from '@/shared/ipc/gateway'

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

export function formatSize(size: SizeDto): string {
  if (size.bytes === null) return t('downloads.unknownSize')
  return `${size.kind === 'estimated' ? '~' : ''}${formatBytes(size.bytes)}`
}

export function formatSpeed(bytesPerSecond: number | null): string {
  if (bytesPerSecond === null) return '0 B/s'
  return `${formatBytes(bytesPerSecond)}/s`
}

export function formatEta(seconds: number | null): string {
  if (seconds === null || !Number.isFinite(seconds)) return t('downloads.etaUnknown')
  const total = Math.max(0, Math.round(seconds))
  const hours = Math.floor(total / 3600)
  const minutes = Math.floor((total % 3600) / 60)
  const rest = total % 60
  if (hours > 0) return t('downloads.etaHours', { hours, minutes })
  if (minutes > 0) return t('downloads.etaMinutes', { minutes, seconds: rest })
  return t('downloads.etaSeconds', { seconds: rest })
}

/** Percent complete, or null when the total size is unknown. */
export function percent(bytes: number | null, total: SizeDto): number | null {
  if (total.bytes === null || total.bytes <= 0) return null
  return Math.min(100, Math.max(0, ((bytes ?? 0) / total.bytes) * 100))
}

export function isActive(job: DownloadJobDto): boolean {
  return !['completed', 'failed', 'cancelled'].includes(job.state)
}

/** `bytes / total · speed · ETA`, with an estimate marker for engines that cannot measure. */
export function progressText(
  job: DownloadJobDto,
  progress: ProgressDto | undefined,
): string {
  const done = progress?.bytes ?? job.bytesDone ?? 0
  const total = progress?.total ?? job.size
  const parts = [
    total.bytes === null
      ? formatBytes(done)
      : `${formatBytes(done)} / ${formatSize(total)}`,
  ]
  if (progress && job.state === 'downloading') {
    parts.push(formatSpeed(progress.bytesPerSecond))
    parts.push(formatEta(progress.etaSeconds))
  }
  if (job.estimated && job.state === 'downloading') parts.push(t('downloads.estimated'))
  return parts.join(' · ')
}
