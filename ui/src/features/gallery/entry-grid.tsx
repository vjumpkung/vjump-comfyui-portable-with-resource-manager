import { useVirtualizer } from '@tanstack/react-virtual'
import { Box, File as FileIcon, Film, Folder, ImageOff } from 'lucide-react'
import { useLayoutEffect, useRef, useState, type KeyboardEvent, type RefObject } from 'react'

import { useTranslation } from '@/shared/i18n'
import { gateway, type GalleryEntryDto } from '@/shared/ipc/gateway'
import { cn } from '@/shared/lib/utils'
import { Checkbox } from '@/shared/ui/checkbox'

import { formatBytes, keyOf } from './format'

/** The narrowest a tile gets; the grid fits as many columns as this allows. */
const TILE_WIDTH = 168
/** A row is a tile plus the gap below it; every row has the same height. */
const ROW_HEIGHT = 208
/** Used until the container has been measured, and wherever nothing is laid out (jsdom). */
const FALLBACK_COLUMNS = 4

function useColumns(container: RefObject<HTMLElement | null>): number {
  const [width, setWidth] = useState(0)
  useLayoutEffect(() => {
    const element = container.current
    if (!element || typeof ResizeObserver === 'undefined') return
    const observer = new ResizeObserver(([entry]) => setWidth(entry.contentRect.width))
    observer.observe(element)
    return () => observer.disconnect()
  }, [container])
  return width > 0 ? Math.max(1, Math.floor(width / TILE_WIDTH)) : FALLBACK_COLUMNS
}

const ICON_CLASS = 'size-10 text-muted-foreground'

/** Stands in for the thumbnail of everything that is not a picture (or whose picture failed). */
function EntryIcon({ entry }: { entry: GalleryEntryDto }) {
  if (entry.kind === 'directory') return <Folder className={ICON_CLASS} aria-hidden="true" />
  if (entry.media === 'video') return <Film className={ICON_CLASS} aria-hidden="true" />
  if (entry.media === 'model3d') return <Box className={ICON_CLASS} aria-hidden="true" />
  if (entry.media === 'image') return <ImageOff className={ICON_CLASS} aria-hidden="true" />
  return <FileIcon className={ICON_CLASS} aria-hidden="true" />
}

interface TileProps {
  entry: GalleryEntryDto
  selected: boolean
  onSelect: () => void
  onToggle: () => void
  onOpen: () => void
}

function Tile({ entry, selected, onSelect, onToggle, onOpen }: TileProps) {
  const { t } = useTranslation()
  const [broken, setBroken] = useState(false)
  const showImage = entry.kind === 'file' && entry.media === 'image' && !broken

  function onKeyDown(event: KeyboardEvent<HTMLButtonElement>) {
    if (event.key === 'Enter') {
      event.preventDefault()
      onOpen()
    } else if (event.key === ' ') {
      event.preventDefault()
      onToggle()
    }
  }

  return (
    <div className="relative min-h-0">
      <button
        type="button"
        aria-pressed={selected}
        onClick={(event) => (event.ctrlKey || event.metaKey ? onToggle() : onSelect())}
        onDoubleClick={onOpen}
        onKeyDown={onKeyDown}
        className={cn(
          'flex h-full w-full flex-col gap-1.5 rounded-lg border p-2 text-left outline-none focus-visible:ring-2 focus-visible:ring-brand-cyan/50',
          selected ? 'border-brand-cyan bg-muted' : 'bg-card hover:bg-muted/50',
        )}
      >
        <span className="flex min-h-0 flex-1 items-center justify-center overflow-hidden rounded-md bg-muted/50">
          {showImage ? (
            <img
              src={gateway.galleryPreviewUrl(entry.location, entry.revision)}
              loading="lazy"
              decoding="async"
              alt=""
              className="size-full object-contain"
              onError={() => setBroken(true)}
            />
          ) : (
            <EntryIcon entry={entry} />
          )}
        </span>
        <span className="block w-full truncate text-sm font-medium" title={entry.name}>
          {entry.name}
        </span>
        <span className="block font-mono text-xs text-muted-foreground">
          {entry.kind === 'file' ? formatBytes(entry.sizeBytes) : t('gallery.folder')}
        </span>
      </button>
      <Checkbox
        checked={selected}
        onCheckedChange={onToggle}
        aria-label={t('gallery.select', { name: entry.name })}
        className="absolute top-3 left-3 bg-background"
      />
    </div>
  )
}

interface Props {
  entries: readonly GalleryEntryDto[]
  selected: ReadonlySet<string>
  onSelect: (key: string) => void
  onToggle: (key: string) => void
  onOpen: (entry: GalleryEntryDto) => void
}

/**
 * The folder as a grid of tiles, windowed by row so a folder of thousands of images only
 * mounts what is on screen. Directories come first in the listing and stay first here.
 */
export function EntryGrid({ entries, selected, onSelect, onToggle, onOpen }: Props) {
  const { t } = useTranslation()
  const scroller = useRef<HTMLDivElement>(null)
  const columns = useColumns(scroller)
  // TanStack Virtual returns unmemoizable functions; the grid is the only consumer.
  // eslint-disable-next-line react-hooks/incompatible-library
  const virtualizer = useVirtualizer({
    count: Math.ceil(entries.length / columns),
    getScrollElement: () => scroller.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 3,
  })

  return (
    <div
      ref={scroller}
      role="group"
      aria-label={t('gallery.entries')}
      className="max-h-[65vh] overflow-y-auto"
    >
      <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
        {virtualizer.getVirtualItems().map((row) => (
          <div
            key={row.key}
            className="absolute top-0 left-0 grid w-full gap-3 pb-3"
            style={{
              height: ROW_HEIGHT,
              transform: `translateY(${row.start}px)`,
              gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))`,
            }}
          >
            {entries.slice(row.index * columns, (row.index + 1) * columns).map((entry) => {
              const key = keyOf(entry.location)
              return (
                <Tile
                  // A new revision is a new file, so a failed thumbnail is tried again.
                  key={`${key}:${entry.revision}`}
                  entry={entry}
                  selected={selected.has(key)}
                  onSelect={() => onSelect(key)}
                  onToggle={() => onToggle(key)}
                  onOpen={() => onOpen(entry)}
                />
              )
            })}
          </div>
        ))}
      </div>
    </div>
  )
}
