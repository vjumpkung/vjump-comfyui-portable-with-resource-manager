import { useTranslation } from '@/shared/i18n'
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
  const { t } = useTranslation()
  const trash = trashName()
  const copy = {
    enable: {
      title: t('customNodes.confirm.enableTitle', { name: node.name }),
      body: t('customNodes.confirm.enableBody'),
      confirm: t('customNodes.confirm.enable'),
    },
    disable: {
      title: t('customNodes.confirm.disableTitle', { name: node.name }),
      body: t('customNodes.confirm.disableBody'),
      confirm: t('customNodes.confirm.disable'),
    },
    uninstall: {
      title: t('customNodes.confirm.uninstallTitle', { name: node.name }),
      body: t('customNodes.confirm.uninstallBody', { trash }),
      confirm: t('customNodes.confirm.uninstall'),
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
          <Button variant="outline" disabled={busy} onClick={onClose}>{t('common.cancel')}</Button>
          <Button variant={action === 'uninstall' ? 'destructive' : 'default'} disabled={busy} onClick={onConfirm}>
            {busy ? t('customNodes.confirm.working') : copy.confirm}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
