import { useTranslation } from '@/shared/i18n'
import type { OperationDto } from '@/shared/ipc/gateway'
import { Button } from '@/shared/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shared/ui/dialog'

import { KIND_LABEL } from './operations-panel'

export function ResolveDialog({ operation, busy, onConfirm, onClose }: {
  operation: OperationDto
  busy: boolean
  onConfirm: () => void
  onClose: () => void
}) {
  const { t } = useTranslation()
  return (
    <Dialog open onOpenChange={(open) => { if (!open && !busy) onClose() }}>
      <DialogContent showCloseButton={!busy}>
        <DialogHeader>
          <DialogTitle>
            {t('customNodes.resolve.title', { action: t(KIND_LABEL[operation.kind]).toLowerCase(), name: operation.nodeName })}
          </DialogTitle>
          <DialogDescription>{t('customNodes.resolve.body')}</DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button variant="outline" disabled={busy} onClick={onClose}>{t('common.cancel')}</Button>
          <Button disabled={busy} onClick={onConfirm}>
            {busy ? t('customNodes.resolve.working') : t('customNodes.resolve.confirm')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
