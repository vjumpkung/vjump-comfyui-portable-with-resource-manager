import { Folder, LoaderCircle } from 'lucide-react'
import { useState } from 'react'

import { useTranslation } from '@/shared/i18n'
import type { GalleryAreaDto, GalleryEntryDto } from '@/shared/ipc/gateway'
import { Button } from '@/shared/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shared/ui/dialog'

import { AreaToggle, Breadcrumb } from './folder-nav'
import { entryRefs, keyOf } from './format'
import {
  galleryErrorMessage,
  inlineError,
  useGalleryListing,
  useTransferEntries,
  type TransferMode,
} from './use-gallery'

interface Props {
  mode: TransferMode
  entries: readonly GalleryEntryDto[]
  /** Where the browser starts: the folder the entries are listed in. */
  area: GalleryAreaDto
  path: string
  onClose: () => void
}

/**
 * Moves or copies the entries into the folder being browsed. Only folders are listed; the
 * destination is always the browsed folder itself, never a highlighted child.
 */
export function TransferDialog({
  mode,
  entries,
  area: startArea,
  path: startPath,
  onClose,
}: Props) {
  const { t } = useTranslation()
  const [area, setArea] = useState(startArea)
  const [path, setPath] = useState(startPath)
  const listing = useGalleryListing(area, path)
  const transfer = useTransferEntries(mode)
  const folders = (listing.data?.entries ?? []).filter((entry) => entry.kind === 'directory')
  const failure = inlineError(transfer.error)

  function confirm() {
    transfer.mutate(
      { items: entryRefs(entries), destination: { area, path } },
      { onSuccess: onClose },
    )
  }

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open && !transfer.isPending) onClose()
      }}
    >
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>
            {mode === 'move'
              ? t('gallery.move.title', { count: entries.length })
              : t('gallery.copy.title', { count: entries.length })}
          </DialogTitle>
          <DialogDescription>{t('gallery.transfer.hint')}</DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-3">
          <AreaToggle
            area={area}
            onChange={(next) => {
              setArea(next)
              setPath('')
            }}
          />
          <Breadcrumb area={area} path={path} onNavigate={setPath} />
          <ul
            aria-label={t('gallery.transfer.folders')}
            className="flex max-h-56 min-h-24 flex-col gap-1 overflow-auto rounded-md border p-1"
          >
            {listing.isPending ? (
              <li role="status" className="flex items-center gap-2 p-2 text-sm text-muted-foreground">
                <LoaderCircle className="size-4 animate-spin" aria-hidden="true" />
                {t('gallery.loading')}
              </li>
            ) : listing.isError ? (
              <li role="alert" className="p-2 text-sm text-destructive">
                {galleryErrorMessage(listing.error)}
              </li>
            ) : folders.length === 0 ? (
              <li className="p-2 text-sm text-muted-foreground">{t('gallery.transfer.noFolders')}</li>
            ) : (
              folders.map((folder) => (
                <li key={keyOf(folder.location)}>
                  <button
                    type="button"
                    onDoubleClick={() => setPath(folder.location.path)}
                    onKeyDown={(event) => {
                      if (event.key === 'Enter') {
                        event.preventDefault()
                        setPath(folder.location.path)
                      }
                    }}
                    className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-brand-cyan/50"
                  >
                    <Folder className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
                    <span className="min-w-0 truncate" title={folder.name}>
                      {folder.name}
                    </span>
                  </button>
                </li>
              ))
            )}
          </ul>
          {failure ? (
            <p role="alert" className="text-sm text-destructive">
              {failure}
            </p>
          ) : null}
        </div>
        <DialogFooter>
          <Button variant="outline" disabled={transfer.isPending} onClick={onClose}>
            {t('common.cancel')}
          </Button>
          <Button disabled={listing.data === undefined || transfer.isPending} onClick={confirm}>
            {transfer.isPending ? <LoaderCircle className="animate-spin" aria-hidden="true" /> : null}
            {mode === 'move' ? t('gallery.move.confirm') : t('gallery.copy.confirm')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
