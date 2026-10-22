import { useMutation } from '@tanstack/react-query'
import { toast } from 'sonner'

import { useTranslation } from '@/shared/i18n'
import { gateway, type CloseChoiceDto } from '@/shared/ipc/gateway'
import { errorMessage } from '@/shared/lib/display'
import { Button } from '@/shared/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shared/ui/dialog'

import { useRuntime } from './use-runtime'

/** Asked when the manager window is closed while ComfyUI runs and the setting is "ask". */
export function CloseRequestDialog() {
  const { t } = useTranslation()
  const { closeRequest } = useRuntime()
  const decide = useMutation({
    mutationFn: (choice: CloseChoiceDto) =>
      gateway.appCloseDecide(closeRequest ?? '', choice),
    onError: (error) => toast.error(errorMessage(error)),
  })
  const open = closeRequest !== null
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next && open) decide.mutate('cancel')
      }}
    >
      <DialogContent className="sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>{t('runtime.close.title')}</DialogTitle>
          <DialogDescription>
            {t('runtime.close.hint')}
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button
            variant="outline"
            disabled={decide.isPending}
            onClick={() => decide.mutate('cancel')}
          >
            {t('common.cancel')}
          </Button>
          <Button
            variant="outline"
            disabled={decide.isPending}
            onClick={() => decide.mutate('keep_running')}
          >
            {t('runtime.close.background')}
          </Button>
          <Button
            disabled={decide.isPending}
            onClick={() => decide.mutate('stop_and_exit')}
          >
            {t('runtime.close.exit')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
