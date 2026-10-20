// @vitest-environment jsdom
import { cleanup, configure, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { useState } from 'react'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'

import type { InstalledNodeDto } from '@/shared/ipc/gateway'

import { manyNodes, node } from './fixtures'
import { InstalledTable } from './installed-table'
import { repairActive } from './model'

// Rendering many rows in jsdom is slow when the whole suite runs in parallel.
vi.setConfig({ testTimeout: 30_000 })
configure({ asyncUtilTimeout: 5000 })

type Restore = () => void
const restores: Restore[] = []
function define(name: string, descriptor: PropertyDescriptor, prototype: object = HTMLElement.prototype) {
  const original = Object.getOwnPropertyDescriptor(prototype, name)
  Object.defineProperty(prototype, name, { configurable: true, ...descriptor })
  restores.push(() => {
    if (original) Object.defineProperty(prototype, name, original)
    else delete (prototype as Record<string, unknown>)[name]
  })
}

beforeEach(() => {
  vi.stubGlobal(
    'ResizeObserver',
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  )
  define('offsetHeight', { value: 480 })
  define('offsetWidth', { value: 900 })
})
afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
  while (restores.length > 0) restores.pop()?.()
})

function Harness({ nodes, initial = null, onToggle }: {
  nodes: readonly InstalledNodeDto[]
  initial?: string | null
  onToggle?: (id: string) => void
}) {
  const [active, setActive] = useState<string | null>(initial)
  const [selected, setSelected] = useState<ReadonlySet<string>>(new Set())
  return (
    <InstalledTable
      nodes={nodes}
      selected={selected}
      activeId={repairActive(nodes, active)}
      onActivate={setActive}
      onToggle={(id) => {
        onToggle?.(id)
        setSelected((current) => {
          const next = new Set(current)
          if (!next.delete(id)) next.add(id)
          return next
        })
      }}
      onToggleAll={() => undefined}
    />
  )
}

const rows = () => [...document.querySelectorAll<HTMLElement>('[role="row"][data-key]')]
const row = (id: string) => rows().find((element) => element.dataset.key === id)

it('virtualizes_only_above_200_rows', () => {
  const view = render(<Harness nodes={manyNodes(200)} />)
  const grid = () => view.container.querySelector('[data-virtualized]')
  expect(grid()?.getAttribute('data-virtualized')).toBe('false')
  expect(rows()).toHaveLength(200)
  cleanup()
  const big = render(<Harness nodes={manyNodes(201)} />)
  expect(big.container.querySelector('[data-virtualized]')?.getAttribute('data-virtualized')).toBe('true')
  expect(rows().length).toBeGreaterThan(0)
  expect(rows().length).toBeLessThan(60)
  expect(screen.getByRole('grid', { name: 'Installed custom nodes' }).getAttribute('aria-rowcount')).toBe('202')
})

it('end_scrolls_then_focuses_the_last_row', async () => {
  const scrollTo = vi.fn(function (this: HTMLElement, options: ScrollToOptions) {
    this.scrollTop = options.top ?? 0
    this.dispatchEvent(new Event('scroll'))
  })
  define('scrollTo', { value: scrollTo })
  // jsdom has no layout: describe the scroll container the way a browser would.
  define('scrollHeight', {
    get(this: HTMLElement) { return this.dataset.virtualized === 'true' ? 56 * 1000 : 0 },
  }, Element.prototype)
  define('clientHeight', {
    get(this: HTMLElement) { return this.dataset.virtualized === 'true' ? 480 : 0 },
  }, Element.prototype)
  const nodes = manyNodes(1000)
  render(<Harness nodes={nodes} />)
  const first = row(nodes[0].id) as HTMLElement
  first.focus()
  fireEvent.click(first)
  expect(row(nodes[999].id)).toBeUndefined()
  fireEvent.keyDown(first, { key: 'End' })
  // The window moves first; the row exists and takes focus only afterwards.
  expect(scrollTo).toHaveBeenCalled()
  await waitFor(() => expect(document.activeElement).toBe(row(nodes[999].id)))
  expect(row(nodes[999].id)?.getAttribute('data-active')).toBe('true')
  expect(row(nodes[999].id)?.tabIndex).toBe(0)
  fireEvent.keyDown(row(nodes[999].id) as HTMLElement, { key: 'Home' })
  await waitFor(() => expect(document.activeElement).toBe(row(nodes[0].id)))
})

it('filtering_preserves_or_repairs_active_identity', () => {
  const [a, b, c] = manyNodes(3)
  const view = render(<Harness nodes={[a, b, c]} initial={b.id} />)
  const tabStops = () => rows().filter((element) => element.tabIndex === 0).map((element) => element.dataset.key)
  expect(tabStops()).toEqual([b.id])
  // The active node survives a filter that still lists it.
  view.rerender(<Harness nodes={[b, c]} initial={b.id} />)
  expect(tabStops()).toEqual([b.id])
  expect(row(b.id)?.getAttribute('data-active')).toBe('true')
  // A filter that removes it moves the single tab stop and the active identity to the first row.
  view.rerender(<Harness nodes={[c]} initial={b.id} />)
  expect(tabStops()).toEqual([c.id])
  expect(row(c.id)?.getAttribute('data-active')).toBe('true')
})

it('keyboard_selection_uses_node_ids', () => {
  const [a, b, c] = [
    node({ id: 'a', name: 'Alpha' }),
    node({ id: 'b', name: 'Beta' }),
    node({ id: 'c', name: 'Gamma' }),
  ]
  const toggled: string[] = []
  const view = render(<Harness nodes={[a, b, c]} onToggle={(id) => toggled.push(id)} />)
  const beta = row('b') as HTMLElement
  beta.focus()
  fireEvent.keyDown(beta, { key: ' ' })
  expect(toggled).toEqual(['b'])
  expect(row('b')?.getAttribute('aria-selected')).toBe('true')
  // Filtering and reordering do not move the selection to another row.
  view.rerender(<Harness nodes={[c, b]} onToggle={(id) => toggled.push(id)} />)
  expect(row('b')?.getAttribute('aria-selected')).toBe('true')
  expect(row('c')?.getAttribute('aria-selected')).toBe('false')
  fireEvent.keyDown(row('c') as HTMLElement, { key: ' ' })
  expect(toggled).toEqual(['b', 'c'])
})

it('shows source, version, branch, state, dependencies, origin, conflict and Git health', () => {
  render(<Harness nodes={[
    node({ id: 'g', name: 'Gitty', dirty: true, conflict: true, dependencies: 'failed', enabled: false, origin: 'observed' }),
  ]} />)
  const gitty = row('g') as HTMLElement
  for (const text of ['Gitty', 'Git', 'aaaaaaaaaa', 'main', 'Disabled', 'Deps failed', 'Observed', 'Conflict', 'Local changes']) {
    expect(gitty.textContent).toContain(text)
  }
})

it('installed_table_active_row_keeps_readable_contrast', () => {
  const [a, b] = [node({ id: 'a', name: 'Alpha' }), node({ id: 'b', name: 'Beta' })]
  render(<Harness nodes={[a, b]} initial={a.id} />)
  const active = row('a') as HTMLElement
  expect(active.dataset.active).toBe('true')
  // No brand fill behind table text: the active row is muted with an accent bar.
  expect(active.classList.contains('bg-muted')).toBe(true)
  expect(active.classList.contains('bg-accent')).toBe(false)
  expect(row('b')?.classList.contains('bg-accent')).toBe(false)
})
