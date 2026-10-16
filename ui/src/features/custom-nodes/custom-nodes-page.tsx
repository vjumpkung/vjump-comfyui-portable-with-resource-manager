import { useQueryClient } from '@tanstack/react-query'
import { LoaderCircle, Puzzle, Search, TriangleAlert, Wrench } from 'lucide-react'
import { useMemo, useState } from 'react'

import type {
  ComfyStatusDto,
  InstalledNodeDto,
  StartResultDto,
} from '@/shared/ipc/gateway'
import { Alert, AlertDescription, AlertTitle } from '@/shared/ui/alert'
import { Button } from '@/shared/ui/button'
import { EmptyState } from '@/shared/ui/empty-state'
import { ErrorNotice } from '@/shared/ui/feedback'
import { Input } from '@/shared/ui/input'

import { ConfirmDialog, type NodeConfirmation } from './confirm-dialog'
import { FailureNotice } from './failure-notice'
import { InstallDialog, type InstallTarget } from './install-dialog'
import { InstalledTable } from './installed-table'
import {
  DEFAULT_NODE_FILTERS,
  filterNodes,
  isUpdateCandidate,
  repairActive,
  sourceLabel,
  type EnabledFilter,
  type NodeFilters,
  type SourceFilter,
} from './model'
import { NodeDetails } from './node-details'
import { OperationsPanel } from './operations-panel'
import { RegistryBrowser } from './registry-browser'
import { ReviewDialog } from './review-dialog'
import { UpdateAllDialog } from './update-all-dialog'
import { UpdateDialog } from './update-dialog'
import { useNodeActions } from './use-node-actions'
import { refreshNodeState, useNodes } from './use-nodes'

const SELECT_CLASS =
  'h-9 rounded-md border border-input bg-card px-2.5 text-sm outline-none focus-visible:border-brand-cyan focus-visible:ring-2 focus-visible:ring-brand-cyan/35'

export interface CustomNodesPageProps {
  /** An environment setup run holds the maintenance gate. */
  maintenanceBusy: boolean
  runtime: ComfyStatusDto | null
  runtimeBusy: boolean
  /** Opens the existing environment Repair flow. */
  onRepair: () => void
  onStopComfy: () => Promise<void>
  onRestartComfy: () => Promise<StartResultDto>
  /** Opens the ComfyUI page, where launch options are resolved. */
  onOpenComfy: () => void
}

type Restart =
  | { kind: 'idle' }
  | { kind: 'working' }
  | { kind: 'started' }
  | { kind: 'port_conflict'; requested: number; suggested: number | null }

export function CustomNodesPage({
  maintenanceBusy,
  runtime,
  runtimeBusy,
  onRepair,
  onStopComfy,
  onRestartComfy,
  onOpenComfy,
}: CustomNodesPageProps) {
  const client = useQueryClient()
  const nodes = useNodes()
  const actions = useNodeActions({ stopComfy: onStopComfy })
  const [filters, setFilters] = useState<NodeFilters>(DEFAULT_NODE_FILTERS)
  const [selected, setSelected] = useState<ReadonlySet<string>>(new Set())
  const [activeId, setActiveId] = useState<string | null>(null)
  const [registryOpen, setRegistryOpen] = useState(false)
  const [installTarget, setInstallTarget] = useState<InstallTarget | null>(null)
  const [updateNode, setUpdateNode] = useState<InstalledNodeDto | null>(null)
  const [updateAll, setUpdateAll] = useState<InstalledNodeDto[] | null>(null)
  const [confirm, setConfirm] = useState<{ node: InstalledNodeDto; action: NodeConfirmation } | null>(null)
  const [operationId, setOperationId] = useState<string | null>(null)
  const [restart, setRestart] = useState<Restart>({ kind: 'idle' })

  const list = nodes.data
  const all = useMemo(() => list?.nodes ?? [], [list])
  const visible = useMemo(() => filterNodes(all, filters), [all, filters])
  const currentId = repairActive(visible, activeId)
  const active = visible.find((node) => node.id === currentId) ?? null
  const selectedNodes = all.filter((node) => selected.has(node.id))

  const repairRequired = list?.repairRequired ?? false
  const pendingReviews = list?.pendingReviews ?? []
  const unresolved = list?.unresolved ?? []
  const blocksStart = unresolved.some((operation) => operation.blocksStart)
  const paused = maintenanceBusy || actions.working
  const lifecycle = runtime?.lifecycle
  const comfyRunning = lifecycle === 'running' || lifecycle === 'starting'
  const installDisabledReason = maintenanceBusy
    ? 'Environment setup is running.'
    : repairRequired
      ? 'Installs are disabled until the environment is repaired.'
      : actions.working
        ? 'Another request is running.'
        : null

  const updateScope = selectedNodes.length > 0 ? selectedNodes : all.filter(isUpdateCandidate)
  const updateCount = updateScope.filter(isUpdateCandidate).length
  const updateAllBlocked = repairRequired || paused || pendingReviews.length > 0 ||
    actions.review !== null || updateCount === 0
  const canRestart = list !== undefined && !repairRequired && !blocksStart &&
    !maintenanceBusy && !runtimeBusy && !actions.working

  const patchFilters = (change: Partial<NodeFilters>) =>
    setFilters((current) => ({ ...current, ...change }))
  const toggle = (id: string) =>
    setSelected((current) => {
      const next = new Set(current)
      if (!next.delete(id)) next.add(id)
      return next
    })
  const toggleAll = () => {
    const ids = visible.map((node) => node.id)
    setSelected((current) => ids.every((id) => current.has(id)) ? new Set() : new Set(ids))
  }

  async function stopRunning() {
    try {
      await onStopComfy()
    } catch (error) {
      actions.report(error, 'Stopping ComfyUI')
    }
  }

  async function restartComfy() {
    setRestart({ kind: 'working' })
    try {
      const result = await onRestartComfy()
      if (result.kind === 'started') {
        setRestart({ kind: 'started' })
      } else {
        // A conflict is never accepted silently: the launch options page resolves it.
        setRestart({ kind: 'port_conflict', requested: result.requested, suggested: result.suggested })
      }
    } catch (error) {
      setRestart({ kind: 'idle' })
      actions.report(error, 'Restarting ComfyUI', { retry: restartComfy })
    }
    refreshNodeState(client)
  }

  const failureNotice = actions.failure && (
    <FailureNotice
      failure={actions.failure}
      stopping={actions.stopping}
      onStopAndRetry={() => void actions.stopAndRetry()}
      onRetry={() => void actions.failure?.retry?.()}
      onPrepareAgain={(preparation) => void actions.prepare(preparation)}
      onRepair={onRepair}
      onRescan={() => void actions.rescan()}
      onDismiss={actions.dismissFailure}
    />
  )

  const offerRestart = actions.restartOffered || restart.kind !== 'idle'

  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h1 className="text-2xl font-extrabold tracking-tighter sm:text-3xl">Custom Nodes</h1>
          <p className="font-mono text-xs text-muted-foreground">
            {all.length} installed · {all.filter((node) => !node.enabled).length} disabled
            {selectedNodes.length > 0 ? ` · ${selectedNodes.length} selected` : ''}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button
            variant="outline"
            aria-pressed={registryOpen}
            onClick={() => setRegistryOpen((open) => !open)}
          >
            {registryOpen ? 'Hide Registry' : 'Browse Registry'}
          </Button>
          <Button
            variant="outline"
            disabled={installDisabledReason !== null}
            title={installDisabledReason ?? undefined}
            onClick={() => setInstallTarget({ kind: 'git' })}
          >
            Install from Git
          </Button>
          <Button variant="outline" disabled={updateAllBlocked} onClick={() => setUpdateAll(updateScope)}>
            {selectedNodes.length > 0 ? 'Update selected' : 'Update all'} ({updateCount})
          </Button>
        </div>
      </div>

      {maintenanceBusy && (
        <Alert>
          <LoaderCircle className="animate-spin" aria-hidden="true" />
          <AlertTitle>Environment setup is running</AlertTitle>
          <AlertDescription>Custom node changes are paused until it finishes.</AlertDescription>
        </Alert>
      )}
      {repairRequired && (
        <Alert variant="destructive">
          <Wrench aria-hidden="true" />
          <AlertTitle>This environment needs repair</AlertTitle>
          <AlertDescription>
            Installs and updates are disabled until Repair succeeds. Retrying dependencies,
            enabling, disabling and uninstalling stay available. A successful retry does not
            clear this notice by itself.
            <Button variant="outline" size="sm" onClick={onRepair}>Repair environment</Button>
          </AlertDescription>
        </Alert>
      )}
      {comfyRunning && (
        <Alert>
          <TriangleAlert aria-hidden="true" />
          <AlertTitle>ComfyUI is running</AlertTitle>
          <AlertDescription>
            Node changes need ComfyUI stopped. Stopping it also rescans the node folders.
            <Button variant="outline" size="sm" disabled={runtimeBusy} onClick={() => void stopRunning()}>
              Stop ComfyUI
            </Button>
          </AlertDescription>
        </Alert>
      )}

      {pendingReviews.length > 0 && !actions.review && updateAll === null && actions.busy === null && (
        <Alert>
          <AlertTitle>
            {pendingReviews.length === 1 ? 'A prepared plan is waiting for review' : `${pendingReviews.length} prepared plans are waiting for review`}
          </AlertTitle>
          <AlertDescription>
            Nothing runs until you confirm. Resuming starts with every confirmation unchecked.
            <ul className="mt-2 flex flex-col gap-2">
              {pendingReviews.map((plan) => (
                <li key={plan.id} className="flex flex-wrap items-center gap-2">
                  <span className="min-w-0 font-medium break-words [overflow-wrap:anywhere]">
                    {plan.name} · {sourceLabel(plan.source)}
                  </span>
                  <Button size="sm" disabled={paused} onClick={() => actions.resume(plan)}>
                    Resume review
                  </Button>
                  <Button variant="ghost" size="sm" disabled={paused} onClick={() => void actions.discard([plan])}>
                    Discard
                  </Button>
                </li>
              ))}
            </ul>
          </AlertDescription>
        </Alert>
      )}

      {actions.busy && (
        <Alert role="status">
          <LoaderCircle className="animate-spin" aria-hidden="true" />
          <AlertTitle>
            {actions.busy.kind === 'prepare' ? `Preparing ${actions.busy.label}…`
              : actions.busy.kind === 'commit' ? `Applying ${actions.busy.label}…`
                : `${actions.busy.label}…`}
          </AlertTitle>
          <AlertDescription>
            {actions.cancelRequested
              ? 'Cancellation requested. Waiting for the operation to stop.'
              : actions.busy.kind === 'direct'
                ? 'This step cannot be cancelled.'
                : 'Closing this page does not stop the work.'}
            {actions.busy.kind === 'prepare' && (
              <Button variant="outline" size="sm" disabled={actions.cancelRequested} onClick={actions.requestCancel}>
                Cancel preparation
              </Button>
            )}
          </AlertDescription>
        </Alert>
      )}
      {!actions.review && failureNotice}
      {actions.notice && (
        <Alert role="status">
          <AlertTitle>{actions.notice.message}</AlertTitle>
          {actions.notice.cleanupPending && (
            <AlertDescription>
              A retained backup could not be moved to the Recycle Bin or Trash yet. The change itself
              finished; cleanup is retried on the next rescan.
            </AlertDescription>
          )}
        </Alert>
      )}
      {offerRestart && (
        <Alert role="status">
          <AlertTitle>
            {restart.kind === 'started' ? 'ComfyUI was restarted'
              : restart.kind === 'port_conflict' ? `Port ${restart.requested} is in use, so ComfyUI was not started`
                : restart.kind === 'working' ? 'Restarting ComfyUI…'
                  : 'Restart ComfyUI to load the changes'}
          </AlertTitle>
          <AlertDescription>
            {restart.kind === 'port_conflict' && (
              <>
                {restart.suggested !== null ? `Port ${restart.suggested} is free, but it is not used automatically. ` : ''}
                Choose a port in the ComfyUI launch options.
                <Button variant="outline" size="sm" onClick={onOpenComfy}>Open ComfyUI launch options</Button>
              </>
            )}
            {restart.kind === 'idle' && (canRestart
              ? <Button size="sm" onClick={() => void restartComfy()}>Restart ComfyUI</Button>
              : 'Restart is unavailable until the environment is healthy and no operation blocks launch.')}
            {(restart.kind === 'started' || restart.kind === 'port_conflict' || restart.kind === 'idle') && (
              <Button variant="ghost" size="sm" onClick={() => { setRestart({ kind: 'idle' }); actions.finishRestartOffer() }}>
                Dismiss
              </Button>
            )}
          </AlertDescription>
        </Alert>
      )}

      {registryOpen && (
        <RegistryBrowser
          installDisabledReason={installDisabledReason}
          onInstall={(node) => setInstallTarget({ kind: 'registry', node })}
        />
      )}

      <section aria-label="Installed custom nodes" className="flex flex-col gap-4">
        <div className="flex flex-wrap items-center gap-2">
          <div className="relative min-w-56 flex-1">
            <Search
              className="pointer-events-none absolute top-2.5 left-2.5 size-4 text-muted-foreground"
              aria-hidden="true"
            />
            <Input
              aria-label="Search installed nodes"
              placeholder="Search by name or source"
              className="pl-8"
              value={filters.text}
              onChange={(event) => patchFilters({ text: event.target.value })}
            />
          </div>
          <select
            aria-label="Source kind"
            className={SELECT_CLASS}
            value={filters.source}
            onChange={(event) => patchFilters({ source: event.target.value as SourceFilter })}
          >
            <option value="all">All sources</option>
            <option value="registry">Registry</option>
            <option value="git">Git</option>
            <option value="manual">Manual</option>
          </select>
          <select
            aria-label="Enabled state"
            className={SELECT_CLASS}
            value={filters.enabled}
            onChange={(event) => patchFilters({ enabled: event.target.value as EnabledFilter })}
          >
            <option value="all">Enabled and disabled</option>
            <option value="enabled">Enabled only</option>
            <option value="disabled">Disabled only</option>
          </select>
        </div>
        {nodes.isPending ? (
          <p role="status" className="text-sm text-muted-foreground">Loading custom nodes…</p>
        ) : nodes.isError && !list ? (
          <ErrorNotice error={nodes.error} retry={() => void nodes.refetch()} />
        ) : all.length === 0 ? (
          <EmptyState
            icon={<Puzzle className="size-8" aria-hidden="true" />}
            title="No custom nodes installed"
            description="Browse the Comfy Registry or install from a Git repository."
          />
        ) : visible.length === 0 ? (
          <EmptyState title="No nodes match" description="Change the search or filters." />
        ) : (
          <div className="grid items-start gap-4 lg:grid-cols-[minmax(0,1fr)_22rem]">
            <div className="min-w-0 overflow-x-auto">
              <div className="min-w-[64rem]">
                <InstalledTable
                  nodes={visible}
                  selected={selected}
                  activeId={currentId}
                  onActivate={setActiveId}
                  onToggle={toggle}
                  onToggleAll={toggleAll}
                />
              </div>
            </div>
            {active && (
              <NodeDetails
                node={active}
                repairRequired={repairRequired}
                busy={paused}
                onUpdate={() => setUpdateNode(active)}
                onRetry={() => void actions.prepare({ kind: 'retry', nodeId: active.id })}
                onToggle={() => setConfirm({ node: active, action: active.enabled ? 'disable' : 'enable' })}
                onUninstall={() => setConfirm({ node: active, action: 'uninstall' })}
              />
            )}
          </div>
        )}
      </section>

      <OperationsPanel
        unresolved={unresolved}
        recent={list?.recent ?? []}
        logs={nodes.logs}
        selectedId={operationId}
        onSelect={setOperationId}
        rescanning={actions.rescanning}
        rescanError={actions.rescanError}
        onRescan={() => void actions.rescan()}
      />

      {installTarget && (
        <InstallDialog
          target={installTarget}
          onClose={() => setInstallTarget(null)}
          onPrepare={(preparation) => {
            setInstallTarget(null)
            void actions.prepare(preparation)
          }}
        />
      )}
      {updateNode && (
        <UpdateDialog
          node={updateNode}
          onClose={() => setUpdateNode(null)}
          onPrepare={(reference) => {
            const node = updateNode
            setUpdateNode(null)
            void actions.prepare({ kind: 'update', nodeId: node.id, reference })
          }}
        />
      )}
      {confirm && (
        <ConfirmDialog
          node={confirm.node}
          action={confirm.action}
          busy={actions.busy?.kind === 'direct'}
          onClose={() => setConfirm(null)}
          onConfirm={() => {
            const { node, action } = confirm
            const work = action === 'uninstall' ? actions.uninstall(node) : actions.toggle(node)
            void work.finally(() => setConfirm(null))
          }}
        />
      )}
      {actions.review && (
        <ReviewDialog
          plans={actions.review}
          busy={actions.busy?.kind === 'commit'}
          notice={failureNotice || undefined}
          onCommit={(items) => void actions.commit(items)}
          onClose={() => void actions.discard(actions.review ?? [])}
          onCancel={actions.busy?.kind === 'commit' ? actions.requestCancel : null}
        />
      )}
      {updateAll && (
        <UpdateAllDialog
          nodes={updateAll}
          onStopComfy={onStopComfy}
          onSettled={(updated) => {
            refreshNodeState(client)
            if (updated > 0) {
              actions.noteResult(`Updated ${updated} ${updated === 1 ? 'node' : 'nodes'}.`)
              setSelected(new Set())
            }
          }}
          onClose={() => setUpdateAll(null)}
        />
      )}
    </div>
  )
}
