import { QueryClientProvider } from '@tanstack/react-query'
import { useEffect, useState } from 'react'

import { useSettings } from '@/features/settings'
import { SetupWizard } from '@/features/setup'
import type { SetupModeDto } from '@/shared/ipc/gateway'
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

function Workspace() {
  const status = useSetupStatus()
  const [wizard, setWizard] = useState<{
    mode: SetupModeDto
    resume: boolean
  } | null>(null)
  const refresh = () => {
    void status.refetch()
  }
  const data = status.data
  const gated = !data?.activeEnv || data.phase === 'run_in_progress'
  const resume =
    !!data?.run &&
    ['registered', 'running', 'failed', 'interrupted'].includes(data.run.status)
  const showWizard = data && (gated || wizard)
  if (
    status.isPending ||
    status.isError ||
    data?.phase === 'recovery_required' ||
    showWizard
  ) {
    return (
      <div className="flex min-h-screen flex-col bg-background text-foreground">
        <header className="flex h-14 shrink-0 items-center justify-between border-b px-6">
          <span className="font-mono text-xs">ComfyUI environment setup</span>
          <ThemeToggle />
        </header>
        <main className="flex flex-col gap-6 p-6">
          {status.isPending ? (
            <Skeleton className="h-64" />
          ) : status.isError ? (
            <ErrorNotice error={status.error} retry={refresh} />
          ) : data?.phase === 'recovery_required' ? (
            <Alert variant="destructive">
              <AlertTitle>Install recovery required</AlertTitle>
              <AlertDescription>
                <p>
                  {data.recovery?.message ??
                    'The installation manifest and environment database need attention.'}
                </p>
                <p className="break-all font-mono text-xs">
                  {data.recovery?.code} · {data.installRoot}
                </p>
                <p>
                  Setup is blocked to preserve this installation. Restore a
                  consistent manifest/database or reopen the app at the intended
                  root. Automatic moved-install rebuild is deferred to M6.
                </p>
                <Button variant="outline" onClick={refresh}>
                  Recheck installation
                </Button>
              </AlertDescription>
            </Alert>
          ) : (
            data && (
              <SetupWizard
                key={wizard ? 'requested' : 'automatic'}
                status={data}
                mode={wizard?.mode ?? (resume ? data.run!.mode : 'first_run')}
                resume={wizard?.resume ?? resume}
                onClose={() => setWizard(null)}
                refresh={refresh}
              />
            )
          )}
        </main>
      </div>
    )
  }
  if (!data) return null
  return (
    <AppShell
      status={data}
      onSetup={(mode) => setWizard({ mode, resume: false })}
      onResume={() => {
        if (data.run) setWizard({ mode: data.run.mode, resume: true })
      }}
    />
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
          <ThemeSync />
          <Workspace />
          <Toaster />
        </TooltipProvider>
      </ThemeProvider>
    </QueryClientProvider>
  )
}
