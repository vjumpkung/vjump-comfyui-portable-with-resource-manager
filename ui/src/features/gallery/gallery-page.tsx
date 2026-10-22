import {
  FolderInput,
  FolderOpen,
  Images,
  LoaderCircle,
  Copy,
  Pencil,
  RefreshCw,
  Trash2,
} from 'lucide-react'
import { useMemo, useState } from 'react'

import { useTranslation } from '@/shared/i18n'
import type { GalleryAreaDto, GalleryEntryDto } from '@/shared/ipc/gateway'
import { Button } from '@/shared/ui/button'
import { EmptyState } from '@/shared/ui/empty-state'
import { ErrorNotice } from '@/shared/ui/feedback'

import { DeleteDialog } from './delete-dialog'
import { EntryGrid } from './entry-grid'
import { AreaToggle, Breadcrumb } from './folder-nav'
import { keyOf } from './format'
import { PreviewDialog } from './preview-dialog'
import { RenameDialog } from './rename-dialog'
import { TransferDialog } from './transfer-dialog'
import {
  isNotInstalled,
  reveal,
  useGalleryListing,
  type TransferMode,
} from './use-gallery'

type Dialogs =
  | { kind: 'rename'; entry: GalleryEntryDto }
  | { kind: 'transfer'; mode: TransferMode; entries: GalleryEntryDto[] }
  | { kind: 'delete'; entries: GalleryEntryDto[] }
  | { kind: 'preview'; key: string }
  | null

/** ComfyUI's input and output folders as a browsable grid of previews. */
export function GalleryPage() {
  const { t } = useTranslation()
  const [area, setArea] = useState<GalleryAreaDto>('input')
  const [path, setPath] = useState('')
  const [selected, setSelected] = useState<ReadonlySet<string>>(new Set())
  const [dialog, setDialog] = useState<Dialogs>(null)
  const listing = useGalleryListing(area, path)

  const entries = useMemo(() => listing.data?.entries ?? [], [listing.data])
  const files = useMemo(() => entries.filter((entry) => entry.kind === 'file'), [entries])
  const chosen = entries.filter((entry) => selected.has(keyOf(entry.location)))
  const previewing =
    dialog?.kind === 'preview'
      ? (files.find((entry) => keyOf(entry.location) === dialog.key) ?? null)
      : null
  const warnings = listing.data?.warnings ?? []
  const single = chosen.length === 1 ? chosen[0] : null

  function navigate(next: string) {
    setPath(next)
    setSelected(new Set())
  }

  function changeArea(next: GalleryAreaDto) {
    setArea(next)
    navigate('')
  }

  function toggle(key: string) {
    setSelected((current) => {
      const next = new Set(current)
      if (!next.delete(key)) next.add(key)
      return next
    })
  }

  function open(entry: GalleryEntryDto) {
    if (entry.kind === 'directory') navigate(entry.location.path)
    else setDialog({ kind: 'preview', key: keyOf(entry.location) })
  }

  const close = () => setDialog(null)

  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h1 className="text-2xl font-extrabold tracking-tighter sm:text-3xl">
            {t('gallery.title')}
          </h1>
          <p className="font-mono text-xs text-muted-foreground">
            {t('common.itemCount', { count: entries.length })}
            {chosen.length > 0 ? ` · ${t('gallery.selected', { count: chosen.length })}` : ''}
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <AreaToggle area={area} onChange={changeArea} />
          <Button
            variant="outline"
            disabled={listing.isFetching}
            onClick={() => void listing.refetch()}
          >
            {listing.isFetching ? (
              <LoaderCircle className="animate-spin" aria-hidden="true" />
            ) : (
              <RefreshCw aria-hidden="true" />
            )}
            {t('common.refresh')}
          </Button>
          <Button
            variant="outline"
            disabled={listing.isError}
            onClick={() => void reveal({ area, path })}
          >
            <FolderOpen aria-hidden="true" />
            {t('gallery.openInExplorer')}
          </Button>
        </div>
      </div>

      <Breadcrumb area={area} path={path} onNavigate={navigate} />

      {chosen.length > 0 ? (
        <div className="flex flex-wrap gap-2">
          <Button variant="outline" disabled={single === null} onClick={() => single && setDialog({ kind: 'rename', entry: single })}>
            <Pencil aria-hidden="true" />
            {t('common.rename')}
          </Button>
          <Button variant="outline" onClick={() => setDialog({ kind: 'transfer', mode: 'move', entries: chosen })}>
            <FolderInput aria-hidden="true" />
            {t('gallery.moveTo')}
          </Button>
          <Button variant="outline" onClick={() => setDialog({ kind: 'transfer', mode: 'copy', entries: chosen })}>
            <Copy aria-hidden="true" />
            {t('gallery.copyTo')}
          </Button>
          <Button variant="outline" onClick={() => setDialog({ kind: 'delete', entries: chosen })}>
            <Trash2 aria-hidden="true" />
            {t('common.delete')}
          </Button>
          <Button variant="outline" disabled={single === null} onClick={() => single && void reveal(single.location)}>
            <FolderOpen aria-hidden="true" />
            {t('common.showInExplorer')}
          </Button>
        </div>
      ) : null}

      {listing.data === undefined && listing.isError ? (
        isNotInstalled(listing.error) ? (
          <EmptyState
            icon={<Images className="size-8" aria-hidden="true" />}
            title={t('gallery.notInstalled.title')}
            description={t('gallery.notInstalled.hint')}
          />
        ) : (
          <ErrorNotice error={listing.error} retry={() => void listing.refetch()} />
        )
      ) : listing.data === undefined ? (
        <p role="status" className="text-sm text-muted-foreground">
          {t('gallery.loading')}
        </p>
      ) : entries.length === 0 ? (
        <EmptyState
          icon={<Images className="size-8" aria-hidden="true" />}
          title={t('gallery.empty')}
          description={t('gallery.emptyHint')}
        />
      ) : (
        <EntryGrid
          entries={entries}
          selected={selected}
          onSelect={(key) => setSelected(new Set([key]))}
          onToggle={toggle}
          onOpen={open}
        />
      )}

      {warnings.length > 0 ? (
        <p className="text-xs text-muted-foreground">
          {t('gallery.warnings', { count: warnings.length })}
        </p>
      ) : null}

      {dialog?.kind === 'rename' ? <RenameDialog entry={dialog.entry} onClose={close} /> : null}
      {dialog?.kind === 'transfer' ? (
        <TransferDialog
          mode={dialog.mode}
          entries={dialog.entries}
          area={area}
          path={path}
          onClose={close}
        />
      ) : null}
      {dialog?.kind === 'delete' ? <DeleteDialog entries={dialog.entries} onClose={close} /> : null}
      {previewing ? (
        <PreviewDialog
          entry={previewing}
          files={files}
          onNavigate={(entry) => setDialog({ kind: 'preview', key: keyOf(entry.location) })}
          onClose={close}
        />
      ) : null}
    </div>
  )
}
