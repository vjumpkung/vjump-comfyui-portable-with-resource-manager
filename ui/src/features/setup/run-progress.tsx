import { Fragment, lazy, Suspense, useState, type ReactNode } from 'react'
import { Check, RefreshCw } from 'lucide-react'

import type { CommitInput, RunSnapshotDto } from '@/shared/ipc/gateway'
import { bytes } from '@/shared/lib/display'
import { Alert, AlertTitle, AlertDescription } from '@/shared/ui/alert'
import { Button } from '@/shared/ui/button'
import {
  Card,
  CardHeader,
  CardTitle,
  CardDescription,
  CardContent,
} from '@/shared/ui/card'
import { Checkbox } from '@/shared/ui/checkbox'
import { ErrorNotice } from '@/shared/ui/feedback'
import { Field, FieldLabel } from '@/shared/ui/field'
import { Progress } from '@/shared/ui/progress'
import { StatusBadge } from '@/shared/ui/status-badge'
import { canSkip, type CuratedReviewProps } from './model'
import type { useSetupRun } from './use-setup-run'

const LogView = lazy(() =>
  import('@/shared/ui/log-view').then((module) => ({
    default: module.LogView,
  })),
)

export function RunProgress({
  run,
  controller,
  refresh,
  onDone,
  renderCuratedReview,
}: {
  run: RunSnapshotDto | null
  controller: ReturnType<typeof useSetupRun>
  refresh: () => void
  onDone?: () => void
  renderCuratedReview: (props: CuratedReviewProps) => ReactNode
}) {
  const { action, cancel, lines, progress, finished } = controller
  const [consent, setConsent] = useState<string | null>(null)
  const reviewKey = JSON.stringify([run?.id, run?.curatedReviews.map((plan) => [plan.id, plan.binding])])
  const [review, setReview] = useState<{
    key: string
    open: boolean
    decisions: CommitInput[] | null
  }>({ key: reviewKey, open: true, decisions: null })
  if (review.key !== reviewKey)
    setReview({ key: reviewKey, open: true, decisions: null })
  if (!run || (action.data && action.data.runId !== run.id))
    return <p role="status">Loading the registered run…</p>
  const busy = run.status === 'registered' || run.status === 'running'
  const retryable = run.status === 'failed' || run.status === 'interrupted'
  const failure = run.failure
  const sourceConsent = failure?.consent
  const curatedPause = retryable && failure?.step === 'curated_nodes'
  const liveReviews = retryable && run.curatedReviews.length > 0
  const curatedApproved = review.key === reviewKey && review.decisions !== null
  const consentKey = sourceConsent
    ? `${run.id}:${sourceConsent.source}:${sourceConsent.commit}`
    : null
  const approved = consentKey !== null && consent === consentKey
  const completed = run.steps.filter((step) =>
    ['succeeded', 'warning', 'skipped'].includes(step.status),
  ).length
  const percentage = (completed / Math.max(1, run.steps.length)) * 100
  const pending = action.isPending || cancel.isPending
  const retry = (curatedDecisions: CommitInput[]) => action.mutate({
    kind: 'retry',
    runId: run.id,
    managerConsent: approved ? sourceConsent?.commit ?? null : null,
    curatedDecisions,
  })
  return (
    <div className="flex flex-col gap-6">
      <Card>
        <CardHeader>
          <CardTitle>
            Installation progress <StatusBadge status={run.status} />
          </CardTitle>
          <CardDescription>
            Run {run.id} · target {run.targetEnvId}. Cancellation completes
            after process cleanup.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <Progress value={percentage} aria-label="Setup steps completed" />
          <p className="font-mono text-xs">
            {completed} / {run.steps.length} steps complete
          </p>
          {progress?.runId === run.id && (
            <p className="font-mono text-xs">
              {progress.step}: {bytes(progress.doneBytes)} /{' '}
              {bytes(progress.totalBytes)}
            </p>
          )}
          <ol className="flex flex-col gap-3">
            {run.steps.map((step) => (
              <li
                key={step.step}
                className="flex flex-wrap items-center justify-between gap-2"
              >
                <div>
                  <span className="font-mono text-xs">{step.number}. </span>
                  {step.label}
                  <p className="text-xs text-muted-foreground">
                    {step.policy} · attempt {step.attempts}
                    {step.message ? ` · ${step.message}` : ''}
                  </p>
                </div>
                <StatusBadge status={step.status} />
              </li>
            ))}
          </ol>
          {run.curated.length > 0 && <section aria-label="Curated node progress">
            <h2 className="font-bold">Curated custom nodes</h2>
            <ul className="flex flex-col gap-3">
              {run.curated.map((entry) => <li key={entry.id}>
                <span className="break-all font-mono">{entry.id}</span>{' '}
                <StatusBadge status={entry.status} />
                {entry.message && <p className="break-words text-sm">{entry.message}</p>}
              </li>)}
            </ul>
          </section>}
          {failure && (
            <Alert variant="destructive">
              <AlertTitle>Step failed: {failure.step}</AlertTitle>
              <AlertDescription>
                <p>{failure.message}</p>
                <p className="break-all font-mono">
                  Command: {failure.command ?? 'Not available'} · exit code:{' '}
                  {failure.exitCode ?? 'Not available'}
                </p>
                {failure.tail.length > 0 && (
                  <pre className="max-h-48 overflow-auto whitespace-pre-wrap break-all font-mono text-xs">
                    {failure.tail.join('\n')}
                  </pre>
                )}
              </AlertDescription>
            </Alert>
          )}
          {sourceConsent && (
            <Alert>
              <AlertTitle>
                Allow code from this exact Manager source?
              </AlertTitle>
              <AlertDescription>
                <p>
                  Installing dependencies can execute code from this repository.
                  Confirm only if you trust this exact source and commit.
                </p>
                <p className="break-all font-mono">
                  {sourceConsent.source}
                  <br />
                  {sourceConsent.commit}
                </p>
                <Field orientation="horizontal">
                  <Checkbox
                    id="manager-consent"
                    checked={approved}
                    onCheckedChange={(checked) =>
                      setConsent(checked === true ? consentKey : null)
                    }
                  />
                  <FieldLabel htmlFor="manager-consent">
                    I trust this source and approve code execution at this
                    commit
                  </FieldLabel>
                </Field>
              </AlertDescription>
            </Alert>
          )}
          {finished?.runId === run.id && finished.outcome === 'error' && (
            <Alert variant="destructive">
              <AlertTitle>The workflow could not run</AlertTitle>
              <AlertDescription>
                Refresh its persisted status before retrying. It may have been
                blocked by maintenance or runtime preflight.
              </AlertDescription>
            </Alert>
          )}
          {action.isError && <ErrorNotice error={action.error} />}
          {cancel.isError && <ErrorNotice error={cancel.error} />}
          {curatedPause && !liveReviews && <Alert>
            <AlertTitle>Fresh curated reviews are required</AlertTitle>
            <AlertDescription>
              Reviews and approvals exist only in the current session. After a backend restart, Retry prepares fresh reviews; approve those exact plans before execution can continue.
            </AlertDescription>
          </Alert>}
          <div className="flex flex-wrap gap-2">
            {liveReviews && <Button variant="outline" disabled={pending}
              onClick={() => setReview((current) => ({ ...current, open: true }))}>
              Review curated plans
            </Button>}
            {retryable && (
              <Button
                disabled={pending || (!!sourceConsent && !approved) || (liveReviews && !curatedApproved)}
                onClick={() => retry(liveReviews ? review.decisions! : [])}
              >
                Retry same run
              </Button>
            )}
            {canSkip(run) && (
              <Button
                variant="outline"
                disabled={pending}
                onClick={() =>
                  action.mutate({
                    kind: 'skipStep',
                    runId: run.id,
                    step: failure!.step,
                  })
                }
              >
                Skip optional step
              </Button>
            )}
            {(busy || retryable) && (
              <Button
                variant="destructive"
                disabled={pending || (busy && cancel.isSuccess)}
                onClick={() => cancel.mutate(run.id)}
              >
                {busy && (cancel.isPending || cancel.isSuccess)
                  ? 'Cancelling and cleaning up…'
                  : 'Abort setup'}
              </Button>
            )}
            <Button variant="outline" onClick={refresh}>
              <RefreshCw data-icon="inline-start" />
              Reconnect status
            </Button>
            {!busy && onDone && (
              <Button variant="outline" onClick={onDone}>
                <Check data-icon="inline-start" />
                {run.status === 'succeeded'
                  ? 'Continue'
                  : 'Back to environment'}
              </Button>
            )}
          </div>
          {run.status === 'cancelled' && (
            <p>
              Setup was cancelled. Start a new setup to try again; the partial
              environment is not active.
            </p>
          )}
          {run.status === 'succeeded' && (
            <p role="status">
              Verification finished. The selected environment is ready.
            </p>
          )}
          {run.status === 'succeeded' && run.mode === 'first_run' && (
            <p role="status">
              Setup finished. Restarting the app to open the new installation…
            </p>
          )}
        </CardContent>
      </Card>
      <Card>
        <CardHeader>
          <CardTitle>Setup log</CardTitle>
          <CardDescription>
            Sanitized live output from this UI session (last 500 lines).
            Reconnection restores step status and failure details; earlier
            output remains in the backend’s per-run log file.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <Suspense fallback={<p role="status">Loading log viewer…</p>}>
            <LogView lines={lines} />
          </Suspense>
        </CardContent>
      </Card>
      {liveReviews && review.key === reviewKey && review.open && <Fragment key={reviewKey}>
        {renderCuratedReview({
          plans: run.curatedReviews,
          busy: pending,
          onCommit: (items) => {
            if (pending || (!!sourceConsent && !approved) || items.length !== run.curatedReviews.length || items.some((item, index) => {
              const plan = run.curatedReviews[index]
              return item.planId !== plan.id || item.binding !== plan.binding ||
                !item.executeNodeCode || (plan.warnings.length > 0 && !item.acknowledgeWarnings)
            })) return
            const decisions = items.map((item) => ({
              ...item,
              runInstallPy: false,
              rememberSource: false,
              rememberInstallPy: false,
            }))
            setReview((current) => ({ ...current, decisions }))
            retry(decisions)
          },
          onClose: () => setReview((current) => ({ ...current, open: false })),
          onCancel: () => cancel.mutate(run.id),
          notice: action.isError ? <ErrorNotice error={action.error} />
            : cancel.isError ? <ErrorNotice error={cancel.error} /> : undefined,
        })}
      </Fragment>}
    </div>
  )
}
