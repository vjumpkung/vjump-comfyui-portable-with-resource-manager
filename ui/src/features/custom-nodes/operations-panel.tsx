import { LoaderCircle, RefreshCw, TriangleAlert } from 'lucide-react'
import { lazy, Suspense } from 'react'

import type { OperationDto } from '@/shared/ipc/gateway'
import { errorMessage } from '@/shared/lib/display'
import { Alert, AlertDescription, AlertTitle } from '@/shared/ui/alert'
import { Button } from '@/shared/ui/button'

import { trashName } from './model'
import { Chip, PhaseChip } from './status-chips'
import { useNodeOperation, type NodeLogEntry } from './use-nodes'

const LogView = lazy(() =>
  import('@/shared/ui/log-view').then((module) => ({ default: module.LogView })),
)

const KIND_LABEL: Record<OperationDto['kind'], string> = {
  install: 'Install',
  update: 'Update',
  retry_dependencies: 'Retry dependencies',
  enable: 'Enable',
  disable: 'Disable',
  uninstall: 'Uninstall',
}

/** An authoritative `logTail` replaces the live buffer: the two overlap and are never merged. */
export function operationLines(
  status: OperationDto | undefined,
  logs: readonly NodeLogEntry[],
  operationId: string,
): string[] {
  if (status) return status.logTail
  return logs.filter((entry) => entry.operationId === operationId).map((entry) => entry.line)
}

function phaseNote(operation: OperationDto): string | null {
  if (operation.phase === 'cleanup_pending') {
    return `The node change itself finished. A retained backup could not be moved to the ${trashName()} yet; cleanup is retried on the next rescan. This is not an installation failure.`
  }
  if (operation.phase === 'verified') return 'Verified. Finishing cleanup.'
  return null
}

function OperationDetail({ operationId, logs }: {
  operationId: string
  logs: readonly NodeLogEntry[]
}) {
  const status = useNodeOperation(operationId)
  const lines = operationLines(status.data, logs, operationId)
  const operation = status.data
  return (
    <div className="flex flex-col gap-3 rounded-lg border p-4" aria-label="Selected operation">
      {status.isPending && <p role="status" className="text-sm text-muted-foreground">Loading the operation…</p>}
      {status.isError && (
        <p role="alert" className="text-sm text-destructive">{errorMessage(status.error)}</p>
      )}
      {operation && (
        <div className="flex flex-wrap items-center gap-2">
          <span className="font-semibold">{operation.nodeName}</span>
          <span className="text-xs text-muted-foreground">{KIND_LABEL[operation.kind]}</span>
          <PhaseChip phase={operation.phase} />
        </div>
      )}
      {operation?.failure && (
        <p className="text-sm break-words text-destructive [overflow-wrap:anywhere]">{operation.failure}</p>
      )}
      {lines.length > 0 ? (
        <Suspense fallback={<p className="text-sm text-muted-foreground">Loading the log viewer…</p>}>
          <LogView lines={lines} />
        </Suspense>
      ) : (
        <p className="text-sm text-muted-foreground">
          No log lines are available. Logs live in memory only; the recorded phase and failure above remain.
        </p>
      )}
    </div>
  )
}

function OperationRow({ operation, selected, onSelect }: {
  operation: OperationDto
  selected: boolean
  onSelect: () => void
}) {
  const note = phaseNote(operation)
  return (
    <li className="flex flex-col gap-1 rounded-lg border p-3">
      <div className="flex flex-wrap items-center gap-2">
        <span className="min-w-0 font-medium break-words [overflow-wrap:anywhere]">{operation.nodeName}</span>
        <span className="text-xs text-muted-foreground">{KIND_LABEL[operation.kind]}</span>
        <PhaseChip phase={operation.phase} />
        {operation.blocksStart && <Chip icon={TriangleAlert} className="bg-brand-pink">Blocks start</Chip>}
        <Button
          variant="ghost"
          size="sm"
          className="ml-auto"
          aria-pressed={selected}
          aria-label={`${selected ? 'Hide' : 'Show'} details of the ${KIND_LABEL[operation.kind].toLowerCase()} of ${operation.nodeName}`}
          onClick={onSelect}
        >
          {selected ? 'Hide details' : 'Details'}
        </Button>
      </div>
      {operation.failure && (
        <p className="text-sm break-words text-destructive [overflow-wrap:anywhere]">{operation.failure}</p>
      )}
      {note && <p className="text-xs text-muted-foreground">{note}</p>}
    </li>
  )
}

export function OperationsPanel({
  unresolved,
  recent,
  logs,
  selectedId,
  onSelect,
  rescanning,
  rescanError,
  onRescan,
}: {
  unresolved: readonly OperationDto[]
  recent: readonly OperationDto[]
  logs: readonly NodeLogEntry[]
  selectedId: string | null
  onSelect: (operationId: string | null) => void
  rescanning: boolean
  rescanError: unknown
  onRescan: () => void
}) {
  const blocking = unresolved.filter((operation) => operation.blocksStart)
  const toggle = (id: string) => onSelect(selectedId === id ? null : id)
  return (
    <section aria-label="Operations" className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h2 className="text-lg font-bold tracking-tight">Operations</h2>
        <Button variant="outline" disabled={rescanning} onClick={onRescan}>
          {rescanning
            ? <LoaderCircle className="animate-spin" aria-hidden="true" />
            : <RefreshCw aria-hidden="true" />}
          Rescan / finish recovery
        </Button>
      </div>
      {blocking.length > 0 && (
        <Alert variant="destructive">
          <TriangleAlert aria-hidden="true" />
          <AlertTitle>An interrupted node operation blocks ComfyUI from starting</AlertTitle>
          <AlertDescription>
            {blocking.map((operation) => operation.nodeName).join(', ')}. Rescan to finish recovery.
            Ambiguous copies are preserved, never deleted, and keep ComfyUI from starting until resolved.
          </AlertDescription>
        </Alert>
      )}
      {rescanError !== null && rescanError !== undefined && (
        <Alert variant="destructive">
          <TriangleAlert aria-hidden="true" />
          <AlertTitle>The rescan failed</AlertTitle>
          <AlertDescription>{errorMessage(rescanError)}</AlertDescription>
        </Alert>
      )}
      {unresolved.length > 0 && (
        <div className="flex flex-col gap-2">
          <h3 className="font-mono text-xs font-semibold tracking-wider text-muted-foreground uppercase">Unfinished</h3>
          <ul className="flex flex-col gap-2">
            {unresolved.map((operation) => (
              <OperationRow key={operation.id} operation={operation}
                selected={selectedId === operation.id} onSelect={() => toggle(operation.id)} />
            ))}
          </ul>
        </div>
      )}
      {recent.length > 0 ? (
        <div className="flex flex-col gap-2">
          <h3 className="font-mono text-xs font-semibold tracking-wider text-muted-foreground uppercase">Recent results (latest 16)</h3>
          <ul className="flex flex-col gap-2">
            {recent.map((operation) => (
              <OperationRow key={operation.id} operation={operation}
                selected={selectedId === operation.id} onSelect={() => toggle(operation.id)} />
            ))}
          </ul>
        </div>
      ) : unresolved.length === 0 ? (
        <p className="text-sm text-muted-foreground">No recent node operations.</p>
      ) : null}
      {selectedId !== null && <OperationDetail operationId={selectedId} logs={logs} />}
    </section>
  )
}
