import { ChevronLeft, ChevronRight, FileQuestion, FolderOpen, LoaderCircle } from 'lucide-react'
import { lazy, Suspense, useCallback, useState, type KeyboardEvent } from 'react'

import { useTranslation } from '@/shared/i18n'
import { gateway, type GalleryEntryDto } from '@/shared/ipc/gateway'
import { Button } from '@/shared/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shared/ui/dialog'

import { formatBytes, keyOf } from './format'
import { reveal } from './use-gallery'

// three.js is large, so it loads only when a 3D file is first previewed.
const ModelViewer = lazy(() => import('./model-viewer'))

function PreviewBody({ entry }: { entry: GalleryEntryDto }) {
  const { t } = useTranslation()
  const [failed, setFailed] = useState(false)
  const fail = useCallback(() => setFailed(true), [])
  const src = gateway.galleryPreviewUrl(entry.location, entry.revision)

  if (entry.media === 'other' || failed) {
    const message =
      entry.media === 'other'
        ? t('gallery.preview.none')
        : t(entry.media === 'model3d' ? 'gallery.preview.failed3d' : 'gallery.preview.failed')
    return (
      <div className="flex flex-col items-center gap-2 text-sm text-muted-foreground">
        <FileQuestion className="size-10" aria-hidden="true" />
        <p>{message}</p>
      </div>
    )
  }
  if (entry.media === 'image') {
    return (
      <img
        src={src}
        alt={entry.name}
        className="max-h-full max-w-full object-contain"
        onError={fail}
      />
    )
  }
  if (entry.media === 'video') {
    return (
      <video
        controls
        autoPlay={false}
        src={src}
        className="max-h-full max-w-full"
        onError={fail}
      />
    )
  }
  return (
    <Suspense
      fallback={<LoaderCircle className="animate-spin" aria-label={t('common.loadingLabel')} />}
    >
      <ModelViewer entry={entry} onError={fail} />
    </Suspense>
  )
}

interface Props {
  entry: GalleryEntryDto
  /** The files of the folder, in listing order; the arrows step through them. */
  files: readonly GalleryEntryDto[]
  onNavigate: (entry: GalleryEntryDto) => void
  onClose: () => void
}

/** Previews one file and steps to the previous or next file of the same folder. */
export function PreviewDialog({ entry, files, onNavigate, onClose }: Props) {
  const { t } = useTranslation()
  const index = files.findIndex((file) => keyOf(file.location) === keyOf(entry.location))
  const previous = index > 0 ? files[index - 1] : null
  const next = index >= 0 && index < files.length - 1 ? files[index + 1] : null

  function onKeyDown(event: KeyboardEvent<HTMLElement>) {
    // The arrows seek inside a playing video, so they only step when focus is elsewhere.
    if (event.target instanceof HTMLVideoElement) return
    if (event.key === 'ArrowLeft' && previous) {
      event.preventDefault()
      onNavigate(previous)
    } else if (event.key === 'ArrowRight' && next) {
      event.preventDefault()
      onNavigate(next)
    }
  }

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-4xl" onKeyDown={onKeyDown}>
        <DialogHeader>
          <DialogTitle className="truncate pr-8" title={entry.name}>
            {entry.name}
          </DialogTitle>
          <DialogDescription>
            {formatBytes(entry.sizeBytes)}
            {index >= 0
              ? ` · ${t('gallery.preview.position', { index: index + 1, total: files.length })}`
              : ''}
          </DialogDescription>
        </DialogHeader>
        <div className="flex h-[60vh] items-center justify-center overflow-hidden rounded-lg bg-muted/40">
          <PreviewBody key={`${keyOf(entry.location)}:${entry.revision}`} entry={entry} />
        </div>
        <DialogFooter className="sm:justify-between">
          <div className="flex gap-2">
            <Button variant="outline" disabled={!previous} onClick={() => previous && onNavigate(previous)}>
              <ChevronLeft aria-hidden="true" />
              {t('common.previous')}
            </Button>
            <Button variant="outline" disabled={!next} onClick={() => next && onNavigate(next)}>
              {t('common.next')}
              <ChevronRight aria-hidden="true" />
            </Button>
          </div>
          <Button variant="outline" onClick={() => void reveal(entry.location)}>
            <FolderOpen aria-hidden="true" />
            {t('common.showInExplorer')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
