import { useState } from 'react'

import { useTranslation } from '@/shared/i18n'
import type { ModelEntryDto, RootDto } from '@/shared/ipc/gateway'
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

interface Props {
  model: ModelEntryDto
  roots: readonly RootDto[]
  categories: readonly string[]
  onClose: () => void
  onSubmit: (rootId: string, directory: string) => Promise<void>
}

function currentDirectory(model: ModelEntryDto): string {
  const slash = model.location.path.lastIndexOf('/')
  return slash < 0 ? '' : model.location.path.slice(0, slash)
}

/** Moves the model and its metadata files to another folder or root (same name). */
export function MoveDialog({ model, roots, categories, onClose, onSubmit }: Props) {
  const { t } = useTranslation()
  const usable = roots.filter((root) => root.state === 'ready')
  const [rootId, setRootId] = useState(model.location.rootId)
  const [directory, setDirectory] = useState(currentDirectory(model))
  const [busy, setBusy] = useState(false)
  const root = usable.find((candidate) => candidate.id === rootId)
  // The default root holds category folders, so a target needs a category first.
  const needsCategory = root !== undefined && root.category === null
  const ready =
    root !== undefined &&
    (directory.trim() !== '' || root.category !== null) &&
    (rootId !== model.location.rootId || directory.trim() !== currentDirectory(model))

  async function submit() {
    setBusy(true)
    try {
      await onSubmit(rootId, directory.trim().replaceAll('\\', '/'))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open && !busy) onClose()
      }}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t('library.move.title')}</DialogTitle>
          <DialogDescription>
            {t('library.move.hint', { name: model.name })}
          </DialogDescription>
        </DialogHeader>
        <form
          className="flex flex-col gap-4"
          onSubmit={(event) => {
            event.preventDefault()
            if (ready && !busy) void submit()
          }}
        >
          <div className="flex flex-col gap-2">
            <Label htmlFor="move-root">{t('library.modelFolder')}</Label>
            <select
              id="move-root"
              value={rootId}
              onChange={(event) => setRootId(event.target.value)}
              className="h-9 w-full rounded-md border border-input bg-card px-2.5 text-sm outline-none focus-visible:border-brand-cyan focus-visible:ring-2 focus-visible:ring-brand-cyan/35"
            >
              {usable.map((candidate) => (
                <option key={candidate.id} value={candidate.id}>
                  {candidate.label}
                </option>
              ))}
            </select>
          </div>
          <div className="flex flex-col gap-2">
            <Label htmlFor="move-directory">
              {needsCategory ? t('library.move.categorySubfolder') : t('library.move.subfolder')}
            </Label>
            <Input
              id="move-directory"
              value={directory}
              list="move-categories"
              placeholder={needsCategory ? 'checkpoints/sdxl' : 'sdxl'}
              onChange={(event) => setDirectory(event.target.value)}
            />
            {needsCategory ? (
              <datalist id="move-categories">
                {categories.map((category) => (
                  <option key={category} value={category} />
                ))}
              </datalist>
            ) : null}
            <p className="text-xs text-muted-foreground">
              {needsCategory
                ? t('library.move.categoryHint')
                : t('library.move.fixedCategory', { category: root?.category ?? '' })}
            </p>
          </div>
          <DialogFooter>
            <Button type="button" variant="outline" disabled={busy} onClick={onClose}>
              {t('common.cancel')}
            </Button>
            <Button type="submit" disabled={!ready || busy}>
              {t('common.move')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
