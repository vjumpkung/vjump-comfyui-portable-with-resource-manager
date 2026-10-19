import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  ExternalLink,
  Play,
  RotateCw,
  Square,
  TriangleAlert,
} from 'lucide-react'
import { lazy, Suspense, useState, type ReactNode } from 'react'
import { toast } from 'sonner'

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
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/shared/ui/table'

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

const FAILURE_TITLES: Record<RuntimeFailureDto['kind'], string> = {
  spawn: 'ComfyUI could not be launched',
  exit_before_ready: 'ComfyUI exited before it was ready',
  readiness_timeout: 'ComfyUI did not become ready in time',
  unexpected_exit: 'ComfyUI stopped unexpectedly',
  termination: 'ComfyUI could not be fully stopped',
  cancelled: 'The start was cancelled',
  persistence: 'ComfyUI runtime state could not be saved',
}

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
  return (
    <Alert variant="destructive">
      <TriangleAlert />
      <AlertTitle>{FAILURE_TITLES[failure.kind]}</AlertTitle>
      <AlertDescription>
        <p>
          {failure.message}
          {failure.exitCode !== null ? ` (exit code ${failure.exitCode})` : ''}
        </p>
        <p>
          Check the log below, change the launch options if needed, then start
          again.
        </p>
      </AlertDescription>
    </Alert>
  )
}

function StatusDetails({ status }: { status: ComfyStatusDto }) {
  const session = status.session
  return (
    <dl className="flex flex-col gap-2">
      <Row label="Address">
        {status.origin ??
          (session
            ? `port ${session.options.port}${session.options.listenLan ? ' (all interfaces)' : ''}`
            : '-')}
      </Row>
      <Row label="Process">{session?.pid ?? '-'}</Row>
      <Row label="Version">
        {session ? describeRevision(session.revision) : '-'}
      </Row>
      <Row label="Started">{session?.startedAt ?? '-'}</Row>
    </dl>
  )
}

export function ComfyPage() {
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
  const extraPaths = useQuery({
    queryKey: comfyKeys.extraPaths,
    queryFn: () => gateway.comfyReadExtraModelPaths(),
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
        toast.success('Diagnostics copied to the clipboard.')
      } else {
        setDiagnosticsFallback(result.text)
        toast.error('The clipboard is unavailable. Select the report below.')
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
          Start, stop and watch the local ComfyUI server.
        </p>
      </header>

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-3 text-lg font-bold tracking-tight">
            Status <LifecycleBadge lifecycle={lifecycle} />
          </CardTitle>
          <CardDescription>
            {status?.pendingOperation ??
              'ComfyUI runs from this installation and stops with the app unless you keep it in the background.'}
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          {status?.failure && lifecycle === 'crashed' ? (
            <FailureNotice failure={status.failure} />
          ) : null}
          {status?.recovery ? (
            <Alert variant="destructive">
              <TriangleAlert />
              <AlertTitle>Recovery required</AlertTitle>
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
                Stop
              </Button>
            ) : (
              <Button
                disabled={busy || !status}
                onClick={() =>
                  start.mutate(undefined, { onSuccess: handleStart, onError })
                }
              >
                <Play aria-hidden="true" />
                {lifecycle === 'crashed' ? 'Retry start' : 'Start'}
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
              Restart
            </Button>
            <Button
              variant="outline"
              disabled={lifecycle !== 'running' || open.isPending}
              onClick={() => open.mutate()}
            >
              <ExternalLink aria-hidden="true" />
              Open ComfyUI
            </Button>
            <Button
              variant="outline"
              disabled={lifecycle !== 'running' || openBrowser.isPending}
              onClick={() => openBrowser.mutate()}
            >
              <ExternalLink aria-hidden="true" />
              Open in system browser
            </Button>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-lg font-bold tracking-tight">
            Log
          </CardTitle>
          <CardDescription>
            Live, redacted output. The full per-session log files are kept on
            disk.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <Suspense fallback={<Skeleton className="h-80" />}>
            <StreamLogView text={runtime.log} label="ComfyUI log" />
          </Suspense>
          <div className="flex gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => runtimeStore.clearLog()}
            >
              Clear view
            </Button>
            <Button
              variant="outline"
              size="sm"
              disabled={copyDiagnostics.isPending}
              onClick={() => copyDiagnostics.mutate()}
            >
              Copy diagnostics
            </Button>
          </div>
          {diagnosticsFallback !== null && (
            <textarea
              readOnly
              aria-label="Diagnostics report"
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

      <Card>
        <CardHeader>
          <CardTitle className="text-lg font-bold tracking-tight">
            Extra model paths
          </CardTitle>
          <CardDescription>
            Read from extra_model_paths.yaml in the ComfyUI folder. Read-only.
          </CardDescription>
        </CardHeader>
        <CardContent>
          {extraPaths.data && extraPaths.data.length > 0 ? (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Section</TableHead>
                  <TableHead>Category</TableHead>
                  <TableHead>Path</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {extraPaths.data.map((root) => (
                  <TableRow key={`${root.section}/${root.category}/${root.path}`}>
                    <TableCell>
                      {root.section}
                      {root.isDefault ? ' (default)' : ''}
                    </TableCell>
                    <TableCell>{root.category}</TableCell>
                    <TableCell className="font-mono text-xs break-all">
                      {root.path}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          ) : extraPaths.isError ? (
            <p className="text-sm text-destructive">
              {errorMessage(extraPaths.error)}
            </p>
          ) : (
            <p className="text-sm text-muted-foreground">
              {extraPaths.isPending
                ? 'Loading...'
                : 'No extra model paths are configured.'}
            </p>
          )}
        </CardContent>
      </Card>

      <Dialog
        open={conflict !== null}
        onOpenChange={(next) => {
          if (!next && !acceptPort.isPending) setConflict(null)
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Port {conflict?.requested} is in use</DialogTitle>
            <DialogDescription>
              {conflict?.suggested
                ? `Another program is listening on port ${conflict.requested}. Use port ${conflict.suggested} instead? It will be saved as your launch port.`
                : `Another program is listening on port ${conflict?.requested} and no free port was found after it. Choose a port in the launch options.`}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button
              variant="outline"
              disabled={acceptPort.isPending}
              onClick={() => setConflict(null)}
            >
              Cancel
            </Button>
            {conflict?.suggested ? (
              <Button
                disabled={acceptPort.isPending}
                onClick={() => acceptPort.mutate(conflict.suggested!)}
              >
                Use port {conflict.suggested}
              </Button>
            ) : null}
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
