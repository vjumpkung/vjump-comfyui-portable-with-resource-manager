import { useState, type ReactNode } from 'react'

import { useTranslation } from '@/shared/i18n'
import type { SetupStatusDto } from '@/shared/ipc/gateway'
import { Alert, AlertDescription, AlertTitle } from '@/shared/ui/alert'
import { Button } from '@/shared/ui/button'
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from '@/shared/ui/card'
import { ErrorNotice } from '@/shared/ui/feedback'

import type { CuratedReviewProps } from './model'
import { RunProgress } from './run-progress'
import { useSetupRun } from './use-setup-run'

/** Rebuild must remain usable without hardware, profile or environment query services. */
export function RebuildSetup({ status, refresh, renderCuratedReview }: {
  status: SetupStatusDto
  refresh: () => void
  renderCuratedReview: (props: CuratedReviewProps) => ReactNode
}) {
  const { t } = useTranslation()
  const controller = useSetupRun()
  const [started, setStarted] = useState(false)
  const [reason] = useState(status.recovery)
  const [recordedEnv] = useState(status.activeEnv)
  const [trackedRunId, setTrackedRunId] = useState(status.run?.mode === 'rebuild' && status.run.status !== 'succeeded' ? status.run.id : null)
  const existing = status.run?.mode === 'rebuild' && status.run.id === trackedRunId ? status.run : null
  const showProgress = started || trackedRunId !== null
  const restarting = showProgress && (existing?.status === 'succeeded' || controller.finished?.outcome === 'succeeded')
  const busy = controller.action.isPending || (showProgress && !existing) ||
    existing?.status === 'registered' || existing?.status === 'running'
  const active = status.activeEnv
  const recorded = recordedEnv ?? active
  const rebuild = () => {
    if (!active || busy || restarting) return
    controller.action.mutate({
      kind: 'new',
      request: {
        mode: 'rebuild',
        pythonSource: { kind: 'managed' },
        profileId: active.profileId,
        comfyuiRef: { kind: 'keepCurrent' },
        installManager: false,
        accelerators: [],
        curatedNodes: [],
        cleanCache: false,
        confirmAdoptedChanges: false,
        allowSourceBuilds: false,
        thirdPartyConsent: false,
      },
    }, { onSuccess: (result) => {
      setTrackedRunId(result.runId)
      setStarted(true)
    } })
  }
  return <div className="mx-auto flex w-full max-w-5xl flex-col gap-6">
    <Card>
      <CardHeader>
        <CardTitle>{t('setup.mode.rebuild')}</CardTitle>
        <CardDescription>{reason?.code === 'moved'
          ? t('setup.rebuild.moved')
          : reason?.code === 'interpreter_broken'
            ? t('setup.rebuild.interpreterBroken')
            : t('setup.rebuild.restore')}</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        {reason && <p className="break-words [overflow-wrap:anywhere]">{reason.message}</p>}
        <dl className="break-words font-mono text-xs [overflow-wrap:anywhere]">
          <dt>{t('setup.rebuild.currentRoot')}</dt><dd>{status.installRoot}</dd>
          <dt>{t('setup.rebuild.recordedProfile')}</dt><dd>{recorded ? t('setup.rebuild.profileRevision', { profile: recorded.profileId, revision: recorded.profileRevision }) : t('common.unavailable')}</dd>
          <dt>{t('setup.rebuild.recordedPython')}</dt><dd>{recorded?.pythonVersion ?? t('common.unavailable')}</dd>
        </dl>
        <p>{t('setup.rebuild.preserveData')}</p>
        <p>{t('setup.rebuild.replacementHint')}</p>
        <p>{t('setup.rebuild.checkoutHint')}</p>
        <p>{t('setup.rebuild.profileHint')}</p>
        {!active && <Alert variant="destructive"><AlertTitle>{t('setup.rebuild.unavailable')}</AlertTitle>
          <AlertDescription>{t('setup.rebuild.unavailableHint')}</AlertDescription>
        </Alert>}
        {existing && ['failed', 'interrupted'].includes(existing.status) && <p>
          {t('setup.rebuild.retryHint')}
        </p>}
        {controller.action.isError && <ErrorNotice error={controller.action.error} />}
        {restarting && <p role="status">{t('setup.rebuild.restarting')}</p>}
      </CardContent>
      <CardFooter className="flex flex-wrap gap-2">
        <Button disabled={!active || busy || restarting} onClick={rebuild}>{existing ? t('setup.rebuild.startFresh') : t('setup.mode.rebuild')}</Button>
        <Button variant="outline" disabled={restarting} onClick={refresh}>{t('app.recheckInstall')}</Button>
      </CardFooter>
    </Card>
    {showProgress && !restarting && <RunProgress run={existing} controller={controller} refresh={refresh}
      renderCuratedReview={renderCuratedReview} />}
  </div>
}
