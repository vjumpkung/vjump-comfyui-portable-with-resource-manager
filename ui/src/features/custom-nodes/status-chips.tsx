import {
  Check,
  CircleHelp,
  CircleX,
  Clock,
  LoaderCircle,
  ShieldCheck,
  Square,
  Trash2,
  TriangleAlert,
  type LucideIcon,
} from 'lucide-react'

import type { DependencyStateDto, SupportDto } from '@/shared/ipc/gateway'
import { cn } from '@/shared/lib/utils'
import { Badge } from '@/shared/ui/badge'

import { isTerminalPhase, type GitHealth } from './model'

const CHIP =
  'font-mono text-[9px] uppercase tracking-widest [&>svg]:size-3 text-[#2a313a]'
const MUTED = 'bg-muted text-muted-foreground border-border'
const YELLOW = 'bg-brand-yellow'
const PINK = 'bg-brand-pink'
const CYAN = 'border-brand-cyan bg-transparent text-foreground'
const FAILED = 'bg-destructive text-white'

/** A chip always carries an icon and a text label; color is never the only signal. */
export function Chip({ icon: Icon, className, spin, children }: {
  icon: LucideIcon
  className: string
  spin?: boolean
  children: string
}) {
  return (
    <Badge variant="outline" className={cn(CHIP, className)}>
      <Icon aria-hidden="true" className={cn(spin && 'motion-safe:animate-spin')} />
      {children}
    </Badge>
  )
}

const SUPPORT: Record<SupportDto, { label: string; icon: LucideIcon; className: string }> = {
  compatible: { label: 'Compatible', icon: Check, className: YELLOW },
  incompatible: { label: 'Incompatible', icon: CircleX, className: FAILED },
  unknown: { label: 'Unknown', icon: CircleHelp, className: MUTED },
}

export function SupportChip({ name, value }: { name: string; value: SupportDto }) {
  const style = SUPPORT[value]
  return <Chip icon={style.icon} className={style.className}>{`${name}: ${style.label}`}</Chip>
}

export function EnabledChip({ enabled }: { enabled: boolean }) {
  return enabled
    ? <Chip icon={Check} className={YELLOW}>Enabled</Chip>
    : <Chip icon={Square} className={MUTED}>Disabled</Chip>
}

export function DependencyChip({ state }: { state: DependencyStateDto }) {
  switch (state) {
    case 'ok': return <Chip icon={ShieldCheck} className={YELLOW}>Deps ok</Chip>
    case 'failed': return <Chip icon={TriangleAlert} className={PINK}>Deps failed</Chip>
    case 'not_run': return <Chip icon={Clock} className={MUTED}>Deps not run</Chip>
  }
}

export function HealthChip({ health }: { health: GitHealth }) {
  switch (health) {
    case 'clean': return <Chip icon={Check} className={YELLOW}>Clean</Chip>
    case 'dirty': return <Chip icon={TriangleAlert} className={PINK}>Local changes</Chip>
    case 'unknown': return <Chip icon={CircleHelp} className={MUTED}>Git unknown</Chip>
    case 'not_git': return <span className="font-mono text-xs text-muted-foreground">—</span>
  }
}

export function PhaseChip({ phase }: { phase: string }) {
  if (phase === 'failed') return <Chip icon={CircleX} className={FAILED}>Failed</Chip>
  if (phase === 'completed') return <Chip icon={Check} className={YELLOW}>Completed</Chip>
  if (phase === 'verified') return <Chip icon={ShieldCheck} className={YELLOW}>Verified</Chip>
  if (phase === 'cleanup_pending') {
    return <Chip icon={Trash2} className={PINK}>Cleanup pending</Chip>
  }
  if (phase === 'prepared') return <Chip icon={Clock} className={MUTED}>Prepared</Chip>
  return (
    <Chip icon={LoaderCircle} className={CYAN} spin={!isTerminalPhase(phase)}>
      {phase.replaceAll('_', ' ')}
    </Chip>
  )
}
