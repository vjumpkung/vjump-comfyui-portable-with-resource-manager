import { Fragment, lazy, Suspense, useState, type ReactNode } from 'react'
import { Check, RefreshCw } from 'lucide-react'

import { useTranslation } from '@/shared/i18n'
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

// Stable StepId values and English labels from application/environment/model.rs.
const STEP_LABELS = {
  preflight: 'setup.steps.preflight',
  tools: 'setup.steps.tools',
  python_venv: 'setup.steps.pythonVenv',
  seed: 'setup.steps.seed',
  clone_comfyui: 'setup.steps.cloneComfyui',
  install_torch: 'setup.steps.installTorch',
  verify_torch: 'setup.steps.verifyTorch',
  triton: 'setup.steps.triton',
  accelerators: 'setup.steps.accelerators',
  comfy_requirements: 'setup.steps.comfyRequirements',
  manager: 'setup.installManager',
  curated_nodes: 'setup.steps.curatedNodes',
  final_verify: 'setup.steps.finalVerify',
  cache_clean: 'setup.steps.cacheClean',
} as const satisfies Record<string, keyof typeof import('@/shared/i18n/translations.json').en>

const POLICY_LABELS = {
  required: 'setup.run.required',
  optional: 'setup.run.optional',
  best_effort: 'setup.run.bestEffort',
} as const satisfies Record<RunSnapshotDto['steps'][number]['policy'], keyof typeof import('@/shared/i18n/translations.json').en>

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
  const { t } = useTranslation()
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
    return <p role="status">{t('setup.run.loading')}</p>
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
            {t('setup.run.progress')} <StatusBadge status={run.status} />
          </CardTitle>
          <CardDescription>
            {t('setup.run.description', { id: run.id, target: run.targetEnvId })}
          </CardDescription>
        </CardHeader>
        <CardContent>
          <Progress value={percentage} aria-label={t('setup.run.completedLabel')} />
          <p className="font-mono text-xs">
            {t('setup.run.stepsComplete', { count: completed, total: run.steps.length })}
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
                  {Object.hasOwn(STEP_LABELS, step.step)
                    ? t(STEP_LABELS[step.step as keyof typeof STEP_LABELS])
                    : step.label}
                  <p className="text-xs text-muted-foreground">
                    {t('setup.run.attempt', {
                      policy: Object.hasOwn(POLICY_LABELS, step.policy) ? t(POLICY_LABELS[step.policy]) : step.policy,
                      attempts: step.attempts,
                    })}
                    {step.message ? ` · ${step.message}` : ''}
                  </p>
                </div>
                <StatusBadge status={step.status} />
              </li>
            ))}
          </ol>
          {run.curated.length > 0 && <section aria-label={t('setup.run.curatedProgress')}>
            <h2 className="font-bold">{t('setup.curatedNodes')}</h2>
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
              <AlertTitle>{t('setup.run.stepFailed', { step: failure.step })}</AlertTitle>
              <AlertDescription>
                <p>{failure.message}</p>
                <p className="break-all font-mono">
                  {t('setup.run.command', {
                    command: failure.command ?? t('common.notAvailable'),
                    code: failure.exitCode ?? t('common.notAvailable'),
                  })}
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
                {t('setup.run.managerSourceConsent')}
              </AlertTitle>
              <AlertDescription>
                <p>
                  {t('setup.run.managerSourceHint')}
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
                    {t('setup.run.trustCommit')}
                  </FieldLabel>
                </Field>
              </AlertDescription>
            </Alert>
          )}
          {finished?.runId === run.id && finished.outcome === 'error' && (
            <Alert variant="destructive">
              <AlertTitle>{t('setup.run.workflowError')}</AlertTitle>
              <AlertDescription>
                {t('setup.run.workflowErrorHint')}
              </AlertDescription>
            </Alert>
          )}
          {action.isError && <ErrorNotice error={action.error} />}
          {cancel.isError && <ErrorNotice error={cancel.error} />}
          {curatedPause && !liveReviews && <Alert>
            <AlertTitle>{t('setup.run.freshReviews')}</AlertTitle>
            <AlertDescription>
              {t('setup.run.freshReviewsHint')}
            </AlertDescription>
          </Alert>}
          <div className="flex flex-wrap gap-2">
            {liveReviews && <Button variant="outline" disabled={pending}
              onClick={() => setReview((current) => ({ ...current, open: true }))}>
              {t('setup.run.reviewPlans')}
            </Button>}
            {retryable && (
              <Button
                disabled={pending || (!!sourceConsent && !approved) || (liveReviews && !curatedApproved)}
                onClick={() => retry(liveReviews ? review.decisions! : [])}
              >
                {t('setup.run.retry')}
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
                {t('setup.run.skipOptional')}
              </Button>
            )}
            {(busy || retryable) && (
              <Button
                variant="destructive"
                disabled={pending || (busy && cancel.isSuccess)}
                onClick={() => cancel.mutate(run.id)}
              >
                {busy && (cancel.isPending || cancel.isSuccess)
                  ? t('setup.run.cancelling')
                  : t('setup.run.abort')}
              </Button>
            )}
            <Button variant="outline" onClick={refresh}>
              <RefreshCw data-icon="inline-start" />
              {t('setup.run.reconnect')}
            </Button>
            {!busy && onDone && (
              <Button variant="outline" onClick={onDone}>
                <Check data-icon="inline-start" />
                {run.status === 'succeeded'
                  ? t('common.continue')
                  : t('setup.run.backEnvironment')}
              </Button>
            )}
          </div>
          {run.status === 'cancelled' && (
            <p>
              {t('setup.run.cancelledHint')}
            </p>
          )}
          {run.status === 'succeeded' && (
            <p role="status">
              {t('setup.run.verified')}
            </p>
          )}
          {run.status === 'succeeded' && run.mode === 'first_run' && (
            <p role="status">
              {t('setup.run.restarting')}
            </p>
          )}
        </CardContent>
      </Card>
      <Card>
        <CardHeader>
          <CardTitle>{t('setup.run.log')}</CardTitle>
          <CardDescription>
            {t('setup.run.logHint')}
          </CardDescription>
        </CardHeader>
        <CardContent>
          <Suspense fallback={<p role="status">{t('setup.run.loadingLog')}</p>}>
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
