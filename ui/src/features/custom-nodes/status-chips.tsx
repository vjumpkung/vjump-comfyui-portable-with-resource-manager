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

import { useTranslation } from '@/shared/i18n'
import type { DependencyStateDto, SupportDto } from '@/shared/ipc/gateway'
import { cn } from '@/shared/lib/utils'
import { Badge } from '@/shared/ui/badge'

import { isTerminalPhase, type GitHealth, type NodeTextKey } from './model'

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

const SUPPORT: Record<SupportDto, { labelKey: NodeTextKey; icon: LucideIcon; className: string }> = {
  compatible: { labelKey: 'customNodes.support.compatible', icon: Check, className: YELLOW },
  incompatible: { labelKey: 'customNodes.support.incompatible', icon: CircleX, className: FAILED },
  unknown: { labelKey: 'common.unknownValue', icon: CircleHelp, className: MUTED },
}

export function SupportChip({ name, value }: { name: string; value: SupportDto }) {
  const { t } = useTranslation()
  const style = SUPPORT[value]
  return <Chip icon={style.icon} className={style.className}>{t('customNodes.supportLabel', { name, support: t(style.labelKey) })}</Chip>
}

export function EnabledChip({ enabled }: { enabled: boolean }) {
  const { t } = useTranslation()
  return enabled
    ? <Chip icon={Check} className={YELLOW}>{t('common.enabled')}</Chip>
    : <Chip icon={Square} className={MUTED}>{t('common.disabled')}</Chip>
}

export function DependencyChip({ state }: { state: DependencyStateDto }) {
  const { t } = useTranslation()
  switch (state) {
    case 'ok': return <Chip icon={ShieldCheck} className={YELLOW}>{t('customNodes.status.depsOk')}</Chip>
    case 'failed': return <Chip icon={TriangleAlert} className={PINK}>{t('customNodes.status.depsFailed')}</Chip>
    case 'not_run': return <Chip icon={Clock} className={MUTED}>{t('customNodes.status.depsNotRun')}</Chip>
  }
}

export function HealthChip({ health }: { health: GitHealth }) {
  const { t } = useTranslation()
  switch (health) {
    case 'clean': return <Chip icon={Check} className={YELLOW}>{t('customNodes.status.clean')}</Chip>
    case 'dirty': return <Chip icon={TriangleAlert} className={PINK}>{t('customNodes.status.localChanges')}</Chip>
    case 'unknown': return <Chip icon={CircleHelp} className={MUTED}>{t('customNodes.status.gitUnknown')}</Chip>
    case 'not_git': return <span className="font-mono text-xs text-muted-foreground">—</span>
  }
}

const PHASE_LABELS = {
  publish_intent: 'customNodes.phase.publishIntent',
  old_backed_up: 'customNodes.phase.oldBackedUp',
  published: 'customNodes.phase.published',
  dependencies_in_progress: 'customNodes.phase.dependenciesInProgress',
} as const satisfies Record<string, NodeTextKey>

export function PhaseChip({ phase }: { phase: string }) {
  const { t } = useTranslation()
  if (phase === 'failed') return <Chip icon={CircleX} className={FAILED}>{t('common.failed')}</Chip>
  if (phase === 'completed') return <Chip icon={Check} className={YELLOW}>{t('common.completed')}</Chip>
  if (phase === 'verified') return <Chip icon={ShieldCheck} className={YELLOW}>{t('common.verified')}</Chip>
  if (phase === 'cleanup_pending') {
    return <Chip icon={Trash2} className={PINK}>{t('customNodes.status.cleanupPending')}</Chip>
  }
  if (phase === 'prepared') return <Chip icon={Clock} className={MUTED}>{t('common.prepared')}</Chip>
  return (
    <Chip icon={LoaderCircle} className={CYAN} spin={!isTerminalPhase(phase)}>
      {Object.hasOwn(PHASE_LABELS, phase)
        ? t(PHASE_LABELS[phase as keyof typeof PHASE_LABELS])
        : phase.replaceAll('_', ' ')}
    </Chip>
  )
}
