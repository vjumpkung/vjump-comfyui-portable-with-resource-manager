import { LoaderCircle } from 'lucide-react'
import { useEffect, useState } from 'react'

import { useTranslation } from '@/shared/i18n'
import { gateway, type GalleryDeletePlanDto, type GalleryEntryDto } from '@/shared/ipc/gateway'
import { Button } from '@/shared/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shared/ui/dialog'

import { baseName, entryRefs, formatBytes, keyOf } from './format'
import {
  galleryErrorMessage,
  isBusy,
  useDeleteEntries,
  useRefreshGallery,
} from './use-gallery'

type Stage =
  | { kind: 'planning' }
  | { kind: 'confirm'; plan: GalleryDeletePlanDto }
  | { kind: 'error'; error: unknown }

const LISTED = 8

interface Props {
  entries: readonly GalleryEntryDto[]
  onClose: () => void
}

/**
 * Plans first, then deletes exactly what was planned. The plan is bound to the revisions the
 * grid showed, so a folder that changed underneath is refused instead of deleted blindly.
 */
export function DeleteDialog({ entries, onClose }: Props) {
  const { t } = useTranslation()
  const [stage, setStage] = useState<Stage>({ kind: 'planning' })
  const remove = useDeleteEntries()
  const refresh = useRefreshGallery()

  useEffect(() => {
    let live = true
    gateway
      .galleryDeletePlan(entryRefs(entries))
      .then((plan) => {
        if (live) setStage({ kind: 'confirm', plan })
      })
      .catch((error: unknown) => {
        if (!live) return
        setStage({ kind: 'error', error })
        void refresh()
      })
    return () => {
      live = false
    }
    // The selection is fixed for the lifetime of the dialog.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  function execute(plan: GalleryDeletePlanDto) {
    remove.mutate(plan.planId, {
      onSuccess: onClose,
      // A busy backend is toasted and leaves the plan in place to confirm again.
      onError: (error) => {
        if (!isBusy(error)) setStage({ kind: 'error', error })
      },
    })
  }

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open && !remove.isPending) onClose()
      }}
    >
      <DialogContent>
        {stage.kind === 'planning' ? (
          <>
            <DialogHeader>
              <DialogTitle>{t('gallery.delete.title', { count: entries.length })}</DialogTitle>
              <DialogDescription>{t('gallery.delete.checking')}</DialogDescription>
            </DialogHeader>
            <LoaderCircle className="animate-spin" aria-label={t('common.loadingLabel')} />
          </>
        ) : null}

        {stage.kind === 'confirm' ? (
          <>
            <DialogHeader>
              <DialogTitle>{t('gallery.delete.title', { count: stage.plan.items.length })}</DialogTitle>
              <DialogDescription>
                {t('gallery.delete.hint', { size: formatBytes(stage.plan.totalBytes) })}
              </DialogDescription>
            </DialogHeader>
            <ul
              className="flex max-h-48 flex-col gap-1 overflow-auto text-sm"
              aria-label={t('gallery.delete.files')}
            >
              {stage.plan.items.slice(0, LISTED).map((item) => (
                <li key={keyOf(item.entry.location)} className="flex justify-between gap-3">
                  <span className="min-w-0 truncate" title={baseName(item.entry.location.path)}>
                    {baseName(item.entry.location.path)}
                  </span>
                  <span className="shrink-0 font-mono text-xs text-muted-foreground">
                    {formatBytes(item.bytes)}
                  </span>
                </li>
              ))}
              {stage.plan.items.length > LISTED ? (
                <li className="text-muted-foreground">
                  {t('gallery.more', { count: stage.plan.items.length - LISTED })}
                </li>
              ) : null}
            </ul>
            <DialogFooter>
              <Button variant="outline" disabled={remove.isPending} onClick={onClose}>
                {t('common.cancel')}
              </Button>
              <Button
                variant="destructive"
                disabled={remove.isPending}
                onClick={() => execute(stage.plan)}
              >
                {remove.isPending ? (
                  <LoaderCircle className="animate-spin" aria-hidden="true" />
                ) : null}
                {t('common.delete')}
              </Button>
            </DialogFooter>
          </>
        ) : null}

        {stage.kind === 'error' ? (
          <>
            <DialogHeader>
              <DialogTitle>{t('gallery.delete.failed')}</DialogTitle>
              <DialogDescription>{galleryErrorMessage(stage.error)}</DialogDescription>
            </DialogHeader>
            <DialogFooter>
              <Button variant="outline" onClick={onClose}>
                {t('common.close')}
              </Button>
            </DialogFooter>
          </>
        ) : null}
      </DialogContent>
    </Dialog>
  )
}
