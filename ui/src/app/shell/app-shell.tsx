import { useState } from 'react'

import { CloseRequestDialog, ComfyPage } from '@/features/comfy-runtime'
import { DownloadsPage } from '@/features/downloads'
import { LibraryPage } from '@/features/library'
import { OverviewPage } from '@/features/overview'
import { SettingsPage } from '@/features/settings'
import { EnvironmentPage } from '@/features/environment'
import {
  gateway,
  type SetupModeDto,
  type SetupStatusDto,
} from '@/shared/ipc/gateway'
import { EmptyState } from '@/shared/ui/empty-state'

import { NAV_ITEMS, type NavId } from './nav'
import { Sidebar } from './sidebar'
import { TopBar } from './top-bar'

/** Features cannot import each other, so the shell hands the library a way to stop ComfyUI. */
async function stopComfy(): Promise<void> {
  await gateway.comfyStop()
}

interface PageActions {
  status: SetupStatusDto
  onSetup: (mode: SetupModeDto) => void
  onResume: () => void
}

interface Navigation {
  onOpenSettings: () => void
}

function Page({
  active,
  status,
  onSetup,
  onResume,
  onOpenSettings,
}: PageActions & Navigation & { active: NavId }) {
  if (active === 'overview') return <OverviewPage />
  if (active === 'comfyui') return <ComfyPage />
  if (active === 'downloads')
    return <DownloadsPage onOpenSettings={onOpenSettings} />
  if (active === 'models') return <LibraryPage onStopComfy={stopComfy} />
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
  const item = NAV_ITEMS.find((candidate) => candidate.id === active)
  return (
    <div className="flex flex-col gap-6">
      <h1 className="text-2xl font-extrabold tracking-tighter sm:text-3xl">
        {item?.label}
      </h1>
      <EmptyState title="Coming in a later milestone" />
      <CloseRequestDialog />
    </div>
  )
}

export function AppShell({ status, onSetup, onResume }: PageActions) {
  const [active, setActive] = useState<NavId>('overview')

  return (
    <div className="flex h-screen bg-background text-foreground">
      <Sidebar active={active} onSelect={setActive} />
      <div className="flex min-w-0 flex-1 flex-col">
        <TopBar />
        <main className="flex-1 overflow-y-auto p-6">
          <Page
            active={active}
            status={status}
            onSetup={onSetup}
            onResume={onResume}
            onOpenSettings={() => setActive('settings')}
          />
        </main>
      </div>
      <CloseRequestDialog />
    </div>
  )
}
