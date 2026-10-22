import { t } from '@/shared/i18n'
import type {
  CommitInput,
  InstalledNodeDto,
  NodeGitStatusDto,
  NodeSourceDto,
  PlanReviewDto,
} from '@/shared/ipc/gateway'

export type NodeTextKey = Exclude<keyof typeof import('@/shared/i18n/translations.json').en, `${string}_one` | `${string}_other`>

/** Manager copy is stored as keys and data; strings here are backend diagnostics. */
export interface NodeText {
  key: NodeTextKey
  values?: Readonly<Record<string, string | number>>
}

export function nodeText(value: NodeText | string): string {
  return typeof value === 'string' ? value : t(value.key, value.values)
}

export type ActionContext =
  | { kind: 'prepare'; preparation: NodePreparation }
  | { kind: 'apply' | 'enable' | 'disable' | 'uninstall'; name: string }
  | { kind: 'stop' | 'restart' }

const ACTION_KEYS = {
  prepare: 'customNodes.action.preparing',
  apply: 'customNodes.action.applying',
  enable: 'customNodes.action.enabling',
  disable: 'customNodes.action.disabling',
  uninstall: 'customNodes.action.uninstalling',
  stop: 'customNodes.action.stopping',
  restart: 'customNodes.action.restarting',
} as const satisfies Record<ActionContext['kind'], NodeTextKey>

export function actionContextLabel(context: ActionContext): string {
  const name = context.kind === 'prepare' ? preparationLabel(context.preparation)
    : 'name' in context ? context.name : ''
  return t(ACTION_KEYS[context.kind], { name })
}

/** Lists longer than this render only the visible window. */
export const VIRTUALIZE_AFTER = 200

/** The backend accepts 1-128 reviewed plans per batch. */
export const MAX_BATCH = 128

export function sourceLabel(source: NodeSourceDto): string {
  switch (source.kind) {
    case 'registry': return `${source.id}@${source.version}`
    case 'git': return `${source.repository}@${source.commit}`
    case 'manual': return t('customNodes.manualSource')
  }
}

/** Identifies the exact reviewed plan: a changed binding is a different plan. */
export function reviewKey(plan: PlanReviewDto): string {
  return JSON.stringify([plan.id, plan.binding])
}

/**
 * Remembered trust is keyed to a Git repository, or to a Registry ID together with its publisher
 * and repository. Without that namespace the backend has nothing to save the trust under.
 */
export function hasTrustNamespace(source: NodeSourceDto): boolean {
  switch (source.kind) {
    case 'git': return source.repository.trim() !== ''
    case 'registry':
      return source.id.trim() !== '' &&
        (source.publisher?.trim() ?? '') !== '' &&
        (source.repository?.trim() ?? '') !== ''
    case 'manual': return false
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

/** Clears every selection that the plan does not offer, or that a parent selection no longer allows. */
export function normalizeConsent(plan: PlanReviewDto, consent: Consent): Consent {
  const namespace = hasTrustNamespace(plan.source)
  const hook = plan.hasInstallPy && consent.hook
  const remember = namespace && consent.remember
  return {
    execute: consent.execute,
    hook,
    warnings: plan.warnings.length > 0 && consent.warnings,
    pins: plan.protectedChanges.length > 0 && consent.pins,
    remember,
    rememberHook: hook && remember && consent.rememberHook,
  }
}

/** Remembered trust never substitutes for the per-plan execution consent. */
export function canCommit(plan: PlanReviewDto, consent: Consent): boolean {
  const value = normalizeConsent(plan, consent)
  return value.execute &&
    (plan.warnings.length === 0 || value.warnings) &&
    (plan.protectedChanges.length === 0 || value.pins)
}

export function decision(plan: PlanReviewDto, consent: Consent): CommitInput {
  const value = normalizeConsent(plan, consent)
  return {
    planId: plan.id,
    binding: plan.binding,
    executeNodeCode: value.execute,
    runInstallPy: value.hook,
    acknowledgeWarnings: value.warnings,
    rememberSource: value.remember,
    rememberInstallPy: value.rememberHook,
    confirmedChanges: value.pins ? plan.protectedChanges : [],
  }
}

/** Why an update cannot start, or null. Backend preparation rechecks everything. */
export function updateUnavailableKey(
  node: InstalledNodeDto,
  repairRequired: boolean,
): NodeTextKey | null {
  if (repairRequired) return 'customNodes.reason.repairFirst'
  if (node.singleFile) return 'customNodes.reason.singleFileUpdate'
  if (node.source.kind === 'manual') return 'customNodes.reason.manualUpdate'
  if (node.conflict) return 'customNodes.reason.diskConflict'
  if (node.dirty === true) return 'customNodes.reason.localChanges'
  return null
}

export function updateUnavailableReason(node: InstalledNodeDto, repairRequired: boolean): string | null {
  const key = updateUnavailableKey(node, repairRequired)
  return key === null ? null : t(key)
}

export function retryUnavailableReason(node: InstalledNodeDto): string | null {
  const key = retryUnavailableKey(node)
  return key === null ? null : t(key)
}

/** Unknown Git health (`dirty === null`) stays a candidate: status inspection decides. */
export function isUpdateCandidate(node: InstalledNodeDto): boolean {
  return updateUnavailableKey(node, false) === null
}

export function retryUnavailableKey(node: InstalledNodeDto): NodeTextKey | null {
  if (node.singleFile) return 'customNodes.reason.singleFileDependencies'
  if (node.conflict) return 'customNodes.reason.diskConflict'
  if (node.dependencies === 'ok') return 'customNodes.reason.dependenciesInstalled'
  return null
}

/** Directory nodes whose dependencies failed or never ran, including manual directories. */
export function canRetryDependencies(node: InstalledNodeDto): boolean {
  return retryUnavailableKey(node) === null
}

export type NodePreparation =
  | { kind: 'registry'; id: string; version: string | null }
  | { kind: 'git'; repository: string; reference: string | null }
  | { kind: 'update'; nodeId: string; reference: string | null }
  | { kind: 'retry'; nodeId: string }

export function preparationLabel(preparation: NodePreparation): string {
  switch (preparation.kind) {
    case 'registry': return preparation.id
    case 'git': return preparation.repository
    case 'update': return t('customNodes.preparation.update')
    case 'retry': return t('customNodes.preparation.retry')
  }
}

export function newRequestId(): string {
  return crypto.randomUUID()
}

export function chunk<T>(items: readonly T[], size: number): T[][] {
  const groups: T[][] = []
  for (let index = 0; index < items.length; index += size) {
    groups.push(items.slice(index, index + size))
  }
  return groups
}

/** A phase after which nothing more changes. `cleanup_pending` and `verified` are not final. */
export function isTerminalPhase(phase: string): boolean {
  return phase === 'completed' || phase === 'failed'
}

export type SourceFilter = 'all' | 'registry' | 'git' | 'manual'
export type EnabledFilter = 'all' | 'enabled' | 'disabled'

export interface NodeFilters {
  text: string
  source: SourceFilter
  enabled: EnabledFilter
}

/** Disabled nodes are included by default. */
export const DEFAULT_NODE_FILTERS: NodeFilters = {
  text: '', source: 'all', enabled: 'all',
}

export function filterNodes(
  nodes: readonly InstalledNodeDto[],
  filters: NodeFilters,
): InstalledNodeDto[] {
  const needle = filters.text.trim().toLowerCase()
  return nodes
    .filter((node) => {
      if (filters.source !== 'all' && node.source.kind !== filters.source) return false
      if (filters.enabled === 'enabled' && !node.enabled) return false
      if (filters.enabled === 'disabled' && node.enabled) return false
      if (needle === '') return true
      return node.name.toLowerCase().includes(needle) ||
        node.id.toLowerCase().includes(needle) ||
        sourceLabel(node.source).toLowerCase().includes(needle)
    })
    .sort((a, b) => a.name.localeCompare(b.name, undefined, { numeric: true }) ||
      a.id.localeCompare(b.id))
}

/** Keeps the active row when it is still listed; otherwise falls back to the first row. */
export function repairActive(
  nodes: readonly InstalledNodeDto[],
  activeId: string | null,
): string | null {
  if (activeId !== null && nodes.some((node) => node.id === activeId)) return activeId
  return nodes[0]?.id ?? null
}

export function nodeVersion(node: InstalledNodeDto): string {
  switch (node.source.kind) {
    case 'registry': return node.source.version
    case 'git': return node.source.commit.slice(0, 10)
    case 'manual': return '—'
  }
}

export function nodeBranch(node: InstalledNodeDto): string {
  return node.source.kind === 'git' ? node.source.branch ?? t('customNodes.detached') : '—'
}

export type GitHealth = 'clean' | 'dirty' | 'unknown' | 'not_git'

export function gitHealth(node: InstalledNodeDto): GitHealth {
  if (node.source.kind !== 'git') return 'not_git'
  if (node.dirty === null) return 'unknown'
  return node.dirty ? 'dirty' : 'clean'
}

export function repositoryOf(node: InstalledNodeDto): string | null {
  switch (node.source.kind) {
    case 'git': return node.source.repository
    case 'registry': return node.source.repository
    case 'manual': return null
  }
}

export interface GitCheck {
  /** Reasons the repository must not be updated; the backend refuses it as well. */
  refusals: string[]
  /** Detached or upstream-less: the update needs an explicit branch, tag or commit. */
  needsReference: boolean
}

export function checkGitStatus(status: NodeGitStatusDto): GitCheck {
  const refusals = gitRefusalKeys(status).map((key) => t(key))
  return {
    refusals,
    needsReference: status.branch === null || !status.hasUpstream,
  }
}

export function gitRefusalKeys(status: NodeGitStatusDto): NodeTextKey[] {
  const refusals: NodeTextKey[] = []
  if (status.dirty) refusals.push('customNodes.reason.localChanges')
  if (status.unfinishedOperation) {
    refusals.push('customNodes.reason.unfinishedGit')
  }
  if ((status.ahead ?? 0) > 0 && (status.behind ?? 0) > 0) {
    refusals.push('customNodes.reason.diverged')
  }
  return refusals
}

export const NEEDS_TARGET_SKIP = 'customNodes.reason.needsTarget'

export function formatCount(value: number | null): string {
  return value === null ? '—' : new Intl.NumberFormat('en-US').format(value)
}

export function unknownText(value: string | null | undefined): string {
  return value === null || value === undefined || value.trim() === '' ? t('common.unknownValue') : value
}

export function formatList(values: readonly string[] | null): string {
  return values === null || values.length === 0 ? t('common.unknownValue') : values.join(', ')
}

export function formatFetched(value: string): string {
  const parsed = new Date(value)
  return Number.isNaN(parsed.getTime()) ? value : parsed.toLocaleString()
}

/** `Recycle Bin` on Windows, `Trash` elsewhere. */
export function trashName(): string {
  return typeof navigator !== 'undefined' && /windows/i.test(navigator.userAgent)
    ? t('customNodes.recycleBin')
    : t('customNodes.trash')
}

export type FailureKind =
  | 'comfyui_running'
  | 'busy'
  | 'stale'
  | 'consent'
  | 'git_refused'
  | 'git_unavailable'
  | 'up_to_date'
  | 'dependencies'
  | 'recovery'
  | 'registry_refused'
  | 'refusal'
  | 'uncertain'
  | 'cancelled'
  | 'other'

export function failureKind(code: string): FailureKind {
  switch (code) {
    case 'comfyui_running': return 'comfyui_running'
    case 'maintenance_busy':
    case 'node_operation_running': return 'busy'
    case 'node_plan_stale': return 'stale'
    case 'node_consent_required': return 'consent'
    case 'node_git_refused': return 'git_refused'
    case 'node_git_unavailable': return 'git_unavailable'
    case 'node_up_to_date': return 'up_to_date'
    case 'node_dependencies_failed':
    case 'pin_drift':
    case 'environment_not_usable': return 'dependencies'
    case 'node_recovery_required':
    case 'node_rescan_failed': return 'recovery'
    case 'registry_refused': return 'registry_refused'
    case 'node_resolution_failed':
    case 'node_unsafe_source':
    case 'node_target_exists':
    case 'node_ambiguous_reference': return 'refusal'
    case 'transport': return 'uncertain'
    case 'cancelled': return 'cancelled'
    default: return 'other'
  }
}

export const FAILURE_GUIDANCE = {
  comfyui_running: 'customNodes.guidance.comfyuiRunning',
  busy: 'customNodes.guidance.busy',
  stale: 'customNodes.guidance.stale',
  consent: 'customNodes.guidance.consent',
  git_refused: 'customNodes.guidance.gitRefused',
  git_unavailable: 'customNodes.guidance.gitUnavailable',
  up_to_date: 'customNodes.guidance.upToDate',
  dependencies: 'customNodes.guidance.dependencies',
  recovery: 'customNodes.guidance.recovery',
  registry_refused: 'customNodes.guidance.registryRefused',
  refusal: 'customNodes.guidance.refusal',
  uncertain: 'customNodes.guidance.uncertain',
  cancelled: 'customNodes.guidance.cancelled',
  other: null,
} as const satisfies Record<FailureKind, NodeTextKey | null>
