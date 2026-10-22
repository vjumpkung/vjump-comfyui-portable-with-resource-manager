import { useMutation } from '@tanstack/react-query'
import { Link2, LoaderCircle } from 'lucide-react'
import { useState } from 'react'

import { t as translate, useTranslation } from '@/shared/i18n'
import {
  gateway,
  IpcClientError,
  type EnqueueItemInput,
  type ResolutionDto,
} from '@/shared/ipc/gateway'
import { errorMessage } from '@/shared/lib/display'
import { Alert, AlertDescription } from '@/shared/ui/alert'
import { Badge } from '@/shared/ui/badge'
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

import {
  guessProvider,
  MODEL_CATEGORIES,
  PROVIDER_LABELS,
} from './categories'
import { formatSize } from './format'
import { CategoryFields } from './category-fields'
import { CREDENTIAL_CODES } from './use-enqueue'

interface Props {
  onClose: () => void
  onSubmit: (items: EnqueueItemInput[]) => Promise<number>
  onOpenSettings: () => void
}

function initialSelection(resolution: ResolutionDto): Set<number> {
  const primary = resolution.files.filter((file) => file.primary)
  if (primary.length > 0) return new Set(primary.map((file) => file.index))
  return resolution.files.length === 1 ? new Set([0]) : new Set()
}

/** URL or repository in, one job per selected file out. */
export function AddDownloadDialog({ onClose, onSubmit, onOpenSettings }: Props) {
  const { t } = useTranslation()
  const [url, setUrl] = useState('')
  const [httpConfirmed, setHttpConfirmed] = useState(false)
  const [resolution, setResolution] = useState<ResolutionDto | null>(null)
  const [selected, setSelected] = useState<Set<number>>(new Set())
  const [filter, setFilter] = useState('')
  const [category, setCategory] = useState('')
  const [subdirectory, setSubdirectory] = useState('')
  const [filename, setFilename] = useState('')
  const [submitting, setSubmitting] = useState(false)

  const provider = guessProvider(url)
  const plainHttp = url.trim().toLowerCase().startsWith('http://')

  const resolve = useMutation({
    mutationFn: () =>
      gateway.dlResolve({ url: url.trim(), httpConfirmed: httpConfirmed }),
    onSuccess: (result) => {
      setResolution(result)
      const chosen = initialSelection(result)
      setSelected(chosen)
      const first = result.files.find((file) => chosen.has(file.index))
      setCategory(first?.suggestedCategory ?? '')
      setFilename('')
    },
  })

  const files = resolution?.files ?? []
  const shown = files.filter((file) =>
    `${file.filename} ${file.label}`.toLowerCase().includes(filter.toLowerCase()),
  )
  const single = selected.size === 1
  const ready = resolution !== null && selected.size > 0 && category.trim() !== ''
  const totalBytes = files
    .filter((file) => selected.has(file.index))
    .reduce((sum, file) => sum + (file.size.bytes ?? 0), 0)

  function toggle(index: number, on: boolean) {
    setSelected((current) => {
      const next = new Set(current)
      if (on) next.add(index)
      else next.delete(index)
      return next
    })
  }

  async function submit() {
    if (!resolution) return
    setSubmitting(true)
    try {
      const items = [...selected].sort().map<EnqueueItemInput>((index) => ({
        resolutionId: resolution.id,
        fileIndex: index,
        category: category.trim(),
        subdirectory: subdirectory.trim(),
        filename: single && filename.trim() !== '' ? filename.trim() : null,
        conflict: null,
        force: false,
      }))
      await onSubmit(items)
      onClose()
    } finally {
      setSubmitting(false)
    }
  }

  const error = resolve.error
  const code = error instanceof IpcClientError ? error.code : null

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) onClose()
      }}
    >
      <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>{t('downloads.add')}</DialogTitle>
          <DialogDescription>
            {t('downloads.linkHint')}
          </DialogDescription>
        </DialogHeader>

        <form
          className="flex flex-col gap-2"
          onSubmit={(event) => {
            event.preventDefault()
            if (url.trim() !== '' && !(plainHttp && !httpConfirmed))
              resolve.mutate()
          }}
        >
          <Label htmlFor="download-url">{t('downloads.link')}</Label>
          <div className="flex gap-2">
            <div className="relative flex-1">
              <Link2
                className="pointer-events-none absolute top-2.5 left-3 size-4 text-muted-foreground"
                aria-hidden="true"
              />
              <Input
                id="download-url"
                className="pl-9"
                autoComplete="off"
                spellCheck={false}
                placeholder="https://civitai.com/models/..."
                value={url}
                onChange={(event) => {
                  setUrl(event.target.value)
                  setResolution(null)
                  resolve.reset()
                }}
              />
            </div>
            <Button
              type="submit"
              disabled={
                resolve.isPending ||
                url.trim() === '' ||
                (plainHttp && !httpConfirmed)
              }
            >
              {resolve.isPending ? (
                <LoaderCircle
                  className="motion-safe:animate-spin"
                  aria-hidden="true"
                />
              ) : null}
              {t('downloads.lookUp')}
            </Button>
          </div>
          {provider ? (
            <Badge
              variant="outline"
              className="w-fit font-mono text-[9px] uppercase tracking-widest"
            >
              {provider === 'direct' ? t(PROVIDER_LABELS.direct) : PROVIDER_LABELS[provider]}
            </Badge>
          ) : null}
          {plainHttp ? (
            <div className="flex items-center gap-2">
              <Checkbox
                id="http-confirmed"
                checked={httpConfirmed}
                onCheckedChange={(value) => setHttpConfirmed(value === true)}
              />
              <Label htmlFor="http-confirmed" className="text-sm">
                {t('downloads.httpConsent')}
              </Label>
            </div>
          ) : null}
        </form>

        {error ? (
          <Alert variant="destructive">
            <AlertDescription>
              <p>{errorMessage(error)}</p>
              {code && CREDENTIAL_CODES.includes(code) ? (
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => {
                    onClose()
                    onOpenSettings()
                  }}
                >
                  {t('common.openSettings')}
                </Button>
              ) : null}
            </AlertDescription>
          </Alert>
        ) : null}

        {resolution ? (
          <div className="flex flex-col gap-4">
            {files.length > 1 ? (
              <div className="flex flex-col gap-2">
                <div className="flex items-center justify-between gap-2">
                  <p className="font-mono text-xs font-semibold uppercase tracking-wider text-muted-foreground">
                    {t('downloads.filesSelected', { count: selected.size, total: files.length })}
                  </p>
                  {files.length > 8 ? (
                    <Input
                      aria-label={t('downloads.filterFiles')}
                      className="h-8 max-w-48"
                      placeholder={t('common.filter')}
                      value={filter}
                      onChange={(event) => setFilter(event.target.value)}
                    />
                  ) : null}
                </div>
                <ul className="max-h-60 divide-y overflow-y-auto rounded-lg border">
                  {shown.map((file) => (
                    <li
                      key={file.index}
                      className="flex items-center gap-3 px-3 py-2"
                    >
                      <Checkbox
                        id={`file-${file.index}`}
                        checked={selected.has(file.index)}
                        onCheckedChange={(value) =>
                          toggle(file.index, value === true)
                        }
                      />
                      <Label
                        htmlFor={`file-${file.index}`}
                        className="flex min-w-0 flex-1 flex-col items-start gap-0.5"
                      >
                        <span className="text-sm font-semibold break-words [overflow-wrap:anywhere]">
                          {file.primary ? t('downloads.primaryFile', { filename: file.filename }) : file.filename}
                        </span>
                        <span className="font-mono text-xs font-normal text-muted-foreground">
                          {formatSize(file.size)}
                        </span>
                      </Label>
                    </li>
                  ))}
                </ul>
              </div>
            ) : files[0] ? (
              <p className="text-sm break-words [overflow-wrap:anywhere]">
                <span className="font-semibold">{files[0].filename}</span>{' '}
                <span className="font-mono text-xs text-muted-foreground">
                  {formatSize(files[0].size)}
                  {' · '}{files[0].sha256 ? t('downloads.checksumPublished') : t('status.unverified')}
                </span>
              </p>
            ) : null}

            <CategoryFields
              category={category}
              onCategory={setCategory}
              subdirectory={subdirectory}
              onSubdirectory={setSubdirectory}
              suggested={
                files.find((file) => selected.has(file.index))
                  ?.suggestedCategory ?? null
              }
              known={MODEL_CATEGORIES}
            />

            {single ? (
              <div className="flex flex-col gap-1.5">
                <Label htmlFor="download-filename">
                  {t('downloads.optionalFilename')}
                </Label>
                <Input
                  id="download-filename"
                  autoComplete="off"
                  placeholder={
                    files.find((file) => selected.has(file.index))?.filename
                  }
                  value={filename}
                  onChange={(event) => setFilename(event.target.value)}
                />
              </div>
            ) : null}
          </div>
        ) : null}

        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t('common.cancel')}
          </Button>
          <Button
            disabled={!ready || submitting}
            onClick={() => void submit()}
          >
            {selected.size > 1
              ? t('downloads.downloadFiles', { count: selected.size, size: formatTotal(totalBytes) })
              : t('common.download')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function formatTotal(bytes: number): string {
  return bytes > 0 ? formatSize({ kind: 'exact', bytes }) : translate('downloads.sizeUnknownLowercase')
}
