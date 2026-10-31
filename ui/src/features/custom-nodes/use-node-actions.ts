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

import {
  failureKind,
  newRequestId,
  type ActionContext,
  type FailureKind,
  type NodePreparation,
  type NodeText,
} from './model'
import { NODE_KEY, refreshNodeState } from './use-nodes'

/** What is running. Every kind but `direct` has a key that `nodeCancel` accepts. */
export type Busy =
  | { kind: 'prepare'; requestId: string; context: ActionContext }
  | { kind: 'commit'; planId: string; context: ActionContext }
  | { kind: 'direct'; context: ActionContext }

export interface ActionFailure {
  code: string
  message: string | NodeText
  kind: FailureKind
  context: ActionContext
  /** Set for node-specific failures so the actual Git status can be shown. */
  nodeId: string | null
  /** Repeats exactly the refused request; null when repeating makes no sense. */
  retry: (() => Promise<void>) | null
  /** The original preparation, so a stale plan is prepared again from the same request. */
  prepare: NodePreparation | null
}

export interface ActionNotice {
  message: NodeText | { count: number }
  /** A retained backup could not be moved to the Recycle Bin or Trash yet. */
  cleanupPending: boolean
}

function toError(error: unknown): { code: string; message: string | NodeText } {
  return error instanceof IpcClientError
    ? error
    : { code: 'other', message: { key: 'common.operationError' } }
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
  install: 'customNodes.result.installed',
  update: 'customNodes.result.updated',
  retry_dependencies: 'customNodes.result.retried',
  enable: 'customNodes.result.enabled',
  disable: 'customNodes.result.disabled',
  uninstall: 'customNodes.result.uninstalled',
} as const satisfies Record<PlanReviewDto['kind'], NodeText['key']>

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
  const [resolving, setResolving] = useState(false)
  const [resolveError, setResolveError] = useState<unknown>(null)
  const inflight = useRef(false)
  const cancelled = useRef(false)
  const preparations = useRef(new Map<string, NodePreparation>())

  const settle = () => refreshNodeState(client)

  function fail(
    error: unknown,
    context: ActionContext,
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

  function succeed(message: NodeText, outcome: NodeOutcomeDto | null) {
    setNotice({ message, cleanupPending: outcome?.cleanupPending ?? false })
    setRestartOffered(true)
  }

  async function prepare(preparation: NodePreparation): Promise<void> {
    const requestId = newRequestId()
    const context: ActionContext = { kind: 'prepare', preparation }
    if (!begin({ kind: 'prepare', requestId, context })) return
    try {
      const plan = await prepareNode(preparation, requestId)
      if (cancelled.current) {
        // An explicit cancellation raced with a successful preparation: discard what came back.
        await gateway.nodeCancel(plan.id).catch(() => undefined)
        setNotice({ message: { key: 'customNodes.result.preparationCancelled' }, cleanupPending: false })
      } else {
        preparations.current.set(plan.id, preparation)
        setReview([plan])
      }
    } catch (error) {
      fail(error, context, {
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
    const context: ActionContext = { kind: 'apply', name: plan.name }
    if (!begin({ kind: 'commit', planId: plan.id, context })) return
    try {
      const outcome = await gateway.nodeCommit(item)
      setReview(null)
      preparations.current.delete(plan.id)
      succeed({ key: PLAN_VERB[plan.kind], values: { name: plan.name } }, outcome)
    } catch (error) {
      const value = fail(error, context, {
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
    context: ActionContext,
    work: () => Promise<NodeOutcomeDto>,
    message: NodeText,
    node: InstalledNodeDto,
  ): Promise<void> {
    if (!begin({ kind: 'direct', context })) return
    try {
      const outcome = await work()
      succeed(message, outcome)
    } catch (error) {
      fail(error, context, { nodeId: node.id, retry: () => direct(context, work, message, node) })
    } finally {
      end()
    }
  }

  function toggle(node: InstalledNodeDto): Promise<void> {
    const enabled = !node.enabled
    return direct(
      { kind: enabled ? 'enable' : 'disable', name: node.name },
      () => gateway.nodeSetEnabled({ nodeId: node.id, enabled }),
      { key: enabled ? PLAN_VERB.enable : PLAN_VERB.disable, values: { name: node.name } },
      node,
    )
  }

  function uninstall(node: InstalledNodeDto): Promise<void> {
    return direct(
      { kind: 'uninstall', name: node.name },
      () => gateway.nodeUninstall(node.id),
      { key: PLAN_VERB.uninstall, values: { name: node.name } },
      node,
    )
  }

  async function rescan(): Promise<void> {
    if (inflight.current || rescanning || resolving) return
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

  async function resolveOperation(operationId: string): Promise<boolean> {
    if (inflight.current || rescanning || resolving) return false
    setResolving(true)
    setResolveError(null)
    try {
      const list = await gateway.nodeResolveOperation(operationId)
      client.setQueryData(NODE_KEY, list)
      setRescanError(null)
      return true
    } catch (error) {
      setResolveError(error)
      return false
    } finally {
      setResolving(false)
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
      fail(error, { kind: 'stop' })
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
    resolving,
    resolveError,
    working: busy !== null || stopping || rescanning || resolving,
    prepare,
    commit,
    discard,
    resume,
    requestCancel,
    toggle,
    uninstall,
    rescan,
    resolveOperation,
    stopAndRetry,
    report: fail,
    dismissFailure: () => setFailure(null),
    dismissNotice: () => setNotice(null),
    finishRestartOffer: () => setRestartOffered(false),
    noteResult: (count: number) => {
      setNotice({ message: { count }, cleanupPending: false })
      setRestartOffered(true)
    },
  }
}
