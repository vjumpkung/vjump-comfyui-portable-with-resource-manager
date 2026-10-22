import { useState } from 'react'

import {
  CloseRequestDialog,
  ComfyPage,
  useRuntime,
  useRuntimeActions,
} from '@/features/comfy-runtime'
import { CustomNodesPage } from '@/features/custom-nodes'
import { DownloadsPage } from '@/features/downloads'
import { GalleryPage } from '@/features/gallery'
import { LibraryPage } from '@/features/library'
import { OverviewPage } from '@/features/overview'
import { SettingsPage } from '@/features/settings'
import { EnvironmentPage } from '@/features/environment'
import {
  gateway,
  type SetupModeDto,
  type SetupStatusDto,
} from '@/shared/ipc/gateway'
import { useTranslation } from '@/shared/i18n'
import { EmptyState } from '@/shared/ui/empty-state'

import { type NavId } from './nav'
import { Sidebar } from './sidebar'
import { TopBar } from './top-bar'

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
  if (active === 'downloads')
    return <DownloadsPage onOpenSettings={onOpenSettings} />
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
    return (
      <SettingsPage
        onSetup={onSetup}
        maintenanceBusy={status.phase === 'run_in_progress'}
      />
    )
  if (active === 'environment')
    return (
      <EnvironmentPage status={status} onSetup={onSetup} onResume={onResume} />
    )
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
          <Page
            active={active}
            status={status}
            onSetup={onSetup}
            onResume={onResume}
            onOpenSettings={() => setActive('settings')}
            onOpenComfy={() => setActive('comfyui')}
          />
        </main>
      </div>
      <CloseRequestDialog />
    </div>
  )
}
