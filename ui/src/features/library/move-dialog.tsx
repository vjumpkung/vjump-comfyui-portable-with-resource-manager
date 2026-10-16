import { useState } from 'react'

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
          <DialogTitle>Move model</DialogTitle>
          <DialogDescription>
            Moves {model.name} and its metadata files. Nothing is overwritten;
            missing folders are created.
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
            <Label htmlFor="move-root">Model folder</Label>
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
              {needsCategory ? 'Category and sub-folder' : 'Sub-folder'}
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
                ? 'The first part is the category folder, for example loras/characters.'
                : `This folder is fixed to the ${root?.category ?? ''} category.`}
            </p>
          </div>
          <DialogFooter>
            <Button type="button" variant="outline" disabled={busy} onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" disabled={!ready || busy}>
              Move
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
