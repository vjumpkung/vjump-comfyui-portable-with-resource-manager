import { lazy, Suspense, useState } from 'react'

import { CloseRequestDialog } from '@/features/comfy-runtime/close-request-dialog'
import { useRuntime, useRuntimeActions } from '@/features/comfy-runtime/use-runtime'
import { gateway, type SetupModeDto, type SetupStatusDto } from '@/shared/ipc/gateway'
import { useTranslation } from '@/shared/i18n'
import { EmptyState } from '@/shared/ui/empty-state'
import { Skeleton } from '@/shared/ui/skeleton'

import { type NavId } from './nav'
import { Sidebar } from './sidebar'
import { TopBar } from './top-bar'

const ComfyPage = lazy(() =>
  import('@/features/comfy-runtime/comfy-page').then((m) => ({ default: m.ComfyPage })),
)
const CustomNodesPage = lazy(() =>
  import('@/features/custom-nodes/custom-nodes-page').then((m) => ({ default: m.CustomNodesPage })),
)
const DownloadsPage = lazy(() =>
  import('@/features/downloads/downloads-page').then((m) => ({ default: m.DownloadsPage })),
)
const GalleryPage = lazy(() =>
  import('@/features/gallery/gallery-page').then((m) => ({ default: m.GalleryPage })),
)
const LibraryPage = lazy(() =>
  import('@/features/library/library-page').then((m) => ({ default: m.LibraryPage })),
)
const OverviewPage = lazy(() =>
  import('@/features/overview/overview-page').then((m) => ({ default: m.OverviewPage })),
)
const SettingsPage = lazy(() =>
  import('@/features/settings/settings-page').then((m) => ({ default: m.SettingsPage })),
)
const EnvironmentPage = lazy(() =>
  import('@/features/environment/environment-page').then((m) => ({ default: m.EnvironmentPage })),
)

/** Features cannot import each other, so the shell hands the library a way to stop ComfyUI. */
async function stopComfy(): Promise<void> {
  await gateway.comfyStop()
}

/** Features cannot import each other, so the shell wires the runtime facade into Custom Nodes. */
function CustomNodesRoute({
  maintenanceBusy,
  onRepair,
  onOpenComfy,
}: {
  maintenanceBusy: boolean
  onRepair: () => void
  onOpenComfy: () => void
}) {
  const runtime = useRuntime()
  const { start, stop, restart } = useRuntimeActions()
  return (
    <CustomNodesPage
      maintenanceBusy={maintenanceBusy}
      runtime={runtime.status}
      runtimeBusy={start.isPending || stop.isPending || restart.isPending}
      onRepair={onRepair}
      onStopComfy={async () => {
        await stop.mutateAsync()
      }}
      onRestartComfy={() => restart.mutateAsync()}
      onOpenComfy={onOpenComfy}
    />
  )
}

interface PageActions {
  status: SetupStatusDto
  onSetup: (mode: SetupModeDto) => void
  onResume: () => void
}

interface Navigation {
  onOpenSettings: () => void
  onOpenComfy: () => void
}

function Page({
  active,
  status,
  onSetup,
  onResume,
  onOpenSettings,
  onOpenComfy,
}: PageActions & Navigation & { active: NavId }) {
  const { t } = useTranslation()
  if (active === 'overview') return <OverviewPage />
  if (active === 'comfyui') return <ComfyPage />
  if (active === 'downloads') return <DownloadsPage onOpenSettings={onOpenSettings} />
  if (active === 'models') return <LibraryPage onStopComfy={stopComfy} />
  if (active === 'custom-nodes')
    return (
      <CustomNodesRoute
        maintenanceBusy={status.phase === 'run_in_progress'}
        onRepair={() => onSetup('repair')}
        onOpenComfy={onOpenComfy}
      />
    )
  if (active === 'gallery') return <GalleryPage />
  if (active === 'settings')
    return <SettingsPage onSetup={onSetup} maintenanceBusy={status.phase === 'run_in_progress'} />
  if (active === 'environment')
    return <EnvironmentPage status={status} onSetup={onSetup} onResume={onResume} />
  return <EmptyState title={t('app.pageUnavailable')} />
}

export function AppShell({ status, onSetup, onResume }: PageActions) {
  const [active, setActive] = useState<NavId>('overview')

  return (
    <div className="relative flex h-screen overflow-hidden bg-background text-foreground">
      <Sidebar active={active} onSelect={setActive} />
      <div className="flex min-h-0 min-w-0 flex-1 flex-col">
        <TopBar />
        <main className="relative min-h-0 flex-1 overflow-y-auto p-6">
          <Suspense fallback={<Skeleton className="h-64" />}>
            <Page
              active={active}
              status={status}
              onSetup={onSetup}
              onResume={onResume}
              onOpenSettings={() => setActive('settings')}
              onOpenComfy={() => setActive('comfyui')}
            />
          </Suspense>
        </main>
      </div>
      <CloseRequestDialog />
    </div>
  )
}
