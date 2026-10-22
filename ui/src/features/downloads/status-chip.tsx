import {
  ArrowDownToLine,
  Ban,
  Check,
  CircleX,
  Clock,
  LoaderCircle,
  Pause,
  ShieldCheck,
  ShieldQuestion,
  type LucideIcon,
} from 'lucide-react'

import { useTranslation } from '@/shared/i18n'
import type { DownloadStateDto } from '@/shared/ipc/gateway'
import { cn } from '@/shared/lib/utils'
import { Badge } from '@/shared/ui/badge'

const CHIP =
  'font-mono text-[9px] uppercase tracking-widest [&>svg]:size-3 text-[#2a313a]'

interface ChipStyle {
  labelKey: Exclude<keyof typeof import('@/shared/i18n/translations.json').en, `${string}_one` | `${string}_other`>
  icon: LucideIcon
  className: string
  spin?: boolean
}

const STYLES: Record<DownloadStateDto, ChipStyle> = {
  queued: {
    labelKey: 'downloads.status.queued',
    icon: Clock,
    className: 'bg-muted text-muted-foreground border-border',
  },
  resolving: {
    labelKey: 'downloads.status.resolving',
    icon: LoaderCircle,
    className: 'border-brand-cyan bg-transparent text-foreground',
    spin: true,
  },
  downloading: {
    labelKey: 'downloads.status.downloading',
    icon: ArrowDownToLine,
    className: 'bg-brand-cyan',
  },
  verifying: {
    labelKey: 'downloads.status.verifying',
    icon: LoaderCircle,
    className: 'border-brand-cyan bg-transparent text-foreground',
    spin: true,
  },
  paused: { labelKey: 'downloads.status.paused', icon: Pause, className: 'bg-brand-pink' },
  completed: { labelKey: 'common.completed', icon: Check, className: 'bg-brand-yellow' },
  failed: {
    labelKey: 'common.failed',
    icon: CircleX,
    className: 'bg-destructive text-white',
  },
  cancelled: {
    labelKey: 'common.cancelled',
    icon: Ban,
    className: 'bg-muted text-muted-foreground border-border',
  },
}

/** A state chip. Icon and text label always travel together; color is never the only signal. */
export function StatusChip({ state }: { state: DownloadStateDto }) {
  const { t } = useTranslation()
  const style = STYLES[state]
  const Icon = style.icon
  return (
    <Badge variant="outline" className={cn(CHIP, style.className)}>
      <Icon
        aria-hidden="true"
        className={cn(style.spin && 'motion-safe:animate-spin')}
      />
      {t(style.labelKey)}
    </Badge>
  )
}

/** Integrity of a completed file: verified against a published checksum, or only locally hashed. */
export function IntegrityChip({ verified }: { verified: boolean }) {
  const { t } = useTranslation()
  return verified ? (
    <Badge variant="outline" className={cn(CHIP, 'bg-brand-yellow')}>
      <ShieldCheck aria-hidden="true" />
      {t('common.verified')}
    </Badge>
  ) : (
    <Badge
      variant="outline"
      className={cn(CHIP, 'border-brand-yellow bg-transparent text-foreground')}
    >
      <ShieldQuestion aria-hidden="true" />
      {t('downloads.unverified')}
    </Badge>
  )
}
