import { useMutation, useQueryClient } from '@tanstack/react-query'
import {
  ExternalLink,
  Play,
  RotateCw,
  Square,
  TriangleAlert,
} from 'lucide-react'
import { lazy, Suspense, useState, type ReactNode } from 'react'
import { toast } from 'sonner'

import { t as translate, useTranslation } from '@/shared/i18n'
import {
  gateway,
  type ComfyStatusDto,
  type RuntimeFailureDto,
  type StartResultDto,
} from '@/shared/ipc/gateway'
import { errorMessage } from '@/shared/lib/display'
import { Alert, AlertDescription, AlertTitle } from '@/shared/ui/alert'
import { Button } from '@/shared/ui/button'
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/shared/ui/card'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shared/ui/dialog'
import { Skeleton } from '@/shared/ui/skeleton'

import { ExtraModelPathsCard } from './extra-model-paths-card'
import { LaunchOptionsCard } from './launch-options'
import { LifecycleBadge } from './lifecycle-badge'
import { runtimeStore } from './runtime-store'
import { comfyKeys, useRuntime, useRuntimeActions } from './use-runtime'
import { VersionsPanel, describeRevision } from './versions-panel'

// The terminal library loads only when the page opens.
const StreamLogView = lazy(() =>
  import('@/shared/ui/log-view').then((module) => ({
    default: module.StreamLogView,
  })),
)

const FAILURE_TITLE_KEYS = {
  spawn: 'runtime.failure.spawn',
  exit_before_ready: 'runtime.failure.exitBeforeReady',
  readiness_timeout: 'runtime.failure.timeout',
  unexpected_exit: 'runtime.failure.unexpectedExit',
  termination: 'runtime.failure.termination',
  cancelled: 'runtime.failure.cancelled',
  persistence: 'runtime.failure.persistence',
} as const satisfies Record<RuntimeFailureDto['kind'], keyof typeof import('@/shared/i18n/translations.json').en>

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-1 sm:flex-row sm:items-baseline sm:gap-4">
      <dt className="w-32 shrink-0 font-mono text-xs font-semibold uppercase tracking-wider text-muted-foreground">
        {label}
      </dt>
      <dd className="font-mono text-xs break-words [overflow-wrap:anywhere]">
        {children}
      </dd>
    </div>
  )
}

function FailureNotice({ failure }: { failure: RuntimeFailureDto }) {
  const { t } = useTranslation()
  return (
    <Alert variant="destructive">
      <TriangleAlert />
      <AlertTitle>{t(FAILURE_TITLE_KEYS[failure.kind])}</AlertTitle>
      <AlertDescription>
        <p>
          {failure.message}
          {failure.exitCode !== null ? t('runtime.failure.exitCode', { code: failure.exitCode }) : ''}
        </p>
        <p>
          {t('runtime.failure.hint')}
        </p>
      </AlertDescription>
    </Alert>
  )
}

function StatusDetails({ status }: { status: ComfyStatusDto }) {
  const { t } = useTranslation()
  const session = status.session
  return (
    <dl className="flex flex-col gap-2">
      <Row label={t('runtime.address')}>
        {status.origin ??
          (session
            ? t(session.options.listenLan ? 'runtime.portAll' : 'runtime.port', { port: session.options.port })
            : '-')}
      </Row>
      <Row label={t('runtime.process')}>{session?.pid ?? '-'}</Row>
      <Row label={t('common.version')}>
        {session ? describeRevision(session.revision) : '-'}
      </Row>
      <Row label={t('runtime.started')}>{session?.startedAt ?? '-'}</Row>
    </dl>
  )
}

export function ComfyPage() {
  const { t } = useTranslation()
  const client = useQueryClient()
  const runtime = useRuntime()
  const { start, restart, stop } = useRuntimeActions()
  const [conflict, setConflict] = useState<{
    requested: number
    suggested: number | null
  } | null>(null)
  const status = runtime.status
  const lifecycle = status?.lifecycle
  const active = lifecycle === 'starting' || lifecycle === 'running'
  const transitioning = lifecycle === 'stopping'
  const busy =
    start.isPending || restart.isPending || stop.isPending || transitioning

  const handleStart = (result: StartResultDto) => {
    if (result.kind === 'port_conflict') {
      setConflict({ requested: result.requested, suggested: result.suggested })
    }
  }
  const onError = (error: unknown) => toast.error(errorMessage(error))

  const acceptPort = useMutation({
    mutationFn: async (port: number) => {
      const current = await gateway.comfyGetLaunchOptions()
      const saved = await gateway.comfySetLaunchOptions({ ...current, port })
      client.setQueryData(comfyKeys.launchOptions, saved)
      return gateway.comfyStart()
    },
    onSuccess: (result) => {
      setConflict(null)
      handleStart(result)
    },
    onError: (error) => {
      setConflict(null)
      onError(error)
    },
    onSettled: () => void runtimeStore.refresh().catch(() => undefined),
  })

  const open = useMutation({
    mutationFn: () => gateway.comfyOpenWindow(),
    onError,
  })
  const openBrowser = useMutation({
    mutationFn: () => gateway.comfyOpenBrowser(),
    onError,
  })
  // The report to select by hand when the clipboard could not be written.
  const [diagnosticsFallback, setDiagnosticsFallback] = useState<string | null>(
    null,
  )
  const copyDiagnostics = useMutation({
    mutationFn: () => gateway.appCopyDiagnostics(),
    onSuccess: (result) => {
      if (result.copied) {
        setDiagnosticsFallback(null)
        toast.success(translate('runtime.diagnosticsCopied'))
      } else {
        setDiagnosticsFallback(result.text)
        toast.error(translate('runtime.clipboardUnavailable'))
      }
    },
    onError,
  })

  return (
    <div className="flex flex-col gap-6">
      <header className="flex flex-col gap-1">
        <h1 className="text-2xl font-extrabold tracking-tighter sm:text-3xl">
          ComfyUI
        </h1>
        <p className="text-sm font-medium leading-relaxed text-muted-foreground">
          {t('runtime.description')}
        </p>
      </header>

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-3 text-lg font-bold tracking-tight">
            {t('common.status')} <LifecycleBadge lifecycle={lifecycle} />
          </CardTitle>
          <CardDescription>
            {status?.pendingOperation ??
              t('runtime.statusHint')}
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          {status?.failure && lifecycle === 'crashed' ? (
            <FailureNotice failure={status.failure} />
          ) : null}
          {status?.recovery ? (
            <Alert variant="destructive">
              <TriangleAlert />
              <AlertTitle>{t('runtime.recovery')}</AlertTitle>
              <AlertDescription>{status.recovery}</AlertDescription>
            </Alert>
          ) : null}
          {status ? <StatusDetails status={status} /> : null}
          <div className="flex flex-wrap gap-2">
            {active ? (
              <Button
                variant="outline"
                disabled={stop.isPending || transitioning}
                onClick={() => stop.mutate(undefined, { onError })}
              >
                <Square aria-hidden="true" />
                {t('common.stop')}
              </Button>
            ) : (
              <Button
                disabled={busy || !status}
                onClick={() =>
                  start.mutate(undefined, { onSuccess: handleStart, onError })
                }
              >
                <Play aria-hidden="true" />
                {lifecycle === 'crashed' ? t('runtime.retryStart') : t('common.start')}
              </Button>
            )}
            <Button
              variant="outline"
              disabled={busy || !status || lifecycle === 'starting'}
              onClick={() =>
                restart.mutate(undefined, { onSuccess: handleStart, onError })
              }
            >
              <RotateCw aria-hidden="true" />
              {t('common.restart')}
            </Button>
            <Button
              variant="outline"
              disabled={lifecycle !== 'running' || open.isPending}
              onClick={() => open.mutate()}
            >
              <ExternalLink aria-hidden="true" />
              {t('runtime.open')}
            </Button>
            <Button
              variant="outline"
              disabled={lifecycle !== 'running' || openBrowser.isPending}
              onClick={() => openBrowser.mutate()}
            >
              <ExternalLink aria-hidden="true" />
              {t('runtime.openBrowser')}
            </Button>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-lg font-bold tracking-tight">
            {t('runtime.log')}
          </CardTitle>
          <CardDescription>
            {t('runtime.logHint')}
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <Suspense fallback={<Skeleton className="h-80" />}>
            <StreamLogView text={runtime.log} label={t('runtime.comfyLog')} />
          </Suspense>
          <div className="flex gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => runtimeStore.clearLog()}
            >
              {t('runtime.clearView')}
            </Button>
            <Button
              variant="outline"
              size="sm"
              disabled={copyDiagnostics.isPending}
              onClick={() => copyDiagnostics.mutate()}
            >
              {t('runtime.copyDiagnostics')}
            </Button>
          </div>
          {diagnosticsFallback !== null && (
            <textarea
              readOnly
              aria-label={t('runtime.diagnosticsReport')}
              value={diagnosticsFallback}
              rows={12}
              onFocus={(event) => event.currentTarget.select()}
              className="w-full rounded-md border bg-muted p-3 font-mono text-xs"
            />
          )}
        </CardContent>
      </Card>

      <LaunchOptionsCard active={status?.session ?? null} />
      <VersionsPanel />

      <ExtraModelPathsCard running={active} />

      <Dialog
        open={conflict !== null}
        onOpenChange={(next) => {
          if (!next && !acceptPort.isPending) setConflict(null)
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('runtime.portInUse', { port: conflict?.requested ?? '' })}</DialogTitle>
            <DialogDescription>
              {conflict?.suggested
                ? t('runtime.portSuggestion', { requested: conflict.requested, suggested: conflict.suggested })
                : t('runtime.noFreePort', { port: conflict?.requested ?? '' })}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button
              variant="outline"
              disabled={acceptPort.isPending}
              onClick={() => setConflict(null)}
            >
              {t('common.cancel')}
            </Button>
            {conflict?.suggested ? (
              <Button
                disabled={acceptPort.isPending}
                onClick={() => acceptPort.mutate(conflict.suggested!)}
              >
                {t('runtime.usePort', { port: conflict.suggested })}
              </Button>
            ) : null}
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
