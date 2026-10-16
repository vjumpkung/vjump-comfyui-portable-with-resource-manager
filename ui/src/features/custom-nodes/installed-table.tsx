import { useVirtualizer } from '@tanstack/react-virtual'
import { TriangleAlert } from 'lucide-react'
import { useEffect, useRef, type KeyboardEvent } from 'react'

import type { InstalledNodeDto } from '@/shared/ipc/gateway'
import { cn } from '@/shared/lib/utils'
import { Badge } from '@/shared/ui/badge'
import { Checkbox } from '@/shared/ui/checkbox'

import { gitHealth, nodeBranch, nodeVersion, sourceLabel, VIRTUALIZE_AFTER } from './model'
import { Chip, DependencyChip, EnabledChip, HealthChip } from './status-chips'

export const ROW_HEIGHT = 56
const COLUMNS =
  'grid grid-cols-[2.25rem_minmax(12rem,3fr)_5.5rem_minmax(7rem,1.2fr)_minmax(5rem,1fr)_6.5rem_7.5rem_5.5rem_minmax(7rem,1fr)]'
const KIND_LABEL = { registry: 'Registry', git: 'Git', manual: 'Manual' } as const

interface Props {
  nodes: readonly InstalledNodeDto[]
  selected: ReadonlySet<string>
  activeId: string | null
  onActivate: (id: string) => void
  onToggle: (id: string) => void
  onToggleAll: () => void
}

/**
 * The installed nodes as an ARIA grid. Past 200 rows only the visible window is rendered. Rows are
 * keyed by node ID, so focus and selection survive scrolling and filtering. Actions live in an
 * adjacent toolbar, never inside the fixed-height rows.
 */
export function InstalledTable({
  nodes,
  selected,
  activeId,
  onActivate,
  onToggle,
  onToggleAll,
}: Props) {
  const scroller = useRef<HTMLDivElement>(null)
  const pendingFocus = useRef<string | null>(null)
  const virtualize = nodes.length > VIRTUALIZE_AFTER
  // TanStack Virtual returns unmemoizable functions; the table is the only consumer.
  // eslint-disable-next-line react-hooks/incompatible-library
  const virtualizer = useVirtualizer({
    count: virtualize ? nodes.length : 0,
    getScrollElement: () => scroller.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 8,
    getItemKey: (index) => nodes[index].id,
  })
  const virtualItems = virtualize ? virtualizer.getVirtualItems() : []

  // Keyboard navigation scrolls first; the row exists only after the next render.
  useEffect(() => {
    const wanted = pendingFocus.current
    if (wanted === null || wanted !== activeId) return
    const rows = scroller.current?.querySelectorAll<HTMLElement>('[role="row"][data-key]')
    const row = [...(rows ?? [])].find((element) => element.dataset.key === wanted)
    if (row) {
      pendingFocus.current = null
      row.focus()
    }
  })

  const allSelected = nodes.length > 0 && nodes.every((node) => selected.has(node.id))

  function move(to: number) {
    if (nodes.length === 0) return
    const index = Math.min(nodes.length - 1, Math.max(0, to))
    pendingFocus.current = nodes[index].id
    onActivate(nodes[index].id)
    if (virtualize) virtualizer.scrollToIndex(index)
  }

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>, index: number) {
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
        move(nodes.length - 1)
        break
      case ' ':
        event.preventDefault()
        onToggle(nodes[index].id)
        break
    }
  }

  const activeIndex = nodes.findIndex((node) => node.id === activeId)
  // Exactly one row is in the tab order, even when the active node was filtered out.
  const tabIndex = activeIndex >= 0 ? activeIndex : 0

  function renderRow(index: number, offset: number | null) {
    const node = nodes[index]
    const isSelected = selected.has(node.id)
    return (
      <div
        key={node.id}
        role="row"
        aria-rowindex={index + 2}
        aria-selected={isSelected}
        data-key={node.id}
        data-index={index}
        data-active={node.id === activeId}
        tabIndex={index === tabIndex ? 0 : -1}
        onClick={() => onActivate(node.id)}
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
          node.id === activeId ? 'bg-accent' : 'even:bg-muted/45 hover:bg-muted',
        )}
      >
        <div role="gridcell" onClick={(event) => event.stopPropagation()}>
          <Checkbox
            checked={isSelected}
            onCheckedChange={() => onToggle(node.id)}
            aria-label={`Select ${node.name}`}
            tabIndex={-1}
          />
        </div>
        <div role="gridcell" className="min-w-0">
          <p className="truncate font-medium" title={node.name}>{node.name}</p>
          <p className="truncate font-mono text-xs text-muted-foreground" title={sourceLabel(node.source)}>
            {sourceLabel(node.source)}
          </p>
        </div>
        <div role="gridcell">
          <Badge variant="outline">{KIND_LABEL[node.source.kind]}</Badge>
        </div>
        <div role="gridcell" className="truncate font-mono text-xs" title={nodeVersion(node)}>
          {nodeVersion(node)}
        </div>
        <div role="gridcell" className="truncate font-mono text-xs" title={nodeBranch(node)}>
          {nodeBranch(node)}
        </div>
        <div role="gridcell"><EnabledChip enabled={node.enabled} /></div>
        <div role="gridcell"><DependencyChip state={node.dependencies} /></div>
        <div role="gridcell" className="text-xs text-muted-foreground">
          {node.origin === 'app' ? 'Installed here' : 'Observed'}
        </div>
        <div role="gridcell" className="flex flex-wrap gap-1">
          <HealthChip health={gitHealth(node)} />
          {node.conflict && <Chip icon={TriangleAlert} className="bg-brand-pink">Conflict</Chip>}
        </div>
      </div>
    )
  }

  return (
    <div
      role="grid"
      aria-label="Installed custom nodes"
      aria-rowcount={nodes.length + 1}
      aria-multiselectable="true"
      className="rounded-lg border bg-card"
    >
      <div
        role="row"
        aria-rowindex={1}
        className={cn(
          COLUMNS,
          'items-center gap-2 border-b bg-brand-cyan px-2 py-2 text-xs font-semibold text-[#2a313a]',
        )}
      >
        <div role="columnheader">
          <Checkbox checked={allSelected} onCheckedChange={onToggleAll} aria-label="Select all" />
        </div>
        <div role="columnheader">Node</div>
        <div role="columnheader">Source</div>
        <div role="columnheader">Version / commit</div>
        <div role="columnheader">Branch</div>
        <div role="columnheader">State</div>
        <div role="columnheader">Dependencies</div>
        <div role="columnheader">Origin</div>
        <div role="columnheader">Git health</div>
      </div>
      <div
        ref={scroller}
        className={cn('min-w-0', virtualize && 'max-h-[60vh] overflow-auto')}
        data-virtualized={virtualize}
      >
        {virtualize ? (
          <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
            {virtualItems.map((item) => renderRow(item.index, item.start))}
          </div>
        ) : (
          nodes.map((_, index) => renderRow(index, null))
        )}
      </div>
    </div>
  )
}
