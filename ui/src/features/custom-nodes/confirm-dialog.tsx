import type { InstalledNodeDto } from '@/shared/ipc/gateway'
import { Button } from '@/shared/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shared/ui/dialog'

import { trashName } from './model'

export type NodeConfirmation = 'enable' | 'disable' | 'uninstall'

/** Direct changes have no cancellation key, so the dialog states exactly what will move. */
export function ConfirmDialog({ node, action, busy, onConfirm, onClose }: {
  node: InstalledNodeDto
  action: NodeConfirmation
  busy: boolean
  onConfirm: () => void
  onClose: () => void
}) {
  const trash = trashName()
  const copy = {
    enable: {
      title: `Enable ${node.name}?`,
      body: `Moves the folder out of custom_nodes/.disabled back into custom_nodes. Resulting state: enabled. ComfyUI loads it at its next start.`,
      confirm: 'Enable node',
    },
    disable: {
      title: `Disable ${node.name}?`,
      body: `Moves the folder into custom_nodes/.disabled. Resulting state: disabled. ComfyUI stops loading it; nothing is deleted.`,
      confirm: 'Disable node',
    },
    uninstall: {
      title: `Uninstall ${node.name}?`,
      body: `Moves the folder to the ${trash}. Python packages that were installed for it remain in the shared environment; they are not removed. Nothing is deleted permanently.`,
      confirm: 'Uninstall node',
    },
  }[action]
  return (
    <Dialog open onOpenChange={(open) => { if (!open && !busy) onClose() }}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{copy.title}</DialogTitle>
          <DialogDescription>{copy.body}</DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button variant="outline" disabled={busy} onClick={onClose}>Cancel</Button>
          <Button variant={action === 'uninstall' ? 'destructive' : 'default'} disabled={busy} onClick={onConfirm}>
            {busy ? 'Working…' : copy.confirm}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
