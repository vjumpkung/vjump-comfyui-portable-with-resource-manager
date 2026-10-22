import { useState } from 'react'

import { useTranslation } from '@/shared/i18n'
import type { GalleryEntryDto } from '@/shared/ipc/gateway'
import { Button } from '@/shared/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shared/ui/dialog'
import { Input } from '@/shared/ui/input'
import { Label } from '@/shared/ui/label'

import { entryRefs, extensionOf } from './format'
import { inlineError, useRenameEntry } from './use-gallery'

interface Props {
  entry: GalleryEntryDto
  onClose: () => void
}

/** Renames within the same folder. Only the backend decides which names are acceptable. */
export function RenameDialog({ entry, onClose }: Props) {
  const { t } = useTranslation()
  const rename = useRenameEntry()
  const [name, setName] = useState(entry.name)
  const trimmed = name.trim()
  const ready = trimmed !== '' && trimmed !== entry.name
  const failure = inlineError(rename.error)
  const extension = entry.kind === 'file' ? extensionOf(entry.name) : ''

  function submit() {
    rename.mutate(
      { entry: entryRefs([entry])[0], newName: trimmed },
      { onSuccess: onClose },
    )
  }

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open && !rename.isPending) onClose()
      }}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t('common.rename')}</DialogTitle>
          <DialogDescription>{t('gallery.rename.hint')}</DialogDescription>
        </DialogHeader>
        <form
          className="flex flex-col gap-4"
          onSubmit={(event) => {
            event.preventDefault()
            if (ready && !rename.isPending) submit()
          }}
        >
          <div className="flex flex-col gap-2">
            <Label htmlFor="gallery-rename">{t('gallery.rename.newName')}</Label>
            <Input
              id="gallery-rename"
              value={name}
              autoFocus
              aria-invalid={failure !== null}
              onChange={(event) => setName(event.target.value)}
              onFocus={(event) => {
                const stem = event.target.value.length - extension.length
                event.target.setSelectionRange(0, Math.max(0, stem))
              }}
            />
            {failure ? (
              <p role="alert" className="text-sm text-destructive">
                {failure}
              </p>
            ) : null}
          </div>
          <DialogFooter>
            <Button type="button" variant="outline" disabled={rename.isPending} onClick={onClose}>
              {t('common.cancel')}
            </Button>
            <Button type="submit" disabled={!ready || rename.isPending}>
              {t('common.rename')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
