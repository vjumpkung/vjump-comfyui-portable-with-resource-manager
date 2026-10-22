import { useMutation } from '@tanstack/react-query'
import { LoaderCircle } from 'lucide-react'
import { useRef, useState } from 'react'

import { useTranslation } from '@/shared/i18n'
import {
  gateway,
  type EnqueueItemInput,
  type PackPreviewRowDto,
} from '@/shared/ipc/gateway'
import { errorMessage } from '@/shared/lib/display'
import { Alert, AlertDescription } from '@/shared/ui/alert'
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

import { formatSize } from './format'

interface Props {
  onClose: () => void
  onSubmit: (items: EnqueueItemInput[]) => Promise<number>
}

function rowFile(row: PackPreviewRowDto, choice: number | undefined) {
  const files = row.resolution?.files ?? []
  if (choice !== undefined) return files.find((file) => file.index === choice)
  return files.find((file) => file.primary) ?? files[0]
}

/** Preview of a resource list or pack index: sizes first, deselect, then queue. */
export function ImportPackDialog({ onClose, onSubmit }: Props) {
  const { t } = useTranslation()
  const [url, setUrl] = useState('')
  const [text, setText] = useState('')
  const [rows, setRows] = useState<PackPreviewRowDto[] | null>(null)
  const [deselected, setDeselected] = useState<Set<number>>(new Set())
  const [choices, setChoices] = useState<Record<number, number>>({})
  const [submitting, setSubmitting] = useState(false)
  const fileInput = useRef<HTMLInputElement>(null)

  const preview = useMutation({
    mutationFn: (input: { source: 'json' | 'url'; value: string }) =>
      gateway.dlImportPack(input),
    onSuccess: (result) => {
      setRows(result)
      setDeselected(
        new Set(
          result.flatMap((row, index) =>
            row.resolution === null ? [index] : [],
          ),
        ),
      )
      setChoices({})
    },
  })

  const queueable = (rows ?? []).flatMap((row, index) =>
    !deselected.has(index) && rowFile(row, choices[index]) ? [index] : [],
  )
  const totalBytes = queueable.reduce(
    (sum, index) =>
      sum + (rowFile(rows![index], choices[index])?.size.bytes ?? 0),
    0,
  )

  async function onFile(file: File | undefined) {
    if (!file) return
    setText(await file.text())
    setUrl('')
  }

  async function queue() {
    if (!rows) return
    setSubmitting(true)
    try {
      const items = queueable.map<EnqueueItemInput>((index) => {
        const row = rows[index]
        const file = rowFile(row, choices[index])!
        return {
          resolutionId: row.resolution!.id,
          fileIndex: file.index,
          category: row.category,
          subdirectory: '',
          filename: null,
          conflict: null,
          force: false,
        }
      })
      await onSubmit(items)
      onClose()
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) onClose()
      }}
    >
      <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle>{t('downloads.importTitle')}</DialogTitle>
          <DialogDescription>
            {t('downloads.importHint')}
          </DialogDescription>
        </DialogHeader>

        <div className="grid gap-4 sm:grid-cols-2">
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="pack-url">{t('downloads.fromLink')}</Label>
            <Input
              id="pack-url"
              autoComplete="off"
              placeholder="https://example.com/resource_list.json"
              value={url}
              onChange={(event) => {
                setUrl(event.target.value)
                if (event.target.value !== '') setText('')
              }}
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="pack-file">{t('downloads.fromFile')}</Label>
            <Input
              id="pack-file"
              ref={fileInput}
              type="file"
              accept=".json,application/json"
              onChange={(event) => void onFile(event.target.files?.[0])}
            />
          </div>
        </div>
        <div>
          <Button
            disabled={preview.isPending || (url.trim() === '' && text === '')}
            onClick={() =>
              preview.mutate(
                url.trim() !== ''
                  ? { source: 'url', value: url.trim() }
                  : { source: 'json', value: text },
              )
            }
          >
            {preview.isPending ? (
              <LoaderCircle
                className="motion-safe:animate-spin"
                aria-hidden="true"
              />
            ) : null}
            {t('downloads.preview')}
          </Button>
        </div>

        {preview.isError ? (
          <Alert variant="destructive">
            <AlertDescription>{errorMessage(preview.error)}</AlertDescription>
          </Alert>
        ) : null}

        {rows ? (
          <ul
            aria-label={t('downloads.packContents')}
            className="max-h-80 divide-y overflow-y-auto rounded-lg border"
          >
            {rows.map((row, index) => {
              const file = rowFile(row, choices[index])
              const files = row.resolution?.files ?? []
              return (
                <li key={index} className="flex items-start gap-3 px-3 py-2">
                  <Checkbox
                    id={`pack-row-${index}`}
                    aria-label={t('downloads.includeFile', { name: row.name })}
                    disabled={!file}
                    checked={!deselected.has(index) && !!file}
                    onCheckedChange={(value) =>
                      setDeselected((current) => {
                        const next = new Set(current)
                        if (value === true) next.delete(index)
                        else next.add(index)
                        return next
                      })
                    }
                  />
                  <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                    <p className="text-sm font-semibold break-words [overflow-wrap:anywhere]">
                      {row.name}
                    </p>
                    <p className="font-mono text-xs text-muted-foreground">
                      {row.category}
                      {file ? ` · ${file.filename} · ${formatSize(file.size)}` : ''}
                    </p>
                    {row.error ? (
                      <p className="text-xs text-destructive">
                        {row.error.message}
                      </p>
                    ) : null}
                    {files.length > 1 ? (
                      <select
                        aria-label={t('downloads.fileFor', { name: row.name })}
                        className="mt-1 h-8 w-full rounded-md border border-input bg-card px-2 text-xs"
                        value={file?.index ?? 0}
                        onChange={(event) =>
                          setChoices((current) => ({
                            ...current,
                            [index]: Number(event.target.value),
                          }))
                        }
                      >
                        {files.map((candidate) => (
                          <option key={candidate.index} value={candidate.index}>
                            {candidate.filename} ({formatSize(candidate.size)})
                          </option>
                        ))}
                      </select>
                    ) : null}
                  </div>
                </li>
              )
            })}
          </ul>
        ) : null}

        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t('common.cancel')}
          </Button>
          <Button
            disabled={queueable.length === 0 || submitting}
            onClick={() => void queue()}
          >
            {queueable.length > 0
              ? t('downloads.queueCount', { count: queueable.length, size: totalBytes > 0
                  ? formatSize({ kind: 'exact', bytes: totalBytes })
                  : t('downloads.sizeUnknownLowercase') })
              : t('downloads.queue')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
