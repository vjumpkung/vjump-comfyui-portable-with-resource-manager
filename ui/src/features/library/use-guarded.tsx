import { useCallback, useState, type ReactNode } from 'react'
import { toast } from 'sonner'

import { IpcClientError } from '@/shared/ipc/gateway'
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

export const LOCKED_CODE = 'model_locked'

interface Pending {
  action: () => Promise<void>
  onSuccess: () => void
}

/**
 * Runs a mutating action. Failures become a toast, except a locked file, which opens the
 * Retry / "Stop ComfyUI and retry" prompt that repeats the same action.
 */
export function useGuarded(onStopComfy: () => Promise<void>): {
  run: (action: () => Promise<void>, onSuccess?: () => void) => Promise<void>
  prompt: ReactNode
} {
  const [pending, setPending] = useState<Pending | null>(null)
  const [busy, setBusy] = useState(false)

  const run = useCallback(
    async (action: () => Promise<void>, onSuccess: () => void = () => undefined) => {
      try {
        await action()
        setPending(null)
        onSuccess()
      } catch (error) {
        if (error instanceof IpcClientError && error.code === LOCKED_CODE) {
          setPending({ action, onSuccess })
        } else {
          setPending(null)
          toast.error(errorMessage(error))
        }
      }
    },
    [],
  )

  async function retry(stopFirst: boolean) {
    if (!pending) return
    setBusy(true)
    try {
      if (stopFirst) await onStopComfy()
      await run(pending.action, pending.onSuccess)
    } catch (error) {
      toast.error(errorMessage(error))
    } finally {
      setBusy(false)
    }
  }

  const prompt = pending ? (
    <LockedDialog
      busy={busy}
      onRetry={() => void retry(false)}
      onStopAndRetry={() => void retry(true)}
      onCancel={() => setPending(null)}
    />
  ) : null
  return { run, prompt }
}

export function LockedDialog({
  busy,
  onRetry,
  onStopAndRetry,
  onCancel,
}: {
  busy: boolean
  onRetry: () => void
  onStopAndRetry: () => void
  onCancel: () => void
}) {
  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) onCancel()
      }}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>The file is in use</DialogTitle>
          <DialogDescription>
            Another program has the file open, so nothing was changed. Close that
            program and retry, or stop ComfyUI first if it is holding the file.
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button variant="outline" disabled={busy} onClick={onCancel}>
            Cancel
          </Button>
          <Button variant="outline" disabled={busy} onClick={onRetry}>
            Retry
          </Button>
          <Button disabled={busy} onClick={onStopAndRetry}>
            Stop ComfyUI and retry
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
