// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'

import type { GalleryEntryDto } from '@/shared/ipc/gateway'

import { EntryGrid } from './entry-grid'
import { file, folder, many } from './fixtures'
import { keyOf } from './format'

vi.mock('@/shared/ipc/gateway', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/ipc/gateway')>()),
  gateway: {
    galleryPreviewUrl: (location: { area: string; path: string }, revision: string) =>
      `http://gallery.localhost/${location.area}/${location.path}?v=${revision}`,
  },
}))

const original = {
  offsetHeight: Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'offsetHeight'),
  offsetWidth: Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'offsetWidth'),
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
  // jsdom lays nothing out; the virtualizer reads these to decide how many rows fit.
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 600 })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, value: 900 })
})
afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
  for (const [name, descriptor] of Object.entries(original)) {
    if (descriptor) Object.defineProperty(HTMLElement.prototype, name, descriptor)
  }
})

function mount(entries: GalleryEntryDto[], selected: string[] = []) {
  const handlers = {
    onSelect: vi.fn<(key: string) => void>(),
    onToggle: vi.fn<(key: string) => void>(),
    onOpen: vi.fn<(entry: GalleryEntryDto) => void>(),
  }
  render(<EntryGrid entries={entries} selected={new Set(selected)} {...handlers} />)
  return handlers
}

const tile = (name: string) =>
  screen.getByRole('button', { name: new RegExp(name.replace('.', '\\.')) })

it('shows an image as a lazy thumbnail and everything else as an icon with its name and size', () => {
  mount([folder('renders'), file('cat.png'), file('clip.mp4', 'video'), file('robot.glb', 'model3d'), file('notes.txt', 'other')])
  const thumbnail = document.querySelector('img')
  expect(document.querySelectorAll('img')).toHaveLength(1)
  expect(thumbnail?.getAttribute('src')).toBe('http://gallery.localhost/input/cat.png?v=rev-cat.png')
  expect(thumbnail?.getAttribute('loading')).toBe('lazy')
  expect(thumbnail?.getAttribute('decoding')).toBe('async')
  expect(thumbnail?.getAttribute('alt')).toBe('')
  expect(tile('renders').textContent).toContain('Folder')
  expect(tile('cat.png').textContent).toContain('2.0 KiB')
  expect(screen.getByTitle('clip.mp4')).toBeTruthy()
})

it('falls back to an icon when a thumbnail cannot be loaded', () => {
  mount([file('broken.png')])
  fireEvent.error(document.querySelector('img') as HTMLImageElement)
  expect(document.querySelector('img')).toBeNull()
  expect(tile('broken.png')).toBeTruthy()
})

it('selects with a click, toggles with ctrl or cmd, and exposes the state as aria-pressed', () => {
  const { onSelect, onToggle } = mount([file('a.png'), file('b.png')], [keyOf(file('b.png').location)])
  expect(tile('a.png').getAttribute('aria-pressed')).toBe('false')
  expect(tile('b.png').getAttribute('aria-pressed')).toBe('true')
  fireEvent.click(tile('a.png'))
  expect(onSelect).toHaveBeenLastCalledWith('input:a.png')
  fireEvent.click(tile('a.png'), { ctrlKey: true })
  expect(onToggle).toHaveBeenLastCalledWith('input:a.png')
  fireEvent.click(tile('b.png'), { metaKey: true })
  expect(onToggle).toHaveBeenLastCalledWith('input:b.png')
  expect(onSelect).toHaveBeenCalledTimes(1)
})

it('toggles with the visible checkbox and the space key, and opens with double-click or enter', () => {
  const entry = file('a.png')
  const { onToggle, onOpen, onSelect } = mount([entry])
  fireEvent.click(screen.getByRole('checkbox', { name: 'Select a.png' }))
  expect(onToggle).toHaveBeenCalledTimes(1)
  fireEvent.keyDown(tile('a.png'), { key: ' ' })
  expect(onToggle).toHaveBeenCalledTimes(2)
  fireEvent.doubleClick(tile('a.png'))
  expect(onOpen).toHaveBeenCalledTimes(1)
  fireEvent.keyDown(tile('a.png'), { key: 'Enter' })
  expect(onOpen).toHaveBeenCalledTimes(2)
  expect(onOpen).toHaveBeenLastCalledWith(entry)
  expect(onSelect).not.toHaveBeenCalled()
})

it('windows a long folder by row while the page keeps the full count', () => {
  mount(many(1000))
  const rendered = document.querySelectorAll('button[aria-pressed]').length
  expect(rendered).toBeGreaterThan(0)
  expect(rendered).toBeLessThan(60)
  expect(screen.getByRole('group', { name: 'Folder contents' })).toBeTruthy()
})
