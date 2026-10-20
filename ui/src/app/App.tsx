import { QueryClientProvider, useMutation } from '@tanstack/react-query'
import { useEffect, useRef, useState, type ReactNode } from 'react'

import { ReviewDialog } from '@/features/custom-nodes'
import { useSettings } from '@/features/settings'
import { RebuildSetup, SetupWizard, type CuratedReviewProps } from '@/features/setup'
import { gateway, IpcClientError, type SetupModeDto, type SetupStatusDto } from '@/shared/ipc/gateway'
import { errorMessage } from '@/shared/lib/display'
import { useSetupStatus } from '@/shared/lib/environment-queries'
import { queryClient } from '@/shared/lib/query'
import { ThemeProvider, useTheme } from '@/shared/lib/theme'
import { Toaster } from '@/shared/ui/sonner'
import { TooltipProvider } from '@/shared/ui/tooltip'
import { Alert, AlertDescription, AlertTitle } from '@/shared/ui/alert'
import { Button } from '@/shared/ui/button'
import { ErrorNotice } from '@/shared/ui/feedback'
import { Skeleton } from '@/shared/ui/skeleton'

import { AppShell } from './shell/app-shell'
import { ThemeToggle } from './shell/theme-toggle'

function renderCuratedReview(props: CuratedReviewProps) {
  return <ReviewDialog {...props} title="Review curated setup nodes"
    commitLabel="Approve and retry setup" closeLabel="Back to setup" allowRememberSource={false} />
}

function SetupFrame({ children, showThemeToggle = false }: { children: ReactNode; showThemeToggle?: boolean }) {
  return <div className="flex min-h-screen flex-col bg-background text-foreground">
    <header className="flex h-14 shrink-0 items-center justify-between border-b px-6">
      <span className="font-mono text-xs">ComfyUI environment setup</span>
      {showThemeToggle && <ThemeToggle />}
    </header>
    <main className="flex flex-col gap-6 p-6">{children}</main>
  </div>
}

const rootErrors: Record<string, string> = {
  root_missing: 'The selected install folder is missing. Choose an available folder.',
  root_invalid: 'That folder is not a valid installation or a suitable new install location.',
  root_fixed_by_override: 'The install root is fixed by a debug override. Remove the override and restart the app.',
  root_switch_failed: 'The install root could not be changed. Check the selected folder and its permissions, then try again.',
  comfyui_running: 'Stop ComfyUI before changing the installation.',
  comfyui_state_unknown: 'The app cannot confirm that ComfyUI is stopped. Close external ComfyUI processes and try again.',
  maintenance_busy: 'Another maintenance operation is running. Wait for it to finish and try again.',
  relocation_recovery_required: 'The selected installation needs relocation recovery before it can be opened.',
}

function RootRecovery({ status, refresh }: { status: SetupStatusDto; refresh: () => void }) {
  const locked = useRef(false)
  const selection = useMutation({
    mutationFn: async (kind: 'existing' | 'new') => {
      const path = await gateway.pickFolder(kind === 'existing' ? 'Locate existing install' : 'Choose new install folder')
      if (!path) return null
      return kind === 'existing' ? gateway.setupOpenInstall(path) : gateway.setupNewInstall(path)
    },
    onSettled: (result) => {
      if (!result?.restartRequired) locked.current = false
    },
  })
  const restarting = selection.data?.restartRequired === true
  const busy = selection.isPending || restarting
  const choose = (kind: 'existing' | 'new') => {
    if (locked.current) return
    locked.current = true
    selection.mutate(kind)
  }
  const code = selection.error instanceof IpcClientError ? selection.error.code : null
  return <Alert variant="destructive">
    <AlertTitle>Install recovery required</AlertTitle>
    <AlertDescription>
      <p>{status.recovery?.message}</p>
      <p className="break-words font-mono text-xs [overflow-wrap:anywhere]">
        {status.recovery?.code} · Recorded install root: {status.installRoot}
      </p>
      {status.recovery?.code === 'root_inaccessible' && <p>
        Check that the drive is connected and that your account has permission to read and write this folder.
      </p>}
      <p>Locate an existing installation or choose a folder for a new installation.</p>
      <p>If the saved root is available again at its original path, restart the app. Recheck updates the report; it cannot reopen backend services.</p>
      {restarting && <p role="status">Restart required. Restarting with the selected install root…</p>}
      {selection.isError && <div role="alert">
        <p>{(code && rootErrors[code]) || 'The installation could not be selected. Try again.'}</p>
        <p className="break-words [overflow-wrap:anywhere]">{code && `${code}: `}{errorMessage(selection.error)}</p>
      </div>}
      <div className="flex flex-wrap gap-2">
        <Button disabled={busy} onClick={() => choose('existing')}>Locate existing install</Button>
        <Button disabled={busy} onClick={() => choose('new')}>Create new install</Button>
        <Button variant="outline" disabled={busy} onClick={refresh}>Recheck installation</Button>
      </div>
    </AlertDescription>
  </Alert>
}

function Workspace() {
  const status = useSetupStatus()
  const [wizard, setWizard] = useState<{
    mode: SetupModeDto
    resume: boolean
  } | null>(null)
  const [rebuildMounted, setRebuildMounted] = useState(false)
  const refresh = () => {
    void status.refetch()
  }
  const data = status.data
  const rebuildEntry = data?.phase === 'rebuild_required' ||
    (data?.run?.mode === 'rebuild' && ['registered', 'running', 'failed', 'interrupted', 'cancelled'].includes(data.run.status))
  // Once entered, retain this route through the ready poll before the native restart.
  if (rebuildEntry && !rebuildMounted) setRebuildMounted(true)
  const gated = !data?.activeEnv || data.phase === 'run_in_progress'
  const resume =
    !!data?.run &&
    ['registered', 'running', 'failed', 'interrupted'].includes(data.run.status)
  const showWizard = data && (gated || wizard)
  if (status.isPending) return <SetupFrame><Skeleton className="h-64" /></SetupFrame>
  if (status.isError && !data) return <SetupFrame><ErrorNotice error={status.error} retry={refresh} /></SetupFrame>
  if (!data) return null
  if (data.phase === 'recovery_required') {
    const rootless = data.recovery?.code === 'root_missing' || data.recovery?.code === 'root_inaccessible'
    return <SetupFrame>{rootless ? <RootRecovery status={data} refresh={refresh} /> :
      <Alert variant="destructive">
        <AlertTitle>Install recovery required</AlertTitle>
        <AlertDescription>
          <p>{data.recovery?.message ?? 'The installation manifest and environment database need attention.'}</p>
          <p className="break-words font-mono text-xs [overflow-wrap:anywhere]">{data.recovery?.code} · {data.installRoot}</p>
          <p>Setup is blocked to preserve this installation. Follow the recovery guidance before continuing.</p>
          <Button variant="outline" onClick={refresh}>Recheck installation</Button>
        </AlertDescription>
      </Alert>}
    </SetupFrame>
  }
  if (rebuildEntry || rebuildMounted) return <SetupFrame>
    <RebuildSetup status={data} refresh={refresh} renderCuratedReview={renderCuratedReview} />
    {status.isError && <ErrorNotice error={status.error} retry={refresh} />}
  </SetupFrame>
  if (showWizard) return <SetupFrame showThemeToggle>
    <ThemeSync />
    <SetupWizard key={wizard ? 'requested' : 'automatic'} status={data}
      mode={wizard?.mode ?? (resume ? data.run!.mode : 'first_run')}
      resume={wizard?.resume ?? resume} onClose={() => setWizard(null)} refresh={refresh}
      renderCuratedReview={renderCuratedReview} />
  </SetupFrame>
  return (
    <><ThemeSync />
    <AppShell
      status={data}
      onSetup={(mode) => setWizard({ mode, resume: false })}
      onResume={() => {
        if (data.run) setWizard({ mode: data.run.mode, resume: true })
      }}
    />
    </>
  )
}

/** Reconciles the first-paint theme cache with the stored settings once they load. */
function ThemeSync() {
  const settings = useSettings()
  const { setSetting } = useTheme()
  const stored = settings.data?.theme

  useEffect(() => {
    if (stored) setSetting(stored)
  }, [stored, setSetting])

  return null
}

export function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <ThemeProvider>
        <TooltipProvider>
          <Workspace />
          <Toaster />
        </TooltipProvider>
      </ThemeProvider>
    </QueryClientProvider>
  )
}
