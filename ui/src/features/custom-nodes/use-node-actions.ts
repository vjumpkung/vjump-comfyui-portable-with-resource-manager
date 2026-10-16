import { useQueryClient } from '@tanstack/react-query'
import { useRef, useState } from 'react'

import {
  gateway,
  IpcClientError,
  type CommitInput,
  type InstalledNodeDto,
  type NodeOutcomeDto,
  type PlanReviewDto,
} from '@/shared/ipc/gateway'
import { errorMessage } from '@/shared/lib/display'

import {
  failureKind,
  newRequestId,
  preparationLabel,
  type FailureKind,
  type NodePreparation,
} from './model'
import { NODE_KEY, refreshNodeState } from './use-nodes'

/** What is running. Every kind but `direct` has a key that `nodeCancel` accepts. */
export type Busy =
  | { kind: 'prepare'; requestId: string; label: string }
  | { kind: 'commit'; planId: string; label: string }
  | { kind: 'direct'; label: string }

export interface ActionFailure {
  code: string
  message: string
  kind: FailureKind
  context: string
  /** Set for node-specific failures so the actual Git status can be shown. */
  nodeId: string | null
  /** Repeats exactly the refused request; null when repeating makes no sense. */
  retry: (() => Promise<void>) | null
  /** The original preparation, so a stale plan is prepared again from the same request. */
  prepare: NodePreparation | null
}

export interface ActionNotice {
  message: string
  /** A retained backup could not be moved to the Recycle Bin or Trash yet. */
  cleanupPending: boolean
}

function toError(error: unknown): IpcClientError {
  return error instanceof IpcClientError
    ? error
    : new IpcClientError('other', errorMessage(error))
}

export function prepareNode(preparation: NodePreparation, requestId: string): Promise<PlanReviewDto> {
  switch (preparation.kind) {
    case 'registry':
      return gateway.nodePrepareRegistry({ requestId, id: preparation.id, version: preparation.version })
    case 'git':
      return gateway.nodePrepareGit({ requestId, repository: preparation.repository, reference: preparation.reference })
    case 'update':
      return gateway.nodePrepareUpdate({ requestId, nodeId: preparation.nodeId, reference: preparation.reference })
    case 'retry':
      return gateway.nodePrepareRetryDeps({ requestId, nodeId: preparation.nodeId })
  }
}

const PLAN_VERB = {
  install: 'Installed',
  update: 'Updated',
  retry_dependencies: 'Retried the dependencies of',
  enable: 'Enabled',
  disable: 'Disabled',
  uninstall: 'Uninstalled',
} as const

/** A refusal before anything ran: the review can be kept and the same request repeated. */
const KEEP_REVIEW: readonly FailureKind[] = ['consent', 'comfyui_running', 'busy']

/**
 * Preparation, review, commit and direct node changes for the page. One request runs at a time.
 * Unmounting never cancels or discards backend work: only an explicit action does.
 */
export function useNodeActions({ stopComfy }: { stopComfy: () => Promise<void> }) {
  const client = useQueryClient()
  const [busy, setBusy] = useState<Busy | null>(null)
  const [cancelRequested, setCancelRequested] = useState(false)
  const [review, setReview] = useState<PlanReviewDto[] | null>(null)
  const [failure, setFailure] = useState<ActionFailure | null>(null)
  const [notice, setNotice] = useState<ActionNotice | null>(null)
  const [restartOffered, setRestartOffered] = useState(false)
  const [stopping, setStopping] = useState(false)
  const [rescanning, setRescanning] = useState(false)
  const [rescanError, setRescanError] = useState<unknown>(null)
  const inflight = useRef(false)
  const cancelled = useRef(false)
  const preparations = useRef(new Map<string, NodePreparation>())

  const settle = () => refreshNodeState(client)

  function fail(
    error: unknown,
    context: string,
    extra: Partial<Pick<ActionFailure, 'nodeId' | 'retry' | 'prepare'>> = {},
  ): ActionFailure {
    const ipc = toError(error)
    const value: ActionFailure = {
      code: ipc.code,
      message: ipc.message,
      kind: failureKind(ipc.code),
      context,
      nodeId: extra.nodeId ?? null,
      retry: extra.retry ?? null,
      prepare: extra.prepare ?? null,
    }
    setFailure(value)
    return value
  }

  function begin(state: Busy): boolean {
    if (inflight.current) return false
    inflight.current = true
    cancelled.current = false
    setCancelRequested(false)
    setBusy(state)
    setFailure(null)
    setNotice(null)
    setRestartOffered(false)
    return true
  }

  function end() {
    inflight.current = false
    setBusy(null)
    settle()
  }

  function succeed(message: string, outcome: NodeOutcomeDto | null) {
    setNotice({ message, cleanupPending: outcome?.cleanupPending ?? false })
    setRestartOffered(true)
  }

  async function prepare(preparation: NodePreparation): Promise<void> {
    const requestId = newRequestId()
    const label = preparationLabel(preparation)
    if (!begin({ kind: 'prepare', requestId, label })) return
    try {
      const plan = await prepareNode(preparation, requestId)
      if (cancelled.current) {
        // An explicit cancellation raced with a successful preparation: discard what came back.
        await gateway.nodeCancel(plan.id).catch(() => undefined)
        setNotice({ message: 'Preparation was cancelled and the prepared plan was discarded.', cleanupPending: false })
      } else {
        preparations.current.set(plan.id, preparation)
        setReview([plan])
      }
    } catch (error) {
      fail(error, `Preparing ${label}`, {
        nodeId: 'nodeId' in preparation ? preparation.nodeId : null,
        retry: () => prepare(preparation),
        prepare: preparation,
      })
    } finally {
      end()
    }
  }

  async function commit(items: CommitInput[]): Promise<void> {
    const item = items[0]
    const plan = review?.find((candidate) => candidate.id === item?.planId)
    if (!item || !plan) return
    if (!begin({ kind: 'commit', planId: plan.id, label: plan.name })) return
    try {
      const outcome = await gateway.nodeCommit(item)
      setReview(null)
      preparations.current.delete(plan.id)
      succeed(`${PLAN_VERB[plan.kind]} ${plan.name}.`, outcome)
    } catch (error) {
      const value = fail(error, `Applying ${plan.name}`, {
        retry: () => commit(items),
        prepare: preparations.current.get(plan.id) ?? null,
      })
      if (!KEEP_REVIEW.includes(value.kind)) setReview(null)
    } finally {
      end()
    }
  }

  /** Idle review only: explicitly discards every plan; never called on unmount. */
  async function discard(plans: PlanReviewDto[]): Promise<void> {
    setReview(null)
    await Promise.all(plans.map(async (plan) => {
      preparations.current.delete(plan.id)
      await gateway.nodeCancel(plan.id).catch(() => undefined)
    }))
    settle()
  }

  function resume(plan: PlanReviewDto) {
    setFailure(null)
    setNotice(null)
    setReview([plan])
  }

  /** Cancellation is a request: the command or status outcome is still awaited. */
  function requestCancel() {
    const key = busy?.kind === 'prepare' ? busy.requestId : busy?.kind === 'commit' ? busy.planId : null
    if (key === null) return
    cancelled.current = true
    setCancelRequested(true)
    void gateway.nodeCancel(key).catch(() => undefined)
  }

  async function direct(
    label: string,
    context: string,
    work: () => Promise<NodeOutcomeDto>,
    message: string,
    node: InstalledNodeDto,
  ): Promise<void> {
    if (!begin({ kind: 'direct', label })) return
    try {
      const outcome = await work()
      succeed(message, outcome)
    } catch (error) {
      fail(error, context, { nodeId: node.id, retry: () => direct(label, context, work, message, node) })
    } finally {
      end()
    }
  }

  function toggle(node: InstalledNodeDto): Promise<void> {
    const enabled = !node.enabled
    return direct(
      `${enabled ? 'Enabling' : 'Disabling'} ${node.name}`,
      `${enabled ? 'Enabling' : 'Disabling'} ${node.name}`,
      () => gateway.nodeSetEnabled({ nodeId: node.id, enabled }),
      `${enabled ? 'Enabled' : 'Disabled'} ${node.name}.`,
      node,
    )
  }

  function uninstall(node: InstalledNodeDto): Promise<void> {
    return direct(
      `Uninstalling ${node.name}`,
      `Uninstalling ${node.name}`,
      () => gateway.nodeUninstall(node.id),
      `Uninstalled ${node.name}.`,
      node,
    )
  }

  async function rescan(): Promise<void> {
    if (inflight.current || rescanning) return
    setRescanning(true)
    setRescanError(null)
    try {
      const list = await gateway.nodeRescan()
      client.setQueryData(NODE_KEY, list)
    } catch (error) {
      setRescanError(error)
    } finally {
      setRescanning(false)
      settle()
    }
  }

  /** Awaits the stop before repeating the refused request. */
  async function stopAndRetry(): Promise<void> {
    const retry = failure?.retry
    if (!retry || stopping) return
    setStopping(true)
    try {
      await stopComfy()
    } catch (error) {
      fail(error, 'Stopping ComfyUI')
      return
    } finally {
      setStopping(false)
    }
    await retry()
  }

  return {
    busy,
    cancelRequested,
    review,
    failure,
    notice,
    restartOffered,
    stopping,
    rescanning,
    rescanError,
    working: busy !== null || stopping || rescanning,
    prepare,
    commit,
    discard,
    resume,
    requestCancel,
    toggle,
    uninstall,
    rescan,
    stopAndRetry,
    report: fail,
    dismissFailure: () => setFailure(null),
    dismissNotice: () => setNotice(null),
    finishRestartOffer: () => setRestartOffered(false),
    noteResult: (message: string) => {
      setNotice({ message, cleanupPending: false })
      setRestartOffered(true)
    },
  }
}
