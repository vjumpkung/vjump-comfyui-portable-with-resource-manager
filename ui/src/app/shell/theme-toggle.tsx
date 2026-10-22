import { Moon, Sun } from 'lucide-react'
import { toast } from 'sonner'

import { useSettings, useSettingsSavePending, useUpdateSettings } from '@/features/settings'
import { t as translate, useTranslation } from '@/shared/i18n'
import { IpcClientError } from '@/shared/ipc/gateway'
import { useTheme } from '@/shared/lib/theme'
import { Button } from '@/shared/ui/button'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shared/ui/tooltip'

export function ThemeToggle() {
  const { t } = useTranslation()
  const { resolved, setSetting } = useTheme()
  const settings = useSettings()
  const update = useUpdateSettings()
  const pending = useSettingsSavePending()
  const next = resolved === 'dark' ? 'light' : 'dark'
  const label = t(next === 'light' ? 'shell.switchToLight' : 'shell.switchToDark')

  function toggle() {
    if (!settings.data || pending) return
    const previous = settings.data.theme
    setSetting(next)
    update.mutate(
      { ...settings.data, theme: next },
      {
        onError: (error) => {
          setSetting(previous)
          toast.error(error instanceof IpcClientError ? error.message : translate('shell.themeSaveError'))
        },
      },
    )
  }

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          variant="ghost"
          size="icon"
          aria-label={label}
          disabled={!settings.data || pending}
          onClick={toggle}
        >
          {resolved === 'dark' ? <Sun aria-hidden="true" /> : <Moon aria-hidden="true" />}
        </Button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  )
}
