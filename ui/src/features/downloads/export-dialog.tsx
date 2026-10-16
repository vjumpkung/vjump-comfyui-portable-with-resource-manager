import { useMutation } from '@tanstack/react-query'
import { Copy } from 'lucide-react'
import { useEffect } from 'react'
import { toast } from 'sonner'

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
      toast.success('Copied to the clipboard.')
    } catch {
      toast.error('The clipboard is not available. Select the text and copy it.')
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
          <DialogTitle>Export resource list</DialogTitle>
          <DialogDescription>
            Completed downloads with stable provider links, in the
            resource_list format.
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
              aria-label="Resource list JSON"
              className="h-64 w-full rounded-md border border-input bg-card p-2 font-mono text-xs"
              value={data.json}
            />
            {data.excluded.length > 0 ? (
              <p className="text-xs text-muted-foreground">
                {data.excluded.length} download
                {data.excluded.length === 1 ? ' was' : 's were'} left out because
                their signed links cannot be reused.
              </p>
            ) : null}
          </>
        ) : null}
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Close
          </Button>
          <Button variant="outline" disabled={!data} onClick={() => void copy()}>
            <Copy aria-hidden="true" />
            Copy
          </Button>
          <Button disabled={!data} onClick={save}>
            Save as file
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
