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

interface ChipStyle {
  labelKey: Exclude<
    keyof typeof import('@/shared/i18n/translations.json').en,
    `${string}_one` | `${string}_other`
  >
  icon: LucideIcon
  variant: 'default' | 'secondary' | 'accent' | 'muted' | 'outline' | 'destructive'
  spin?: boolean
}

const STYLES: Record<DownloadStateDto, ChipStyle> = {
  queued: { labelKey: 'downloads.status.queued', icon: Clock, variant: 'muted' },
  resolving: {
    labelKey: 'downloads.status.resolving',
    icon: LoaderCircle,
    variant: 'outline',
    spin: true,
  },
  downloading: {
    labelKey: 'downloads.status.downloading',
    icon: ArrowDownToLine,
    variant: 'secondary',
  },
  verifying: {
    labelKey: 'downloads.status.verifying',
    icon: LoaderCircle,
    variant: 'outline',
    spin: true,
  },
  paused: { labelKey: 'downloads.status.paused', icon: Pause, variant: 'accent' },
  completed: { labelKey: 'common.completed', icon: Check, variant: 'default' },
  failed: { labelKey: 'common.failed', icon: CircleX, variant: 'destructive' },
  cancelled: { labelKey: 'common.cancelled', icon: Ban, variant: 'muted' },
}

/** A state chip. Icon and text label always travel together; color is never the only signal. */
export function StatusChip({ state }: { state: DownloadStateDto }) {
  const { t } = useTranslation()
  const style = STYLES[state]
  const Icon = style.icon
  return (
    <Badge variant={style.variant} className="type-tag">
      <Icon aria-hidden="true" className={cn(style.spin && 'motion-safe:animate-spin')} />
      {t(style.labelKey)}
    </Badge>
  )
}

/** Integrity of a completed file: verified against a published checksum, or only locally hashed. */
export function IntegrityChip({ verified }: { verified: boolean }) {
  const { t } = useTranslation()
  return verified ? (
    <Badge variant="default" className="type-tag">
      <ShieldCheck aria-hidden="true" />
      {t('common.verified')}
    </Badge>
  ) : (
    <Badge variant="outline" className="type-tag">
      <ShieldQuestion aria-hidden="true" />
      {t('downloads.unverified')}
    </Badge>
  )
}
