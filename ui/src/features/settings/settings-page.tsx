import { CredentialsPanel } from './credentials-panel'
import { SettingsForm } from './settings-form'
import type { SetupModeDto } from '@/shared/ipc/gateway'
import { Button } from '@/shared/ui/button'
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/shared/ui/card'

export function SettingsPage({
  onSetup,
  maintenanceBusy,
}: {
  onSetup: (mode: SetupModeDto) => void
  maintenanceBusy: boolean
}) {
  return (
    <div className="flex flex-col gap-6">
      <header className="flex flex-col gap-1">
        <h1 className="text-2xl font-extrabold tracking-tighter sm:text-3xl">
          Settings
        </h1>
        <p className="text-sm font-medium leading-relaxed text-muted-foreground">
          Downloads, appearance and access tokens.
        </p>
      </header>
      <SettingsForm />
      <CredentialsPanel />
      <Card>
        <CardHeader>
          <CardTitle>Environment setup</CardTitle>
          <CardDescription>
            Re-run setup with ComfyUI stopped. Replacements are activated after
            verification; the previous environment and user data are retained.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="flex flex-wrap gap-2">
            <Button
              variant="outline"
              disabled={maintenanceBusy}
              onClick={() => onSetup('repair')}
            >
              Repair
            </Button>
            <Button
              variant="outline"
              disabled={maintenanceBusy}
              onClick={() => onSetup('change_torch')}
            >
              Change torch / CUDA
            </Button>
            <Button
              variant="outline"
              disabled={maintenanceBusy}
              onClick={() => onSetup('change_python')}
            >
              Change Python
            </Button>
            <Button
              variant="outline"
              disabled={maintenanceBusy}
              onClick={() => onSetup('switch_venv')}
            >
              Switch to custom venv
            </Button>
          </div>
        </CardContent>
      </Card>
    </div>
  )
}
