import { Ban, Check, CircleX, Clock, LoaderCircle, Square, Trash2, type LucideIcon } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'

import {
  gateway,
  IpcClientError,
  type CommitInput,
  type InstalledNodeDto,
  type PlanReviewDto,
} from '@/shared/ipc/gateway'
import { errorMessage } from '@/shared/lib/display'
import { Alert, AlertDescription, AlertTitle } from '@/shared/ui/alert'
import { Button } from '@/shared/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shared/ui/dialog'

import {
  checkGitStatus,
  chunk,
  failureKind,
  MAX_BATCH,
  NEEDS_TARGET_SKIP,
  newRequestId,
  updateUnavailableReason,
} from './model'
import { ReviewDialog } from './review-dialog'
import { Chip } from './status-chips'

/** The cancellation key of a batch execution. */
export const UPDATE_ALL_KEY = 'update-all'

type Entry =
  | { state: 'pending' }
  | { state: 'preparing' }
  | { state: 'prepared' }
  | { state: 'applying' }
  | { state: 'skipped'; message: string }
  | { state: 'prepare_failed'; message: string }
  | { state: 'updated'; cleanupPending: boolean }
  | { state: 'up_to_date' }
  | { state: 'failed'; message: string }
  | { state: 'cancelled' }
  | { state: 'discarded' }
  | { state: 'not_run' }

type Stage = 'intro' | 'preparing' | 'review' | 'result'

const MUTED = 'bg-muted text-muted-foreground border-border'
const CHIPS: Record<Entry['state'], { label: string; icon: LucideIcon; className: string; spin?: boolean }> = {
  pending: { label: 'Waiting', icon: Clock, className: MUTED },
  preparing: { label: 'Preparing', icon: LoaderCircle, className: 'border-brand-cyan bg-transparent text-foreground', spin: true },
  prepared: { label: 'Prepared', icon: Clock, className: MUTED },
  applying: { label: 'Applying', icon: LoaderCircle, className: 'border-brand-cyan bg-transparent text-foreground', spin: true },
  skipped: { label: 'Skipped', icon: Ban, className: MUTED },
  prepare_failed: { label: 'Preparation failed', icon: CircleX, className: 'bg-destructive text-white' },
  updated: { label: 'Updated', icon: Check, className: 'bg-brand-yellow' },
  up_to_date: { label: 'Up to date', icon: Check, className: MUTED },
  failed: { label: 'Failed', icon: CircleX, className: 'bg-destructive text-white' },
  cancelled: { label: 'Cancelled', icon: Square, className: 'bg-brand-pink' },
  discarded: { label: 'Discarded', icon: Trash2, className: MUTED },
  not_run: { label: 'Not run', icon: Clock, className: MUTED },
}

/** Failures that would refuse every remaining node the same way. */
const GLOBAL_REFUSALS = new Set([
  'comfyui_running',
  'maintenance_busy',
  'node_operation_running',
  'node_recovery_required',
  'environment_not_usable',
  'transport',
])

/** Refused before anything ran: the review stays open so the same batch can be applied again. */
const BEFORE_RUN = new Set([
  'comfyui_running',
  'maintenance_busy',
  'node_operation_running',
  'node_consent_required',
])

const RETRYABLE: readonly Entry['state'][] = ['prepare_failed', 'failed', 'not_run', 'cancelled', 'discarded']

function ipc(error: unknown): { code: string; message: string } {
  return error instanceof IpcClientError
    ? { code: error.code, message: error.message }
    : { code: 'other', message: errorMessage(error) }
}

function describe(entry: Entry): string | null {
  switch (entry.state) {
    case 'skipped':
    case 'prepare_failed':
    case 'failed': return entry.message
    case 'updated': return entry.cleanupPending
      ? 'A retained backup could not be moved to the Recycle Bin or Trash yet; cleanup is retried on the next rescan.'
      : null
    case 'up_to_date': return 'Already up to date. Nothing was changed.'
    case 'cancelled': return 'Cancellation was requested. Check health; Repair may still be needed.'
    case 'discarded': return 'The reviewed plan was discarded without applying it.'
    case 'not_run': return 'Not started. Prepare it again to review a fresh plan.'
    default: return null
  }
}

/**
 * Prepares each eligible node one at a time, shows every exact plan in the shared review, and
 * applies it through one batch command. More than 128 targets run as successive reviewed groups.
 * A partially executed batch is never replayed: failed and unrun targets are prepared again.
 */
export function UpdateAllDialog({ nodes, onStopComfy, onSettled, onClose }: {
  nodes: readonly InstalledNodeDto[]
  onStopComfy: () => Promise<void>
  /** Backend state may have changed; `updated` counts the nodes updated so far. */
  onSettled: (updated: number) => void
  onClose: () => void
}) {
  const [stage, setStage] = useState<Stage>('intro')
  const [entries, setEntries] = useState<Record<string, Entry>>({})
  const [plans, setPlans] = useState<PlanReviewDto[]>([])
  const [applying, setApplying] = useState(false)
  const [progress, setProgress] = useState('')
  const [stopReason, setStopReason] = useState<string | null>(null)
  const [refusal, setRefusal] = useState<{ code: string; message: string } | null>(null)
  const [stoppingComfy, setStoppingComfy] = useState(false)
  const alive = useRef(true)
  const stopRequested = useRef(false)
  const userCancelled = useRef(false)
  const requestId = useRef<string | null>(null)
  const planNodes = useRef(new Map<string, string>())
  const decided = useRef<(() => void) | null>(null)
  const updated = useRef(0)

  useEffect(() => {
    alive.current = true
    return () => { alive.current = false }
  }, [])

  const patch = (id: string, entry: Entry) =>
    setEntries((current) => ({ ...current, [id]: entry }))
  const markNotRun = (targets: readonly InstalledNodeDto[]) =>
    setEntries((current) => {
      const next = { ...current }
      for (const node of targets) {
        const state = next[node.id]?.state
        if (state === 'pending' || state === 'preparing' || state === 'prepared') {
          next[node.id] = { state: 'not_run' }
        }
      }
      return next
    })

  async function prepareGroup(group: readonly InstalledNodeDto[]): Promise<PlanReviewDto[]> {
    const prepared: PlanReviewDto[] = []
    setStage('preparing')
    for (let index = 0; index < group.length; index += 1) {
      const node = group[index]
      if (stopRequested.current || !alive.current) {
        markNotRun(group.slice(index))
        break
      }
      setProgress(`Preparing ${index + 1} of ${group.length}: ${node.name}`)
      patch(node.id, { state: 'preparing' })
      if (node.source.kind === 'git') {
        try {
          const check = checkGitStatus(await gateway.nodeGitStatus(node.id))
          if (check.refusals.length > 0) {
            patch(node.id, { state: 'skipped', message: check.refusals.join(' ') })
            continue
          }
          if (check.needsReference) {
            patch(node.id, { state: 'skipped', message: NEEDS_TARGET_SKIP })
            continue
          }
        } catch (error) {
          const failure = ipc(error)
          patch(node.id, { state: 'prepare_failed', message: failure.message })
          if (GLOBAL_REFUSALS.has(failure.code)) {
            stopRequested.current = true
            setStopReason(failure.message)
          }
          continue
        }
      }
      const id = newRequestId()
      requestId.current = id
      try {
        const plan = await gateway.nodePrepareUpdate({ requestId: id, nodeId: node.id, reference: null })
        if (stopRequested.current) {
          // An explicit cancellation raced with a prepared plan: discard what came back.
          await gateway.nodeCancel(plan.id).catch(() => undefined)
          patch(node.id, { state: 'cancelled' })
          continue
        }
        planNodes.current.set(plan.id, node.id)
        prepared.push(plan)
        patch(node.id, { state: 'prepared' })
      } catch (error) {
        const failure = ipc(error)
        if (failure.code === 'node_up_to_date') {
          patch(node.id, { state: 'up_to_date' })
        } else if (failure.code === 'cancelled') {
          patch(node.id, { state: 'cancelled' })
          stopRequested.current = true
        } else {
          patch(node.id, { state: 'prepare_failed', message: failure.message })
          if (GLOBAL_REFUSALS.has(failure.code)) {
            stopRequested.current = true
            setStopReason(failure.message)
          }
        }
      } finally {
        requestId.current = null
      }
    }
    return prepared
  }

  function review(group: PlanReviewDto[]): Promise<void> {
    return new Promise((resolve) => {
      decided.current = resolve
      setRefusal(null)
      setPlans(group)
      setStage('review')
    })
  }

  async function start(targets: readonly InstalledNodeDto[]) {
    stopRequested.current = false
    userCancelled.current = false
    setStopReason(null)
    const eligible: InstalledNodeDto[] = []
    const initial: Record<string, Entry> = {}
    for (const node of targets) {
      const reason = updateUnavailableReason(node, false)
      if (reason !== null) {
        initial[node.id] = { state: 'skipped', message: reason }
      } else {
        initial[node.id] = { state: 'pending' }
        eligible.push(node)
      }
    }
    setEntries((current) => ({ ...current, ...initial }))
    const groups = chunk(eligible, MAX_BATCH)
    for (let index = 0; index < groups.length; index += 1) {
      const prepared = await prepareGroup(groups[index])
      if (!alive.current) return
      if (userCancelled.current) await discardPlans(prepared)
      else if (prepared.length > 0) await review(prepared)
      if (!alive.current) return
      if (stopRequested.current) {
        markNotRun(groups.slice(index + 1).flat())
        break
      }
    }
    setStage('result')
    onSettled(updated.current)
  }

  function cancelPreparation() {
    stopRequested.current = true
    userCancelled.current = true
    setProgress('Cancellation requested. Waiting for the current step to stop…')
    if (requestId.current !== null) {
      void gateway.nodeCancel(requestId.current).catch(() => undefined)
    }
  }

  function requestCancelApply() {
    stopRequested.current = true
    void gateway.nodeCancel(UPDATE_ALL_KEY).catch(() => undefined)
  }

  async function commit(items: CommitInput[]) {
    setApplying(true)
    setRefusal(null)
    for (const plan of plans) {
      const id = planNodes.current.get(plan.id)
      if (id) patch(id, { state: 'applying' })
    }
    try {
      const batch = await gateway.nodeUpdateAll(items)
      const seen = new Set<string>()
      for (const item of batch.items) {
        const id = planNodes.current.get(item.planId)
        if (!id) continue
        seen.add(item.planId)
        if (item.outcome) {
          updated.current += 1
          patch(id, { state: 'updated', cleanupPending: item.outcome.cleanupPending })
        } else if (item.error?.code === 'node_up_to_date') {
          patch(id, { state: 'up_to_date' })
        } else if (item.error?.code === 'cancelled') {
          patch(id, { state: 'cancelled' })
          stopRequested.current = true
        } else {
          patch(id, { state: 'failed', message: item.error?.message ?? 'The update did not complete.' })
        }
      }
      for (const plan of plans) {
        const id = planNodes.current.get(plan.id)
        if (id && !seen.has(plan.id)) patch(id, { state: 'not_run' })
      }
      if (batch.stoppedForRepair) {
        stopRequested.current = true
        setStopReason('The environment now needs repair, so no further groups were started.')
      }
      setPlans([])
      setApplying(false)
      decided.current?.()
    } catch (error) {
      const failure = ipc(error)
      setApplying(false)
      if (BEFORE_RUN.has(failure.code)) {
        for (const plan of plans) {
          const id = planNodes.current.get(plan.id)
          if (id) patch(id, { state: 'prepared' })
        }
        setRefusal(failure)
        return
      }
      // Anything else leaves the outcome uncertain: never replay, prepare again after refreshing.
      for (const plan of plans) {
        const id = planNodes.current.get(plan.id)
        if (id) patch(id, { state: 'failed', message: `${failure.message} The outcome is uncertain; refresh before preparing again.` })
      }
      stopRequested.current = true
      setStopReason(failure.message)
      setPlans([])
      decided.current?.()
    }
  }

  /** Explicit discard of reviewed plans: nothing was applied, so the run ends here. */
  async function discardPlans(current: readonly PlanReviewDto[]) {
    await Promise.all(current.map((plan) => gateway.nodeCancel(plan.id).catch(() => undefined)))
    for (const plan of current) {
      const id = planNodes.current.get(plan.id)
      if (id) patch(id, { state: 'discarded' })
    }
    stopRequested.current = true
  }

  async function discard() {
    const current = plans
    setPlans([])
    await discardPlans(current)
    decided.current?.()
  }

  async function stopComfy() {
    setStoppingComfy(true)
    try {
      await onStopComfy()
      setRefusal(null)
    } catch (error) {
      setRefusal(ipc(error))
    } finally {
      setStoppingComfy(false)
    }
  }

  const eligibleCount = nodes.filter((node) => updateUnavailableReason(node, false) === null).length
  const retryTargets = nodes.filter((node) => {
    const state = entries[node.id]?.state
    return state !== undefined && RETRYABLE.includes(state)
  })

  if (stage === 'review') {
    const notice = refusal && (
      <Alert variant="destructive">
        <AlertTitle>The batch was not applied</AlertTitle>
        <AlertDescription>
          <p>{refusal.message}</p>
          {failureKind(refusal.code) === 'comfyui_running' && (
            <Button size="sm" disabled={stoppingComfy} onClick={() => void stopComfy()}>
              {stoppingComfy ? 'Stopping ComfyUI…' : 'Stop ComfyUI'}
            </Button>
          )}
          <p>Nothing ran. Apply the reviewed plans again when you are ready.</p>
        </AlertDescription>
      </Alert>
    )
    return (
      <ReviewDialog
        plans={plans}
        busy={applying}
        notice={notice}
        onCommit={(items) => void commit(items)}
        onClose={() => void discard()}
        onCancel={applying ? requestCancelApply : null}
      />
    )
  }

  const counts = Object.values(entries).reduce<Record<string, number>>((total, entry) => {
    total[entry.state] = (total[entry.state] ?? 0) + 1
    return total
  }, {})

  return (
    <Dialog open onOpenChange={(open) => { if (!open && stage !== 'preparing') onClose() }}>
      <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-2xl" showCloseButton={stage !== 'preparing'}>
        <DialogHeader>
          <DialogTitle>{stage === 'result' ? 'Update results' : 'Update nodes'}</DialogTitle>
          <DialogDescription>
            {stage === 'intro' && `${eligibleCount} of ${nodes.length} selected nodes can be updated. Each plan is prepared one at a time and reviewed before anything changes.${eligibleCount > MAX_BATCH ? ` They run as reviewed groups of at most ${MAX_BATCH}.` : ''}`}
            {stage === 'preparing' && progress}
            {stage === 'result' && 'Each result below is final for this run. Nothing is replayed automatically.'}
          </DialogDescription>
        </DialogHeader>
        {stage === 'result' && stopReason && (
          <Alert>
            <AlertTitle>The run stopped early</AlertTitle>
            <AlertDescription>{stopReason}</AlertDescription>
          </Alert>
        )}
        {stage !== 'intro' && (
          <ul aria-label="Update results" className="flex max-h-80 flex-col gap-2 overflow-y-auto">
            {nodes.map((node) => {
              const entry = entries[node.id]
              if (!entry) return null
              const chip = CHIPS[entry.state]
              const note = describe(entry)
              return (
                <li key={node.id} className="flex flex-col gap-1 rounded-lg border p-2">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="min-w-0 font-medium break-words [overflow-wrap:anywhere]">{node.name}</span>
                    <Chip icon={chip.icon} className={chip.className} spin={chip.spin}>{chip.label}</Chip>
                  </div>
                  {note && <p className="text-xs break-words text-muted-foreground [overflow-wrap:anywhere]">{note}</p>}
                </li>
              )
            })}
          </ul>
        )}
        {stage === 'result' && (
          <p className="font-mono text-xs text-muted-foreground">
            {counts.updated ?? 0} updated · {counts.up_to_date ?? 0} up to date · {counts.skipped ?? 0} skipped ·{' '}
            {(counts.prepare_failed ?? 0) + (counts.failed ?? 0)} failed · {counts.not_run ?? 0} not run
          </p>
        )}
        <DialogFooter>
          {stage === 'intro' && <>
            <Button variant="outline" onClick={onClose}>Cancel</Button>
            <Button disabled={eligibleCount === 0} onClick={() => void start(nodes)}>
              Prepare {eligibleCount} {eligibleCount === 1 ? 'update' : 'updates'}
            </Button>
          </>}
          {stage === 'preparing' &&
            <Button variant="outline" onClick={cancelPreparation}>Cancel preparation</Button>}
          {stage === 'result' && <>
            {retryTargets.length > 0 && (
              <Button variant="outline" onClick={() => void start(retryTargets)}>
                Prepare failed and unrun nodes again ({retryTargets.length})
              </Button>
            )}
            <Button onClick={onClose}>Close</Button>
          </>}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
