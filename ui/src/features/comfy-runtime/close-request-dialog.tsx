import { useMutation } from '@tanstack/react-query'
import { toast } from 'sonner'

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
          <DialogTitle>ComfyUI is still active</DialogTitle>
          <DialogDescription>
            Stop ComfyUI and exit, or keep it running in the background. In the
            background the app stays alive without a window; opening the app
            again restores these controls and the live log. The default can be
            changed in Settings.
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button
            variant="outline"
            disabled={decide.isPending}
            onClick={() => decide.mutate('cancel')}
          >
            Cancel
          </Button>
          <Button
            variant="outline"
            disabled={decide.isPending}
            onClick={() => decide.mutate('keep_running')}
          >
            Keep running in background
          </Button>
          <Button
            disabled={decide.isPending}
            onClick={() => decide.mutate('stop_and_exit')}
          >
            Stop ComfyUI and exit
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
