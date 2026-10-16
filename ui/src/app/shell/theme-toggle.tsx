import { Moon, Sun } from 'lucide-react'
import { toast } from 'sonner'

import { useSettings, useUpdateSettings } from '@/features/settings'
import { IpcClientError } from '@/shared/ipc/gateway'
import { useTheme } from '@/shared/lib/theme'
import { Button } from '@/shared/ui/button'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shared/ui/tooltip'

export function ThemeToggle() {
  const { resolved, setSetting } = useTheme()
  const settings = useSettings()
  const update = useUpdateSettings()
  const next = resolved === 'dark' ? 'light' : 'dark'
  const label = `Switch to ${next} theme`

  function toggle() {
    if (!settings.data) return
    const previous = settings.data.theme
    setSetting(next)
    update.mutate(
      { ...settings.data, theme: next },
      {
        onError: (error) => {
          setSetting(previous)
          toast.error(error instanceof IpcClientError ? error.message : 'Could not save the theme.')
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
          disabled={!settings.data || update.isPending}
          onClick={toggle}
        >
          {resolved === 'dark' ? <Sun aria-hidden="true" /> : <Moon aria-hidden="true" />}
        </Button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  )
}
