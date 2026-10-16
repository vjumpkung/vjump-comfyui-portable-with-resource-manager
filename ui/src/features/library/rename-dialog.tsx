import { TriangleAlert } from 'lucide-react'
import { useState } from 'react'

import type { ModelEntryDto } from '@/shared/ipc/gateway'
import { Alert, AlertDescription, AlertTitle } from '@/shared/ui/alert'
import { Button } from '@/shared/ui/button'
import { Checkbox } from '@/shared/ui/checkbox'
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

/** The extension including its dot, or an empty string (mirrors the backend rule). */
export function extensionOf(name: string): string {
  const dot = name.lastIndexOf('.')
  return dot <= 0 ? '' : name.slice(dot)
}

interface Props {
  model: ModelEntryDto
  onClose: () => void
  onSubmit: (newName: string, allowExtensionChange: boolean) => Promise<void>
}

/** Renames within the same folder; a changed extension needs an explicit acknowledgement. */
export function RenameDialog({ model, onClose, onSubmit }: Props) {
  const [name, setName] = useState(model.name)
  const [acknowledged, setAcknowledged] = useState(false)
  const [busy, setBusy] = useState(false)
  const trimmed = name.trim()
  const changedExtension = extensionOf(trimmed) !== extensionOf(model.name)
  const ready =
    trimmed !== '' &&
    trimmed !== model.name &&
    (!changedExtension || acknowledged)

  async function submit() {
    setBusy(true)
    try {
      await onSubmit(trimmed, changedExtension && acknowledged)
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
          <DialogTitle>Rename model</DialogTitle>
          <DialogDescription>
            The file stays in its folder. Preview, CivitAI and JSON files that
            share its name are renamed with it.
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
            <Label htmlFor="rename-input">New name</Label>
            <Input
              id="rename-input"
              value={name}
              autoFocus
              onChange={(event) => setName(event.target.value)}
              onFocus={(event) => {
                const stem = event.target.value.length - extensionOf(model.name).length
                event.target.setSelectionRange(0, Math.max(0, stem))
              }}
            />
          </div>
          {changedExtension && trimmed !== '' ? (
            <Alert variant="destructive">
              <TriangleAlert aria-hidden="true" />
              <AlertTitle>The file extension changes</AlertTitle>
              <AlertDescription>
                ComfyUI may stop recognizing the model. Change it only if you are
                sure.
                <span className="mt-2 flex items-center gap-2">
                  <Checkbox
                    id="rename-extension-ack"
                    checked={acknowledged}
                    onCheckedChange={(checked) => setAcknowledged(checked === true)}
                  />
                  <Label htmlFor="rename-extension-ack">
                    Change the extension anyway
                  </Label>
                </span>
              </AlertDescription>
            </Alert>
          ) : null}
          <DialogFooter>
            <Button type="button" variant="outline" disabled={busy} onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" disabled={!ready || busy}>
              Rename
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
