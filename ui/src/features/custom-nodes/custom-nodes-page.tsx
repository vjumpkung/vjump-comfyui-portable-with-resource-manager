import { useQueryClient } from '@tanstack/react-query'
import { FolderOpen, LoaderCircle, Puzzle, Search, TriangleAlert, Wrench } from 'lucide-react'
import { useMemo, useState } from 'react'
import { toast } from 'sonner'

import { useTranslation } from '@/shared/i18n'
import {
  gateway,
  type ComfyStatusDto,
  type InstalledNodeDto,
  type OperationDto,
  type StartResultDto,
} from '@/shared/ipc/gateway'
import { errorMessage } from '@/shared/lib/display'
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
  actionContextLabel,
  DEFAULT_NODE_FILTERS,
  nodeText,
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
import { ResolveDialog } from './resolve-dialog'
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
  const { t } = useTranslation()
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
  const [keepFiles, setKeepFiles] = useState<OperationDto | null>(null)
  const [restart, setRestart] = useState<Restart>({ kind: 'idle' })

  const list = nodes.data
  const all = useMemo(() => list?.nodes ?? [], [list])
  const visible = filterNodes(all, filters)
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
    ? t('customNodes.setupRunningReason')
    : repairRequired
      ? t('customNodes.repairInstallReason')
      : actions.working
        ? t('customNodes.anotherRequest')
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

  async function resolveKeepFiles() {
    if (keepFiles && await actions.resolveOperation(keepFiles.id)) setKeepFiles(null)
  }

  async function stopRunning() {
    try {
      await onStopComfy()
    } catch (error) {
      actions.report(error, { kind: 'stop' })
    }
  }

  async function openFolder() {
    try {
      await gateway.nodeReveal(null)
    } catch (error) {
      toast.error(errorMessage(error))
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
      actions.report(error, { kind: 'restart' }, { retry: restartComfy })
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
          <h1 className="text-2xl font-extrabold tracking-tighter sm:text-3xl">{t('nav.customNodes')}</h1>
          <p className="font-mono text-xs text-muted-foreground">
            {selectedNodes.length > 0
              ? t('customNodes.summarySelected', { count: selectedNodes.length, installed: all.length, disabled: all.filter((node) => !node.enabled).length })
              : t('customNodes.summary', { count: all.length, installed: all.length, disabled: all.filter((node) => !node.enabled).length })}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button
            variant="outline"
            aria-pressed={registryOpen}
            onClick={() => setRegistryOpen((open) => !open)}
          >
            {registryOpen ? t('customNodes.hideRegistry') : t('customNodes.browseRegistry')}
          </Button>
          <Button
            variant="outline"
            disabled={installDisabledReason !== null}
            title={installDisabledReason ?? undefined}
            onClick={() => setInstallTarget({ kind: 'git' })}
          >
            {t('customNodes.installGit')}
          </Button>
          <Button variant="outline" disabled={updateAllBlocked} onClick={() => setUpdateAll(updateScope)}>
            {selectedNodes.length > 0 ? t('customNodes.updateSelected') : t('common.updateAll')} ({updateCount})
          </Button>
          <Button variant="outline" onClick={() => void openFolder()}>
            <FolderOpen aria-hidden="true" />
            {t('common.openFolder')}
          </Button>
        </div>
      </div>

      {maintenanceBusy && (
        <Alert>
          <LoaderCircle className="animate-spin" aria-hidden="true" />
          <AlertTitle>{t('customNodes.setupRunning')}</AlertTitle>
          <AlertDescription>{t('customNodes.setupPausedHint')}</AlertDescription>
        </Alert>
      )}
      {repairRequired && (
        <Alert variant="destructive">
          <Wrench aria-hidden="true" />
          <AlertTitle>{t('customNodes.repairTitle')}</AlertTitle>
          <AlertDescription>
            {t('customNodes.repairHint')}
            <Button variant="outline" size="sm" onClick={onRepair}>{t('setup.mode.repair')}</Button>
          </AlertDescription>
        </Alert>
      )}
      {comfyRunning && (
        <Alert>
          <TriangleAlert aria-hidden="true" />
          <AlertTitle>{t('customNodes.comfyRunning')}</AlertTitle>
          <AlertDescription>
            {t('customNodes.stopHint')}
            <Button variant="outline" size="sm" disabled={runtimeBusy} onClick={() => void stopRunning()}>
              {t('customNodes.stopComfy')}
            </Button>
          </AlertDescription>
        </Alert>
      )}

      {pendingReviews.length > 0 && !actions.review && updateAll === null && actions.busy === null && (
        <Alert>
          <AlertTitle>
            {t('customNodes.waitingReview', { count: pendingReviews.length })}
          </AlertTitle>
          <AlertDescription>
            {t('customNodes.resumeHint')}
            <ul className="mt-2 flex flex-col gap-2">
              {pendingReviews.map((plan) => (
                <li key={plan.id} className="flex flex-wrap items-center gap-2">
                  <span className="min-w-0 font-medium break-words [overflow-wrap:anywhere]">
                    {plan.name} · {sourceLabel(plan.source)}
                  </span>
                  <Button size="sm" disabled={paused} onClick={() => actions.resume(plan)}>
                    {t('customNodes.resumeReview')}
                  </Button>
                  <Button variant="ghost" size="sm" disabled={paused} onClick={() => void actions.discard([plan])}>
                    {t('customNodes.discard')}
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
            {t('customNodes.busy', { context: actionContextLabel(actions.busy.context) })}
          </AlertTitle>
          <AlertDescription>
            {actions.cancelRequested
              ? t('customNodes.cancelRequested')
              : actions.busy.kind === 'direct'
                ? t('customNodes.cannotCancel')
                : t('customNodes.closingHint')}
            {actions.busy.kind === 'prepare' && (
              <Button variant="outline" size="sm" disabled={actions.cancelRequested} onClick={actions.requestCancel}>
                {t('customNodes.cancelPreparation')}
              </Button>
            )}
          </AlertDescription>
        </Alert>
      )}
      {!actions.review && failureNotice}
      {actions.notice && (
        <Alert role="status">
          <AlertTitle>{'count' in actions.notice.message
            ? t('customNodes.updatedCount', { count: actions.notice.message.count })
            : nodeText(actions.notice.message)}</AlertTitle>
          {actions.notice.cleanupPending && (
            <AlertDescription>
              {t('customNodes.cleanupHint')}
            </AlertDescription>
          )}
        </Alert>
      )}
      {offerRestart && (
        <Alert role="status">
          <AlertTitle>
            {restart.kind === 'started' ? t('customNodes.restarted')
              : restart.kind === 'port_conflict' ? t('customNodes.portInUse', { port: restart.requested })
                : restart.kind === 'working' ? t('customNodes.restarting')
                  : t('customNodes.restartHint')}
          </AlertTitle>
          <AlertDescription>
            {restart.kind === 'port_conflict' && (
              <>
                {restart.suggested !== null ? t('customNodes.portFree', { port: restart.suggested }) : ''}
                {t('customNodes.choosePort')}
                <Button variant="outline" size="sm" onClick={onOpenComfy}>{t('customNodes.openLaunchOptions')}</Button>
              </>
            )}
            {restart.kind === 'idle' && (canRestart
              ? <Button size="sm" onClick={() => void restartComfy()}>{t('customNodes.restartComfy')}</Button>
              : t('customNodes.restartUnavailable'))}
            {(restart.kind === 'started' || restart.kind === 'port_conflict' || restart.kind === 'idle') && (
              <Button variant="ghost" size="sm" onClick={() => { setRestart({ kind: 'idle' }); actions.finishRestartOffer() }}>
                {t('common.dismiss')}
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

      <section aria-label={t('customNodes.installed')} className="flex flex-col gap-4">
        <div className="flex flex-wrap items-center gap-2">
          <div className="relative min-w-56 flex-1">
            <Search
              className="pointer-events-none absolute top-2.5 left-2.5 size-4 text-muted-foreground"
              aria-hidden="true"
            />
            <Input
              aria-label={t('customNodes.searchInstalled')}
              placeholder={t('customNodes.searchHint')}
              className="pl-8"
              value={filters.text}
              onChange={(event) => patchFilters({ text: event.target.value })}
            />
          </div>
          <select
            aria-label={t('customNodes.sourceKind')}
            className={SELECT_CLASS}
            value={filters.source}
            onChange={(event) => patchFilters({ source: event.target.value as SourceFilter })}
          >
            <option value="all">{t('customNodes.allSources')}</option>
            <option value="registry">{t('customNodes.source.registry')}</option>
            <option value="git">Git</option>
            <option value="manual">{t('customNodes.source.manual')}</option>
          </select>
          <select
            aria-label={t('customNodes.enabledState')}
            className={SELECT_CLASS}
            value={filters.enabled}
            onChange={(event) => patchFilters({ enabled: event.target.value as EnabledFilter })}
          >
            <option value="all">{t('customNodes.bothStates')}</option>
            <option value="enabled">{t('customNodes.enabledOnly')}</option>
            <option value="disabled">{t('customNodes.disabledOnly')}</option>
          </select>
        </div>
        {nodes.isPending ? (
          <p role="status" className="text-sm text-muted-foreground">{t('customNodes.loading')}</p>
        ) : nodes.isError && !list ? (
          <ErrorNotice error={nodes.error} retry={() => void nodes.refetch()} />
        ) : all.length === 0 ? (
          <EmptyState
            icon={<Puzzle className="size-8" aria-hidden="true" />}
            title={t('customNodes.emptyTitle')}
            description={t('customNodes.emptyHint')}
          />
        ) : visible.length === 0 ? (
          <EmptyState title={t('customNodes.noMatch')} description={t('customNodes.changeFilters')} />
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
        onKeepFiles={setKeepFiles}
        resolving={actions.resolving}
        resolveError={actions.resolveError}
      />

      {keepFiles && (
        <ResolveDialog
          operation={keepFiles}
          busy={actions.resolving}
          onClose={() => setKeepFiles(null)}
          onConfirm={() => void resolveKeepFiles()}
        />
      )}

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
              actions.noteResult(updated)
              setSelected(new Set())
            }
          }}
          onClose={() => setUpdateAll(null)}
        />
      )}
    </div>
  )
}
