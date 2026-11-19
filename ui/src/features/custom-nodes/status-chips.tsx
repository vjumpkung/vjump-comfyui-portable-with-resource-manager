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

export type ChipVariant = 'default' | 'accent' | 'muted' | 'outline' | 'destructive'

/** A chip always carries an icon and a text label; color is never the only signal. */
export function Chip({
  icon: Icon,
  variant,
  spin,
  children,
}: {
  icon: LucideIcon
  variant: ChipVariant
  spin?: boolean
  children: string
}) {
  return (
    <Badge variant={variant} className="type-tag">
      <Icon aria-hidden="true" className={cn(spin && 'motion-safe:animate-spin')} />
      {children}
    </Badge>
  )
}

const SUPPORT: Record<
  SupportDto,
  { labelKey: NodeTextKey; icon: LucideIcon; variant: ChipVariant }
> = {
  compatible: { labelKey: 'customNodes.support.compatible', icon: Check, variant: 'default' },
  incompatible: {
    labelKey: 'customNodes.support.incompatible',
    icon: CircleX,
    variant: 'destructive',
  },
  unknown: { labelKey: 'common.unknownValue', icon: CircleHelp, variant: 'muted' },
}

export function SupportChip({ name, value }: { name: string; value: SupportDto }) {
  const { t } = useTranslation()
  const style = SUPPORT[value]
  return (
    <Chip icon={style.icon} variant={style.variant}>
      {t('customNodes.supportLabel', { name, support: t(style.labelKey) })}
    </Chip>
  )
}

export function EnabledChip({ enabled }: { enabled: boolean }) {
  const { t } = useTranslation()
  return enabled ? (
    <Chip icon={Check} variant="default">
      {t('common.enabled')}
    </Chip>
  ) : (
    <Chip icon={Square} variant="muted">
      {t('common.disabled')}
    </Chip>
  )
}

export function DependencyChip({ state }: { state: DependencyStateDto }) {
  const { t } = useTranslation()
  switch (state) {
    case 'ok':
      return (
        <Chip icon={ShieldCheck} variant="default">
          {t('customNodes.status.depsOk')}
        </Chip>
      )
    case 'failed':
      return (
        <Chip icon={TriangleAlert} variant="accent">
          {t('customNodes.status.depsFailed')}
        </Chip>
      )
    case 'not_run':
      return (
        <Chip icon={Clock} variant="muted">
          {t('customNodes.status.depsNotRun')}
        </Chip>
      )
  }
}

export function HealthChip({ health }: { health: GitHealth }) {
  const { t } = useTranslation()
  switch (health) {
    case 'clean':
      return (
        <Chip icon={Check} variant="default">
          {t('customNodes.status.clean')}
        </Chip>
      )
    case 'dirty':
      return (
        <Chip icon={TriangleAlert} variant="accent">
          {t('customNodes.status.localChanges')}
        </Chip>
      )
    case 'unknown':
      return (
        <Chip icon={CircleHelp} variant="muted">
          {t('customNodes.status.gitUnknown')}
        </Chip>
      )
    case 'not_git':
      return <span className="text-xs text-muted-foreground">—</span>
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
  if (phase === 'failed')
    return (
      <Chip icon={CircleX} variant="destructive">
        {t('common.failed')}
      </Chip>
    )
  if (phase === 'completed')
    return (
      <Chip icon={Check} variant="default">
        {t('common.completed')}
      </Chip>
    )
  if (phase === 'verified')
    return (
      <Chip icon={ShieldCheck} variant="default">
        {t('common.verified')}
      </Chip>
    )
  if (phase === 'cleanup_pending') {
    return (
      <Chip icon={Trash2} variant="accent">
        {t('customNodes.status.cleanupPending')}
      </Chip>
    )
  }
  if (phase === 'prepared')
    return (
      <Chip icon={Clock} variant="muted">
        {t('common.prepared')}
      </Chip>
    )
  return (
    <Chip icon={LoaderCircle} variant="outline" spin={!isTerminalPhase(phase)}>
      {Object.hasOwn(PHASE_LABELS, phase)
        ? t(PHASE_LABELS[phase as keyof typeof PHASE_LABELS])
        : phase.replaceAll('_', ' ')}
    </Chip>
  )
}
