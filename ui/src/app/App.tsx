import { QueryClientProvider, useMutation } from '@tanstack/react-query'
import { useEffect, useRef, useState, type ReactNode } from 'react'

import { ReviewDialog } from '@/features/custom-nodes'
import { useSettings } from '@/features/settings'
import { RebuildSetup, type CuratedReviewProps } from '@/features/setup'
import { setLanguage, useTranslation } from '@/shared/i18n'
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
import { SetupWizardPreferences } from './setup-wizard-preferences'

function CuratedReview(props: CuratedReviewProps) {
  const { t } = useTranslation()
  return <ReviewDialog {...props} title={t('app.reviewCuratedNodes')}
    commitLabel={t('app.approveRetry')} closeLabel={t('app.backToSetup')} allowRememberSource={false} />
}

function renderCuratedReview(props: CuratedReviewProps) {
  return <CuratedReview {...props} />
}

function SetupFrame({ children, showThemeToggle = false }: { children: ReactNode; showThemeToggle?: boolean }) {
  const { t } = useTranslation()
  return <div className="flex min-h-screen flex-col bg-background text-foreground">
    <header className="flex h-14 shrink-0 items-center justify-between border-b px-6">
      <span className="font-mono text-xs">{t('app.setupTitle')}</span>
      {showThemeToggle && <ThemeToggle />}
    </header>
    <main className="flex flex-col gap-6 p-6">{children}</main>
  </div>
}

const rootErrors = {
  root_missing: 'app.rootMissing',
  root_invalid: 'app.rootInvalid',
  root_fixed_by_override: 'app.rootFixed',
  root_switch_failed: 'app.rootSwitchFailed',
  comfyui_running: 'app.comfyRunning',
  comfyui_state_unknown: 'app.comfyUnknown',
  maintenance_busy: 'app.maintenanceBusy',
  relocation_recovery_required: 'app.relocationRecovery',
} as const satisfies Record<string, keyof typeof import('@/shared/i18n/translations.json').en>

function RootRecovery({ status, refresh }: { status: SetupStatusDto; refresh: () => void }) {
  const { t } = useTranslation()
  const locked = useRef(false)
  const selection = useMutation({
    mutationFn: async (kind: 'existing' | 'new') => {
      const path = await gateway.pickFolder(kind === 'existing' ? t('app.locateInstall') : t('app.chooseNewFolder'))
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
    <AlertTitle>{t('app.recoveryRequired')}</AlertTitle>
    <AlertDescription>
      <p>{status.recovery?.message}</p>
      <p className="break-words font-mono text-xs [overflow-wrap:anywhere]">
        {status.recovery?.code} · {t('app.recordedRoot', { root: status.installRoot })}
      </p>
      {status.recovery?.code === 'root_inaccessible' && <p>
        {t('app.rootAccessHint')}
      </p>}
      <p>{t('app.selectInstallHint')}</p>
      <p>{t('app.recheckHint')}</p>
      {restarting && <p role="status">{t('app.restartingRoot')}</p>}
      {selection.isError && <div role="alert">
        <p>{t(code && Object.hasOwn(rootErrors, code) ? rootErrors[code as keyof typeof rootErrors] : 'app.selectionError')}</p>
        <p className="break-words [overflow-wrap:anywhere]">{code && `${code}: `}{errorMessage(selection.error)}</p>
      </div>}
      <div className="flex flex-wrap gap-2">
        <Button disabled={busy} onClick={() => choose('existing')}>{t('app.locateInstall')}</Button>
        <Button disabled={busy} onClick={() => choose('new')}>{t('app.createInstall')}</Button>
        <Button variant="outline" disabled={busy} onClick={refresh}>{t('app.recheckInstall')}</Button>
      </div>
    </AlertDescription>
  </Alert>
}

function Workspace() {
  const { t } = useTranslation()
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
        <AlertTitle>{t('app.recoveryRequired')}</AlertTitle>
        <AlertDescription>
          <p>{data.recovery?.message ?? t('app.manifestAttention')}</p>
          <p className="break-words font-mono text-xs [overflow-wrap:anywhere]">{data.recovery?.code} · {data.installRoot}</p>
          <p>{t('app.setupBlocked')}</p>
          <Button variant="outline" onClick={refresh}>{t('app.recheckInstall')}</Button>
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
    <LanguageSync />
    <SetupWizardPreferences key={wizard ? 'requested' : 'automatic'} status={data}
      mode={wizard?.mode ?? (resume ? data.run!.mode : 'first_run')}
      resume={wizard?.resume ?? resume} onClose={() => setWizard(null)} refresh={refresh}
      renderCuratedReview={renderCuratedReview} />
  </SetupFrame>
  return (
    <><ThemeSync /><LanguageSync />
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

/** Reconciles the first-paint language cache with the stored settings. */
function LanguageSync() {
  const settings = useSettings()
  const stored = settings.data?.language

  useEffect(() => {
    if (stored) setLanguage(stored)
  }, [stored])

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
