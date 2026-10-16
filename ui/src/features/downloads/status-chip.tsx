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

import type { DownloadStateDto } from '@/shared/ipc/gateway'
import { cn } from '@/shared/lib/utils'
import { Badge } from '@/shared/ui/badge'

const CHIP =
  'font-mono text-[9px] uppercase tracking-widest [&>svg]:size-3 text-[#2a313a]'

interface ChipStyle {
  label: string
  icon: LucideIcon
  className: string
  spin?: boolean
}

const STYLES: Record<DownloadStateDto, ChipStyle> = {
  queued: {
    label: 'Queued',
    icon: Clock,
    className: 'bg-muted text-muted-foreground border-border',
  },
  resolving: {
    label: 'Resolving',
    icon: LoaderCircle,
    className: 'border-brand-cyan bg-transparent text-foreground',
    spin: true,
  },
  downloading: {
    label: 'Downloading',
    icon: ArrowDownToLine,
    className: 'bg-brand-cyan',
  },
  verifying: {
    label: 'Verifying',
    icon: LoaderCircle,
    className: 'border-brand-cyan bg-transparent text-foreground',
    spin: true,
  },
  paused: { label: 'Paused', icon: Pause, className: 'bg-brand-pink' },
  completed: { label: 'Completed', icon: Check, className: 'bg-brand-yellow' },
  failed: {
    label: 'Failed',
    icon: CircleX,
    className: 'bg-destructive text-white',
  },
  cancelled: {
    label: 'Cancelled',
    icon: Ban,
    className: 'bg-muted text-muted-foreground border-border',
  },
}

/** A state chip. Icon and text label always travel together; color is never the only signal. */
export function StatusChip({ state }: { state: DownloadStateDto }) {
  const style = STYLES[state]
  const Icon = style.icon
  return (
    <Badge variant="outline" className={cn(CHIP, style.className)}>
      <Icon
        aria-hidden="true"
        className={cn(style.spin && 'motion-safe:animate-spin')}
      />
      {style.label}
    </Badge>
  )
}

/** Integrity of a completed file: verified against a published checksum, or only locally hashed. */
export function IntegrityChip({ verified }: { verified: boolean }) {
  return verified ? (
    <Badge variant="outline" className={cn(CHIP, 'bg-brand-yellow')}>
      <ShieldCheck aria-hidden="true" />
      Verified
    </Badge>
  ) : (
    <Badge
      variant="outline"
      className={cn(CHIP, 'border-brand-yellow bg-transparent text-foreground')}
    >
      <ShieldQuestion aria-hidden="true" />
      Unverified
    </Badge>
  )
}
