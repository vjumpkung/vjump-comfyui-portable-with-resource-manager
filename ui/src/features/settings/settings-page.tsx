import { useTranslation } from '@/shared/i18n'
import { CredentialsPanel } from './credentials-panel'
import { DiagnosticsPanel } from './diagnostics-panel'
import { SettingsForm } from './settings-form'
import { UpdatesPanel } from './updates-panel'
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
  const { t } = useTranslation()
  return (
    <div className="flex flex-col gap-6">
      <header className="flex flex-col gap-1">
        <h1 className="text-2xl font-extrabold tracking-tighter sm:text-3xl">
          {t('nav.settings')}
        </h1>
        <p className="text-sm font-medium leading-relaxed text-muted-foreground">
          {t('settings.description')}
        </p>
      </header>
      <SettingsForm />
      <CredentialsPanel />
      <Card>
        <CardHeader>
          <CardTitle>{t('settings.environmentSetup')}</CardTitle>
          <CardDescription>
            {t('settings.environmentHint')}
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="flex flex-wrap gap-2">
            <Button
              variant="outline"
              disabled={maintenanceBusy}
              onClick={() => onSetup('repair')}
            >
              {t('common.repair')}
            </Button>
            <Button
              variant="outline"
              disabled={maintenanceBusy}
              onClick={() => onSetup('change_torch')}
            >
              {t('setup.mode.changeTorch')}
            </Button>
            <Button
              variant="outline"
              disabled={maintenanceBusy}
              onClick={() => onSetup('change_python')}
            >
              {t('setup.mode.changePython')}
            </Button>
            <Button
              variant="outline"
              disabled={maintenanceBusy}
              onClick={() => onSetup('switch_venv')}
            >
              {t('setup.mode.switchVenv')}
            </Button>
          </div>
        </CardContent>
      </Card>
      <UpdatesPanel maintenanceBusy={maintenanceBusy} />
      <DiagnosticsPanel />
    </div>
  )
}
