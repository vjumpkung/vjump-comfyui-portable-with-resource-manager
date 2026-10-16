import type {
  CommitInput,
  InstalledNodeDto,
  NodeGitStatusDto,
  NodeSourceDto,
  PlanReviewDto,
} from '@/shared/ipc/gateway'

/** Lists longer than this render only the visible window. */
export const VIRTUALIZE_AFTER = 200

/** The backend accepts 1-128 reviewed plans per batch. */
export const MAX_BATCH = 128

export function sourceLabel(source: NodeSourceDto): string {
  switch (source.kind) {
    case 'registry': return `${source.id}@${source.version}`
    case 'git': return `${source.repository}@${source.commit}`
    case 'manual': return 'Manual / unmanaged'
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
export function updateUnavailableReason(
  node: InstalledNodeDto,
  repairRequired: boolean,
): string | null {
  if (repairRequired) return 'Updates are disabled until the environment is repaired.'
  if (node.singleFile) return 'Single-file nodes have no source to update from.'
  if (node.source.kind === 'manual') return 'Manually installed nodes have no source to update from.'
  if (node.conflict) return 'The files on disk disagree with the stored record.'
  if (node.dirty === true) return 'The repository has local changes.'
  return null
}

/** Unknown Git health (`dirty === null`) stays a candidate: status inspection decides. */
export function isUpdateCandidate(node: InstalledNodeDto): boolean {
  return updateUnavailableReason(node, false) === null
}

export function retryUnavailableReason(node: InstalledNodeDto): string | null {
  if (node.singleFile) return 'Single-file nodes have no dependencies to install.'
  if (node.conflict) return 'The files on disk disagree with the stored record.'
  if (node.dependencies === 'ok') return 'The dependencies are already installed.'
  return null
}

/** Directory nodes whose dependencies failed or never ran, including manual directories. */
export function canRetryDependencies(node: InstalledNodeDto): boolean {
  return retryUnavailableReason(node) === null
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
    case 'update': return 'the update'
    case 'retry': return 'the dependency retry'
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
  return node.source.kind === 'git' ? node.source.branch ?? 'detached' : '—'
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
  const refusals: string[] = []
  if (status.dirty) refusals.push('The repository has local changes.')
  if (status.unfinishedOperation) {
    refusals.push('A merge, rebase or other Git operation is unfinished.')
  }
  if ((status.ahead ?? 0) > 0 && (status.behind ?? 0) > 0) {
    refusals.push('The branch has diverged from its upstream.')
  }
  return {
    refusals,
    needsReference: status.branch === null || !status.hasUpstream,
  }
}

export const NEEDS_TARGET_SKIP = 'Update individually with an explicit target.'

export function formatCount(value: number | null): string {
  return value === null ? '—' : new Intl.NumberFormat('en-US').format(value)
}

export function unknownText(value: string | null | undefined): string {
  return value === null || value === undefined || value.trim() === '' ? 'Unknown' : value
}

export function formatList(values: readonly string[] | null): string {
  return values === null || values.length === 0 ? 'Unknown' : values.join(', ')
}

export function formatFetched(value: string): string {
  const parsed = new Date(value)
  return Number.isNaN(parsed.getTime()) ? value : parsed.toLocaleString()
}

/** `Recycle Bin` on Windows, `Trash` elsewhere. */
export function trashName(): string {
  return typeof navigator !== 'undefined' && /windows/i.test(navigator.userAgent)
    ? 'Recycle Bin'
    : 'Trash'
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

export const FAILURE_GUIDANCE: Record<FailureKind, string> = {
  comfyui_running: 'ComfyUI is running and holds the node folders. Stop it, then the request is repeated.',
  busy: 'Another maintenance or custom node operation is running. Wait for it to finish, then try again. Nothing else was stopped.',
  stale: 'The reviewed plan is no longer current and its confirmations were discarded. Prepare it again to review the new plan.',
  consent: 'The confirmations do not cover this exact plan. Check the required boxes and apply again.',
  git_refused: 'The repository was not changed. Resolve local changes or divergence manually in the node folder, or update with an explicit target.',
  git_unavailable: 'Git is not available. On Windows, complete tool setup from the Environment page; on Linux, install Git and try again.',
  up_to_date: 'Nothing was changed.',
  dependencies: 'Dependency health was refreshed. Read the operation log, retry dependencies when the node is eligible, or repair the environment.',
  recovery: 'An interrupted node operation or a failed rescan must be resolved. Rescan to finish recovery.',
  registry_refused: 'The registry refused the request. This is not an offline cache; adjust the search or try again later.',
  refusal: 'Nothing was changed and there is no automatic override.',
  uncertain: 'The outcome is uncertain. State was refreshed; check the node list and operations before repeating the action.',
  cancelled: 'Cancellation was requested. Health and operation state were refreshed; a cancellation after dependency changes can still require Repair.',
  other: '',
}
