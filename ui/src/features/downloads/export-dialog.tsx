import { useMutation } from '@tanstack/react-query'
import { Copy } from 'lucide-react'
import { useEffect } from 'react'
import { toast } from 'sonner'

import { t as translate, useTranslation } from '@/shared/i18n'
import { gateway } from '@/shared/ipc/gateway'
import { errorMessage } from '@/shared/lib/display'
import { Alert, AlertDescription } from '@/shared/ui/alert'
import { Button } from '@/shared/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shared/ui/dialog'

/** Completed downloads as resource_list JSON, ready to copy or save. */
export function ExportDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation()
  const exported = useMutation({
    mutationFn: () => gateway.dlExport({ jobIds: null }),
  })
  const { mutate } = exported
  useEffect(() => mutate(), [mutate])

  const data = exported.data

  async function copy() {
    if (!data) return
    try {
      await navigator.clipboard.writeText(data.json)
      toast.success(translate('downloads.copied'))
    } catch {
      toast.error(translate('downloads.clipboardUnavailable'))
    }
  }

  function save() {
    if (!data) return
    const url = URL.createObjectURL(
      new Blob([data.json], { type: 'application/json' }),
    )
    const link = document.createElement('a')
    link.href = url
    link.download = 'resource_list.json'
    link.click()
    URL.revokeObjectURL(url)
  }

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) onClose()
      }}
    >
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>{t('downloads.exportTitle')}</DialogTitle>
          <DialogDescription>
            {t('downloads.exportHint')}
          </DialogDescription>
        </DialogHeader>
        {exported.isError ? (
          <Alert variant="destructive">
            <AlertDescription>{errorMessage(exported.error)}</AlertDescription>
          </Alert>
        ) : null}
        {data ? (
          <>
            <textarea
              readOnly
              aria-label={t('downloads.resourceJson')}
              className="h-64 w-full rounded-md border border-input bg-card p-2 font-mono text-xs"
              value={data.json}
            />
            {data.excluded.length > 0 ? (
              <p className="text-xs text-muted-foreground">
                {t('downloads.excluded', { count: data.excluded.length })}
              </p>
            ) : null}
          </>
        ) : null}
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t('common.close')}
          </Button>
          <Button variant="outline" disabled={!data} onClick={() => void copy()}>
            <Copy aria-hidden="true" />
            {t('common.copy')}
          </Button>
          <Button disabled={!data} onClick={save}>
            {t('downloads.saveFile')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
