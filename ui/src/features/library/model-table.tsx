import { useVirtualizer } from '@tanstack/react-virtual'
import { Image as ImageIcon } from 'lucide-react'
import { useEffect, useRef, type KeyboardEvent } from 'react'

import type { ModelEntryDto } from '@/shared/ipc/gateway'
import { cn } from '@/shared/lib/utils'
import { Badge } from '@/shared/ui/badge'
import { Checkbox } from '@/shared/ui/checkbox'

import {
  formatBytes,
  formatModified,
  keyOf,
  sourceLabel,
  VIRTUALIZE_AFTER,
} from './model-list'

export const ROW_HEIGHT = 40
const COLUMNS =
  'grid grid-cols-[2.25rem_minmax(12rem,3fr)_7rem_minmax(5rem,1.5fr)_5.5rem_10.5rem_8rem]'

interface Props {
  models: readonly ModelEntryDto[]
  selected: ReadonlySet<string>
  activeKey: string | null
  onActivate: (key: string) => void
  onToggle: (key: string) => void
  onToggleAll: () => void
  onDelete: () => void
}

/**
 * The model list as an ARIA grid. Past 200 rows only the visible window is rendered; rows keep
 * their model identity as the React key, so focus and selection survive scrolling and filtering.
 */
export function ModelTable({
  models,
  selected,
  activeKey,
  onActivate,
  onToggle,
  onToggleAll,
  onDelete,
}: Props) {
  const scroller = useRef<HTMLDivElement>(null)
  const pendingFocus = useRef<string | null>(null)
  const virtualize = models.length > VIRTUALIZE_AFTER
  // TanStack Virtual returns unmemoizable functions; the table is the only consumer.
  // eslint-disable-next-line react-hooks/incompatible-library
  const virtualizer = useVirtualizer({
    count: virtualize ? models.length : 0,
    getScrollElement: () => scroller.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 8,
    getItemKey: (index) => keyOf(models[index].location),
  })
  const virtualItems = virtualize ? virtualizer.getVirtualItems() : []

  // Keyboard navigation scrolls first; the row exists only after the next render.
  useEffect(() => {
    const wanted = pendingFocus.current
    if (wanted === null || wanted !== activeKey) return
    const rows = scroller.current?.querySelectorAll<HTMLElement>(
      '[role="row"][data-key]',
    )
    const row = [...(rows ?? [])].find((element) => element.dataset.key === wanted)
    if (row) {
      pendingFocus.current = null
      row.focus()
    }
  })

  const allSelected =
    models.length > 0 && models.every((m) => selected.has(keyOf(m.location)))

  function move(to: number) {
    if (models.length === 0) return
    const index = Math.min(models.length - 1, Math.max(0, to))
    const key = keyOf(models[index].location)
    pendingFocus.current = key
    onActivate(key)
    if (virtualize) virtualizer.scrollToIndex(index)
  }

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>, index: number) {
    const key = keyOf(models[index].location)
    switch (event.key) {
      case 'ArrowDown':
        event.preventDefault()
        move(index + 1)
        break
      case 'ArrowUp':
        event.preventDefault()
        move(index - 1)
        break
      case 'PageDown':
        event.preventDefault()
        move(index + 10)
        break
      case 'PageUp':
        event.preventDefault()
        move(index - 10)
        break
      case 'Home':
        event.preventDefault()
        move(0)
        break
      case 'End':
        event.preventDefault()
        move(models.length - 1)
        break
      case ' ':
        event.preventDefault()
        onToggle(key)
        break
      case 'Delete':
        event.preventDefault()
        onDelete()
        break
    }
  }

  const activeIndex = models.findIndex((m) => keyOf(m.location) === activeKey)
  // Exactly one row is in the tab order.
  const tabIndex = activeIndex >= 0 ? activeIndex : 0

  function renderRow(index: number, offset: number | null) {
    const model = models[index]
    const key = keyOf(model.location)
    const isSelected = selected.has(key)
    return (
      <div
        key={key}
        role="row"
        aria-rowindex={index + 2}
        aria-selected={isSelected}
        data-key={key}
        data-index={index}
        tabIndex={index === tabIndex ? 0 : -1}
        onClick={() => onActivate(key)}
        onKeyDown={(event) => onKeyDown(event, index)}
        style={
          offset === null
            ? { height: ROW_HEIGHT }
            : {
                height: ROW_HEIGHT,
                transform: `translateY(${offset}px)`,
                position: 'absolute',
                top: 0,
                left: 0,
                width: '100%',
              }
        }
        className={cn(
          COLUMNS,
          'items-center gap-2 border-b px-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-brand-cyan/50',
          key === activeKey
            ? 'bg-muted shadow-[inset_3px_0_0_var(--accent)]'
            : isSelected
              ? 'bg-muted/60'
              : 'hover:bg-muted/50',
        )}
      >
        <div role="gridcell" onClick={(event) => event.stopPropagation()}>
          <Checkbox
            checked={isSelected}
            onCheckedChange={() => onToggle(key)}
            aria-label={`Select ${model.name}`}
            tabIndex={-1}
          />
        </div>
        <div role="gridcell" className="flex min-w-0 items-center gap-1.5">
          <span className="truncate font-medium" title={model.name}>
            {model.name}
          </span>
          {model.sidecars.preview ? (
            <ImageIcon
              className="size-3.5 shrink-0 text-muted-foreground"
              aria-label="Has preview"
            />
          ) : null}
        </div>
        <div role="gridcell">
          <Badge variant="outline">{model.category}</Badge>
        </div>
        <div
          role="gridcell"
          className="truncate font-mono text-xs text-muted-foreground"
          title={model.subfolder}
        >
          {model.subfolder || '-'}
        </div>
        <div role="gridcell" className="font-mono text-xs">
          {formatBytes(model.size)}
        </div>
        <div role="gridcell" className="truncate text-xs text-muted-foreground">
          {formatModified(model.modifiedMs)}
        </div>
        <div role="gridcell" className="truncate text-xs">
          {sourceLabel(model)}
        </div>
      </div>
    )
  }

  return (
    <div
      role="grid"
      aria-label="Models"
      aria-rowcount={models.length + 1}
      aria-multiselectable="true"
      className="rounded-xl border"
    >
      <div
        role="row"
        aria-rowindex={1}
        className={cn(
          COLUMNS,
          'items-center gap-2 border-b bg-muted/40 px-2 py-2 text-xs font-semibold text-muted-foreground',
        )}
      >
        <div role="columnheader">
          <Checkbox
            checked={allSelected}
            onCheckedChange={onToggleAll}
            aria-label="Select all"
          />
        </div>
        <div role="columnheader">Name</div>
        <div role="columnheader">Category</div>
        <div role="columnheader">Subfolder</div>
        <div role="columnheader">Size</div>
        <div role="columnheader">Modified</div>
        <div role="columnheader">Source</div>
      </div>
      <div
        ref={scroller}
        className={cn('min-w-0', virtualize && 'max-h-[60vh] overflow-auto')}
        data-virtualized={virtualize}
      >
        {virtualize ? (
          <div
            style={{ height: virtualizer.getTotalSize(), position: 'relative' }}
          >
            {virtualItems.map((item) => renderRow(item.index, item.start))}
          </div>
        ) : (
          models.map((_, index) => renderRow(index, null))
        )}
      </div>
    </div>
  )
}
