import { useState, type ReactNode } from 'react'

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
      },
    }, { onSuccess: (result) => {
      setTrackedRunId(result.runId)
      setStarted(true)
    } })
  }
  return <div className="mx-auto flex w-full max-w-5xl flex-col gap-6">
    <Card>
      <CardHeader>
        <CardTitle>Rebuild environment</CardTitle>
        <CardDescription>{reason?.code === 'moved'
          ? 'This installation has moved. Its Python environment must be rebuilt at the current root.'
          : reason?.code === 'interpreter_broken'
            ? 'The recorded Python interpreter is broken. Rebuild the environment to restore it.'
            : 'Restore the recorded environment using guided rebuild.'}</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        {reason && <p className="break-words [overflow-wrap:anywhere]">{reason.message}</p>}
        <dl className="break-words font-mono text-xs [overflow-wrap:anywhere]">
          <dt>Current root</dt><dd>{status.installRoot}</dd>
          <dt>Recorded profile</dt><dd>{recorded ? `${recorded.profileId} · revision ${recorded.profileRevision}` : 'Unavailable'}</dd>
          <dt>Recorded Python version</dt><dd>{recorded?.pythonVersion ?? 'Unavailable'}</dd>
        </dl>
        <p>Models, custom nodes, user/input/output folders, external model roots and launch options are preserved.</p>
        <p>A fresh app-owned environment replaces the selected environment after verification. Previous and adopted environment files remain untouched.</p>
        <p>The current ComfyUI checkout is retained and node requirements are reinstalled. Setup does not run install.py.</p>
        <p>Recorded profile, Python version and component choices are retained. Rebuild validates the recorded setup before registering a replacement.</p>
        {!active && <Alert variant="destructive"><AlertTitle>Recorded environment unavailable</AlertTitle>
          <AlertDescription>The recorded active environment is required to rebuild. Recheck the installation to recover its metadata.</AlertDescription>
        </Alert>}
        {existing && ['failed', 'interrupted'].includes(existing.status) && <p>
          Inspect or retry the existing rebuild below. The backend checks whether it belongs to the current root; a historical run may be refused. You can also start a fresh rebuild.
        </p>}
        {controller.action.isError && <ErrorNotice error={controller.action.error} />}
        {restarting && <p role="status">Restart required. Rebuild verified; restarting the app…</p>}
      </CardContent>
      <CardFooter className="flex flex-wrap gap-2">
        <Button disabled={!active || busy || restarting} onClick={rebuild}>{existing ? 'Start fresh rebuild' : 'Rebuild environment'}</Button>
        <Button variant="outline" disabled={restarting} onClick={refresh}>Recheck installation</Button>
      </CardFooter>
    </Card>
    {showProgress && !restarting && <RunProgress run={existing} controller={controller} refresh={refresh}
      renderCuratedReview={renderCuratedReview} />}
  </div>
}
