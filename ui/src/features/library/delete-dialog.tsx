import { Check, CircleX, LoaderCircle, TriangleAlert } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'

import { useTranslation } from '@/shared/i18n'
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
  | { kind: 'error'; error: unknown }

interface Props {
  locations: readonly ModelLocationDto[]
  onStopComfy: () => Promise<void>
  /** Called whenever files may have changed, so the list refreshes. */
  onChanged: () => void
  onClose: () => void
}

const OUTCOME_KEYS = {
  deleted: 'library.delete.outcome.deleted',
  locked: 'library.delete.outcome.locked',
  trash_failed: 'library.delete.outcome.trashFailed',
  stale: 'library.delete.outcome.stale',
  missing: 'library.delete.outcome.missing',
  busy: 'library.delete.outcome.busy',
  failed: 'common.failed',
} as const satisfies Record<DeleteOutcomeDto['kind'], keyof typeof import('@/shared/i18n/translations.json').en>

function nameOf(plan: DeletePlanDto): Map<string, string> {
  return new Map(plan.items.map((item) => [keyOf(item.location), item.name]))
}

/**
 * Delete in explicit steps: a confirmation with the total size, per-file results, and a
 * separate permanent-delete confirmation for files the trash refused.
 */
export function DeleteDialog({ locations, onStopComfy, onChanged, onClose }: Props) {
  const { t } = useTranslation()
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
        if (live) setStage({ kind: 'error', error })
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
      setStage({ kind: 'error', error })
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
      setStage({ kind: 'error', error })
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
              <DialogTitle>{t('library.delete.title')}</DialogTitle>
              <DialogDescription>{t('library.delete.checking')}</DialogDescription>
            </DialogHeader>
            <LoaderCircle className="animate-spin" aria-label={t('common.loadingLabel')} />
          </>
        ) : null}

        {stage.kind === 'confirm' ? (
          <>
            <DialogHeader>
              <DialogTitle>{t('library.delete.confirm', { count: stage.plan.items.length })}</DialogTitle>
              <DialogDescription>
                {t('library.delete.disclosure', { trash })}
              </DialogDescription>
            </DialogHeader>
            <FileList plan={stage.plan} />
            <DialogFooter>
              <Button variant="outline" onClick={onClose}>
                {t('common.cancel')}
              </Button>
              <Button
                variant="destructive"
                onClick={() => void execute(stage.plan)}
              >
                {t('library.delete.move', { count: stage.plan.items.length, size: formatBytes(stage.plan.totalBytes), trash })}
              </Button>
            </DialogFooter>
          </>
        ) : null}

        {stage.kind === 'running' ? (
          <>
            <DialogHeader>
              <DialogTitle>
                {stage.permanent ? t('library.delete.permanently') : t('library.delete.moving', { trash })}
              </DialogTitle>
              <DialogDescription>{t('library.delete.largeFiles')}</DialogDescription>
            </DialogHeader>
            <LoaderCircle className="animate-spin" aria-label={t('common.working')} />
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
                {t('library.delete.confirmPermanent', { count: stage.plan.items.length })}
              </DialogTitle>
              <DialogDescription>
                {t('library.delete.permanentHint', { trash, size: formatBytes(stage.plan.totalBytes) })}
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
                {t('library.delete.acknowledge')}
              </Label>
            </div>
            <DialogFooter>
              <Button variant="outline" onClick={onClose}>
                {t('library.delete.keep')}
              </Button>
              <Button
                variant="destructive"
                disabled={!acknowledged}
                onClick={() => void execute(stage.plan)}
              >
                {t('library.delete.permanentAction', { size: formatBytes(stage.plan.totalBytes) })}
              </Button>
            </DialogFooter>
          </>
        ) : null}

        {stage.kind === 'error' ? (
          <>
            <DialogHeader>
              <DialogTitle>{t('library.delete.failed')}</DialogTitle>
              <DialogDescription>{errorMessage(stage.error)}</DialogDescription>
            </DialogHeader>
            <DialogFooter>
              <Button variant="outline" onClick={onClose}>
                {t('common.close')}
              </Button>
            </DialogFooter>
          </>
        ) : null}
      </DialogContent>
    </Dialog>
  )
}

function FileList({ plan }: { plan: DeletePlanDto }) {
  const { t } = useTranslation()
  const shown = plan.items.slice(0, 8)
  return (
    <ul className="flex max-h-48 flex-col gap-1 overflow-auto text-sm" aria-label={t('library.delete.files')}>
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
          {t('library.delete.more', { count: plan.items.length - shown.length })}
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
  const { t } = useTranslation()
  const deleted = report.results.filter((r) => r.outcome.kind === 'deleted').length
  const locked = report.results.filter((r) => r.outcome.kind === 'locked').length
  const trash = trashName()
  return (
    <>
      <DialogHeader>
        <DialogTitle>
          {t('library.delete.summary', { deleted, count: report.results.length })}
        </DialogTitle>
        <DialogDescription>
          {t('library.delete.reportHint')}
        </DialogDescription>
      </DialogHeader>
      <ul className="flex max-h-56 flex-col gap-1.5 overflow-auto text-sm" aria-label={t('library.delete.results')}>
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
                  {t(OUTCOME_KEYS[result.outcome.kind])}
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
            {t('library.delete.trashFailed', { count: report.permanentPlan.items.length, trash })}
          </AlertTitle>
          <AlertDescription>
            {t('library.delete.retryHint')}
          </AlertDescription>
        </Alert>
      ) : null}
      <DialogFooter>
        {locked > 0 ? (
          <>
            <Button variant="outline" disabled={busy} onClick={() => onRetry(false)}>
              {t('common.retry')}
            </Button>
            <Button variant="outline" disabled={busy} onClick={() => onRetry(true)}>
              {t('common.stopRetry')}
            </Button>
          </>
        ) : null}
        {report.permanentPlan ? (
          <Button
            variant="destructive"
            disabled={busy}
            onClick={() => onPermanent(report.permanentPlan as DeletePlanDto)}
          >
            {t('library.delete.reviewPermanent')}
          </Button>
        ) : null}
        <Button disabled={busy} onClick={onClose}>
          {t('common.done')}
        </Button>
      </DialogFooter>
    </>
  )
}
