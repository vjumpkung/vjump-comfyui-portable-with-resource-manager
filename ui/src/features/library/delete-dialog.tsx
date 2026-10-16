import { Check, CircleX, LoaderCircle, TriangleAlert } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'

import {
  gateway,
  type DeleteOutcomeDto,
  type DeletePlanDto,
  type DeleteReportDto,
  type ModelLocationDto,
} from '@/shared/ipc/gateway'
import { errorMessage } from '@/shared/lib/display'
import { Alert, AlertDescription, AlertTitle } from '@/shared/ui/alert'
import { Button } from '@/shared/ui/button'
import { Checkbox } from '@/shared/ui/checkbox'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shared/ui/dialog'
import { Label } from '@/shared/ui/label'

import { formatBytes, keyOf, trashName } from './model-list'

type Stage =
  | { kind: 'planning' }
  | { kind: 'confirm'; plan: DeletePlanDto }
  | { kind: 'running'; permanent: boolean }
  | { kind: 'report'; report: DeleteReportDto; names: Map<string, string> }
  | { kind: 'confirm-permanent'; plan: DeletePlanDto }
  | { kind: 'error'; message: string }

interface Props {
  locations: readonly ModelLocationDto[]
  onStopComfy: () => Promise<void>
  /** Called whenever files may have changed, so the list refreshes. */
  onChanged: () => void
  onClose: () => void
}

const OUTCOME_TEXT: Record<DeleteOutcomeDto['kind'], string> = {
  deleted: 'Deleted',
  locked: 'In use by another program',
  trash_failed: 'Could not be moved; nothing was deleted',
  stale: 'Changed since it was listed; nothing was deleted',
  missing: 'Already gone',
  busy: 'A download or another operation is using it',
  failed: 'Failed',
}

function nameOf(plan: DeletePlanDto): Map<string, string> {
  return new Map(plan.items.map((item) => [keyOf(item.location), item.name]))
}
function count(n: number): string {
  return `${n} ${n === 1 ? 'file' : 'files'}`
}

/**
 * Delete in explicit steps: a confirmation with the total size, per-file results, and a
 * separate permanent-delete confirmation for files the trash refused.
 */
export function DeleteDialog({ locations, onStopComfy, onChanged, onClose }: Props) {
  const [stage, setStage] = useState<Stage>({ kind: 'planning' })
  const [acknowledged, setAcknowledged] = useState(false)
  const [busy, setBusy] = useState(false)
  const names = useRef(new Map<string, string>())

  useEffect(() => {
    let live = true
    gateway
      .modelDeletePlan([...locations])
      .then((plan) => {
        if (!live) return
        names.current = nameOf(plan)
        setStage({ kind: 'confirm', plan })
      })
      .catch((error: unknown) => {
        if (live) setStage({ kind: 'error', message: errorMessage(error) })
      })
    return () => {
      live = false
    }
    // The selection is fixed for the lifetime of the dialog.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  async function execute(plan: DeletePlanDto) {
    setStage({ kind: 'running', permanent: plan.permanent })
    try {
      const report = plan.permanent
        ? await gateway.modelDeletePermanently(plan.id)
        : await gateway.modelDelete(plan.id)
      setStage({ kind: 'report', report, names: names.current })
    } catch (error) {
      setStage({ kind: 'error', message: errorMessage(error) })
    } finally {
      onChanged()
    }
  }

  async function retryLocked(report: DeleteReportDto, stopFirst: boolean) {
    setBusy(true)
    try {
      if (stopFirst) await onStopComfy()
      const locked = report.results
        .filter((result) => result.outcome.kind === 'locked')
        .map((result) => result.location)
      const plan = await gateway.modelDeletePlan(locked)
      names.current = new Map([...names.current, ...nameOf(plan)])
      await execute(plan)
    } catch (error) {
      setStage({ kind: 'error', message: errorMessage(error) })
    } finally {
      setBusy(false)
    }
  }

  const trash = trashName()
  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open && stage.kind !== 'running') onClose()
      }}
    >
      <DialogContent>
        {stage.kind === 'planning' ? (
          <>
            <DialogHeader>
              <DialogTitle>Delete models</DialogTitle>
              <DialogDescription>Checking the selected files...</DialogDescription>
            </DialogHeader>
            <LoaderCircle className="animate-spin" aria-label="Loading" />
          </>
        ) : null}

        {stage.kind === 'confirm' ? (
          <>
            <DialogHeader>
              <DialogTitle>Delete {count(stage.plan.items.length)}?</DialogTitle>
              <DialogDescription>
                The models move to the {trash}, so you can restore them there.
                Metadata files beside them are kept.
              </DialogDescription>
            </DialogHeader>
            <FileList plan={stage.plan} />
            <DialogFooter>
              <Button variant="outline" onClick={onClose}>
                Cancel
              </Button>
              <Button
                variant="destructive"
                onClick={() => void execute(stage.plan)}
              >
                Move {count(stage.plan.items.length)} (
                {formatBytes(stage.plan.totalBytes)}) to {trash}
              </Button>
            </DialogFooter>
          </>
        ) : null}

        {stage.kind === 'running' ? (
          <>
            <DialogHeader>
              <DialogTitle>
                {stage.permanent ? 'Deleting permanently' : `Moving to the ${trash}`}
              </DialogTitle>
              <DialogDescription>This can take a moment for large files.</DialogDescription>
            </DialogHeader>
            <LoaderCircle className="animate-spin" aria-label="Working" />
          </>
        ) : null}

        {stage.kind === 'report' ? (
          <Report
            report={stage.report}
            names={stage.names}
            busy={busy}
            onClose={onClose}
            onRetry={(stop) => void retryLocked(stage.report, stop)}
            onPermanent={(plan) => {
              setAcknowledged(false)
              setStage({ kind: 'confirm-permanent', plan })
            }}
          />
        ) : null}

        {stage.kind === 'confirm-permanent' ? (
          <>
            <DialogHeader>
              <DialogTitle>
                Delete {count(stage.plan.items.length)} permanently?
              </DialogTitle>
              <DialogDescription>
                The {trash} could not take these files. Deleting them now frees{' '}
                {formatBytes(stage.plan.totalBytes)} and cannot be undone.
              </DialogDescription>
            </DialogHeader>
            <FileList plan={stage.plan} />
            <div className="flex items-center gap-2">
              <Checkbox
                id="permanent-ack"
                checked={acknowledged}
                onCheckedChange={(checked) => setAcknowledged(checked === true)}
              />
              <Label htmlFor="permanent-ack">
                I understand this cannot be undone
              </Label>
            </div>
            <DialogFooter>
              <Button variant="outline" onClick={onClose}>
                Keep the files
              </Button>
              <Button
                variant="destructive"
                disabled={!acknowledged}
                onClick={() => void execute(stage.plan)}
              >
                Delete permanently ({formatBytes(stage.plan.totalBytes)})
              </Button>
            </DialogFooter>
          </>
        ) : null}

        {stage.kind === 'error' ? (
          <>
            <DialogHeader>
              <DialogTitle>Delete failed</DialogTitle>
              <DialogDescription>{stage.message}</DialogDescription>
            </DialogHeader>
            <DialogFooter>
              <Button variant="outline" onClick={onClose}>
                Close
              </Button>
            </DialogFooter>
          </>
        ) : null}
      </DialogContent>
    </Dialog>
  )
}

function FileList({ plan }: { plan: DeletePlanDto }) {
  const shown = plan.items.slice(0, 8)
  return (
    <ul className="flex max-h-48 flex-col gap-1 overflow-auto text-sm" aria-label="Files">
      {shown.map((item) => (
        <li key={keyOf(item.location)} className="flex justify-between gap-3">
          <span className="min-w-0 truncate" title={item.name}>
            {item.name}
          </span>
          <span className="shrink-0 font-mono text-xs text-muted-foreground">
            {formatBytes(item.size)}
          </span>
        </li>
      ))}
      {plan.items.length > shown.length ? (
        <li className="text-muted-foreground">
          and {plan.items.length - shown.length} more
        </li>
      ) : null}
    </ul>
  )
}

function Report({
  report,
  names,
  busy,
  onClose,
  onRetry,
  onPermanent,
}: {
  report: DeleteReportDto
  names: Map<string, string>
  busy: boolean
  onClose: () => void
  onRetry: (stopComfy: boolean) => void
  onPermanent: (plan: DeletePlanDto) => void
}) {
  const deleted = report.results.filter((r) => r.outcome.kind === 'deleted').length
  const locked = report.results.filter((r) => r.outcome.kind === 'locked').length
  const trash = trashName()
  return (
    <>
      <DialogHeader>
        <DialogTitle>
          {deleted} of {count(report.results.length)} deleted
        </DialogTitle>
        <DialogDescription>
          Each file is reported on its own; a failure never affects the others.
        </DialogDescription>
      </DialogHeader>
      <ul className="flex max-h-56 flex-col gap-1.5 overflow-auto text-sm" aria-label="Results">
        {report.results.map((result) => {
          const ok = result.outcome.kind === 'deleted'
          return (
            <li key={keyOf(result.location)} className="flex items-start gap-2">
              {ok ? (
                <Check className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
              ) : (
                <CircleX
                  className="mt-0.5 size-4 shrink-0 text-destructive"
                  aria-hidden="true"
                />
              )}
              <span className="min-w-0 flex-1 break-words [overflow-wrap:anywhere]">
                {names.get(keyOf(result.location)) ?? result.location.path}
                <span className="block text-xs text-muted-foreground">
                  {OUTCOME_TEXT[result.outcome.kind]}
                </span>
              </span>
            </li>
          )
        })}
      </ul>
      {report.permanentPlan ? (
        <Alert variant="destructive">
          <TriangleAlert aria-hidden="true" />
          <AlertTitle>
            {count(report.permanentPlan.items.length)} could not go to the {trash}
          </AlertTitle>
          <AlertDescription>
            Nothing was deleted. You can delete them permanently, which needs a
            separate confirmation.
          </AlertDescription>
        </Alert>
      ) : null}
      <DialogFooter>
        {locked > 0 ? (
          <>
            <Button variant="outline" disabled={busy} onClick={() => onRetry(false)}>
              Retry
            </Button>
            <Button variant="outline" disabled={busy} onClick={() => onRetry(true)}>
              Stop ComfyUI and retry
            </Button>
          </>
        ) : null}
        {report.permanentPlan ? (
          <Button
            variant="destructive"
            disabled={busy}
            onClick={() => onPermanent(report.permanentPlan as DeletePlanDto)}
          >
            Review permanent deletion
          </Button>
        ) : null}
        <Button disabled={busy} onClick={onClose}>
          Done
        </Button>
      </DialogFooter>
    </>
  )
}
