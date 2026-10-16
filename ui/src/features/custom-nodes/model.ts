import type { CommitInput, NodeSourceDto, PlanReviewDto } from '@/shared/ipc/gateway'

export function sourceLabel(source: NodeSourceDto): string {
  switch (source.kind) {
    case 'registry': return `${source.id}@${source.version}`
    case 'git': return `${source.repository}@${source.commit}`
    case 'manual': return 'Manual / unmanaged'
  }
}

export interface Consent {
  execute: boolean
  hook: boolean
  warnings: boolean
  pins: boolean
  remember: boolean
  rememberHook: boolean
}

export const EMPTY_CONSENT: Consent = {
  execute: false, hook: false, warnings: false, pins: false,
  remember: false, rememberHook: false,
}

export function canCommit(plan: PlanReviewDto, consent: Consent): boolean {
  return (consent.execute || plan.trustedSource) &&
    (plan.warnings.length === 0 || consent.warnings) &&
    (plan.protectedChanges.length === 0 || consent.pins)
}

export function decision(plan: PlanReviewDto, consent: Consent): CommitInput {
  return {
    planId: plan.id,
    binding: plan.binding,
    executeNodeCode: consent.execute,
    runInstallPy: plan.hasInstallPy && consent.hook,
    acknowledgeWarnings: consent.warnings,
    rememberSource: consent.remember,
    rememberInstallPy: plan.hasInstallPy && consent.hook && consent.rememberHook,
    confirmedChanges: consent.pins ? plan.protectedChanges : [],
  }
}
