// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { useEffect } from 'react'
import { toast } from 'sonner'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'

import { gateway, IpcClientError, type GalleryEntryDto } from '@/shared/ipc/gateway'

import { file, folder, listing, report, result } from './fixtures'
import { GalleryPage } from './gallery-page'

vi.mock('@/shared/ipc/gateway', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/ipc/gateway')>()),
  gateway: {
    galleryList: vi.fn(),
    galleryRename: vi.fn(),
    galleryMove: vi.fn(),
    galleryCopy: vi.fn(),
    galleryDeletePlan: vi.fn(),
    galleryDelete: vi.fn(),
    galleryReveal: vi.fn(),
    // Plain functions, so `resetAllMocks` leaves them alone.
    galleryPreviewBase: () => 'http://gallery.localhost/',
    galleryPreviewUrl: (location: { area: string; path: string }, revision: string) =>
      `http://gallery.localhost/${location.area}/${location.path}?v=${revision}`,
  },
}))
vi.mock('sonner', () => ({
  toast: { error: vi.fn(), success: vi.fn(), info: vi.fn() },
}))
// The real viewer needs WebGL, which jsdom does not have.
const viewer = vi.hoisted(() => ({ fail: false }))
vi.mock('./model-viewer', () => {
  function ModelViewerStub({ entry, onError }: { entry: GalleryEntryDto; onError: () => void }) {
    useEffect(() => {
      if (viewer.fail) onError()
    }, [onError])
    return <p>3D viewer for {entry.name}</p>
  }
  return { default: ModelViewerStub }
})

const original = {
  offsetHeight: Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'offsetHeight'),
  offsetWidth: Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'offsetWidth'),
}

/** The folders of the fake gallery, keyed by `area:path`. */
let tree = new Map<string, GalleryEntryDto[]>()

beforeEach(() => {
  vi.stubGlobal(
    'ResizeObserver',
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  )
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 600 })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, value: 900 })
  vi.resetAllMocks()
  viewer.fail = false
  tree = new Map([
    [
      'input:',
      [
        folder('archive'),
        folder('renders'),
        file('cat.png'),
        file('clip.mp4', 'video'),
        file('robot.glb', 'model3d'),
        file('notes.txt', 'other'),
      ],
    ],
    ['input:renders', [file('deep.png', 'image', 'renders')]],
    ['input:archive', [folder('old', 'archive')]],
    ['output:', [file('result.png', 'image', '', 'output')]],
  ])
  vi.mocked(gateway.galleryList).mockImplementation(({ area, path }) =>
    Promise.resolve(listing(tree.get(`${area}:${path}`) ?? [], { area, path })),
  )
  vi.mocked(gateway.galleryReveal).mockResolvedValue(null)
})
afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
  for (const [name, descriptor] of Object.entries(original)) {
    if (descriptor) Object.defineProperty(HTMLElement.prototype, name, descriptor)
  }
})

function mount() {
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <GalleryPage />
    </QueryClientProvider>,
  )
}

const escaped = (name: string) => new RegExp(name.replace('.', '\\.'))
const tile = (name: string) => screen.getByRole('button', { name: escaped(name) })
const findTile = (name: string) => screen.findByRole('button', { name: escaped(name) })
const refOf = (entry: GalleryEntryDto) => ({ location: entry.location, revision: entry.revision })

async function select(...names: string[]) {
  for (const [index, name] of names.entries()) {
    fireEvent.click(await findTile(name), { ctrlKey: index > 0 })
  }
}

it('lists folders, images, videos, 3D models and other files with an item count', async () => {
  mount()
  for (const name of ['archive', 'renders', 'cat.png', 'clip.mp4', 'robot.glb', 'notes.txt']) {
    expect(await findTile(name)).toBeTruthy()
  }
  expect(gateway.galleryList).toHaveBeenCalledWith({ area: 'input', path: '' })
  expect(screen.getByText('6 items')).toBeTruthy()
  expect(document.querySelector('img')?.getAttribute('src')).toBe(
    'http://gallery.localhost/input/cat.png?v=rev-cat.png',
  )
  await select('cat.png')
  expect(screen.getByText('6 items · 1 selected')).toBeTruthy()
})

it('opens a folder with a double-click and returns through the breadcrumb', async () => {
  mount()
  fireEvent.doubleClick(await findTile('renders'))
  expect(await findTile('deep.png')).toBeTruthy()
  expect(gateway.galleryList).toHaveBeenCalledWith({ area: 'input', path: 'renders' })
  const crumbs = screen.getByRole('navigation', { name: 'Folder path' })
  expect(within(crumbs).getByRole('button', { name: 'renders' }).getAttribute('aria-current')).toBe('page')
  fireEvent.click(within(crumbs).getByRole('button', { name: 'Input' }))
  expect(await findTile('cat.png')).toBeTruthy()
  expect(screen.queryByRole('button', { name: /deep\.png/ })).toBeNull()
})

it('switches area, starts at its root and drops the selection', async () => {
  mount()
  fireEvent.doubleClick(await findTile('renders'))
  await findTile('deep.png')
  await select('deep.png')
  fireEvent.click(screen.getByRole('radio', { name: 'Output' }))
  expect(await findTile('result.png')).toBeTruthy()
  expect(gateway.galleryList).toHaveBeenLastCalledWith({ area: 'output', path: '' })
  expect(screen.queryByRole('button', { name: 'Rename' })).toBeNull()
  expect(screen.queryByText(/selected/)).toBeNull()
  // Choosing the active area again does not deselect it.
  fireEvent.click(screen.getByRole('radio', { name: 'Output' }))
  expect(screen.getByRole('radio', { name: 'Output' }).getAttribute('aria-checked')).toBe('true')
})

it('opens the current folder in the system file manager', async () => {
  mount()
  await findTile('cat.png')
  fireEvent.click(screen.getByRole('button', { name: 'Open in Explorer' }))
  expect(gateway.galleryReveal).toHaveBeenCalledWith({ area: 'input', path: '' })
  fireEvent.doubleClick(tile('renders'))
  await findTile('deep.png')
  fireEvent.click(screen.getByRole('button', { name: 'Open in Explorer' }))
  expect(gateway.galleryReveal).toHaveBeenLastCalledWith({ area: 'input', path: 'renders' })
})

it('shows the selected entry in the system file manager and reports a failure', async () => {
  vi.mocked(gateway.galleryReveal).mockRejectedValue(
    new IpcClientError('explorer_failed', 'The file manager could not be opened.'),
  )
  mount()
  await select('cat.png')
  fireEvent.click(screen.getByRole('button', { name: 'Show in Explorer' }))
  expect(gateway.galleryReveal).toHaveBeenCalledWith(file('cat.png').location)
  await waitFor(() => expect(toast.error).toHaveBeenCalledWith('The file manager could not be opened.'))
})

it('limits rename and show-in-explorer to exactly one selected entry', async () => {
  mount()
  await select('cat.png', 'clip.mp4')
  expect(screen.getByRole<HTMLButtonElement>('button', { name: 'Rename' }).disabled).toBe(true)
  expect(screen.getByRole<HTMLButtonElement>('button', { name: 'Show in Explorer' }).disabled).toBe(true)
  expect(screen.getByRole<HTMLButtonElement>('button', { name: 'Move to...' }).disabled).toBe(false)
})

it('renames the selected entry with its revision and refetches', async () => {
  vi.mocked(gateway.galleryRename).mockResolvedValue({ area: 'input', path: 'dog.png' })
  mount()
  await select('cat.png')
  fireEvent.click(screen.getByRole('button', { name: 'Rename' }))
  const dialog = await screen.findByRole('dialog')
  const input = within(dialog).getByLabelText<HTMLInputElement>('New name')
  expect(input.value).toBe('cat.png')
  // The stem is selected so typing keeps the extension.
  expect([input.selectionStart, input.selectionEnd]).toEqual([0, 3])
  const submit = within(dialog).getByRole<HTMLButtonElement>('button', { name: 'Rename' })
  expect(submit.disabled).toBe(true)
  fireEvent.change(input, { target: { value: ' dog.png ' } })
  const before = vi.mocked(gateway.galleryList).mock.calls.length
  fireEvent.click(submit)
  await waitFor(() =>
    expect(gateway.galleryRename).toHaveBeenCalledWith({ entry: refOf(file('cat.png')), newName: 'dog.png' }),
  )
  await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  expect(vi.mocked(gateway.galleryList).mock.calls.length).toBeGreaterThan(before)
})

it('keeps the rename dialog open and shows a refused name inline', async () => {
  vi.mocked(gateway.galleryRename).mockRejectedValue(
    new IpcClientError('validation', 'That name is not allowed.'),
  )
  mount()
  await select('cat.png')
  fireEvent.click(screen.getByRole('button', { name: 'Rename' }))
  const dialog = await screen.findByRole('dialog')
  fireEvent.change(within(dialog).getByLabelText('New name'), { target: { value: 'con' } })
  fireEvent.click(within(dialog).getByRole('button', { name: 'Rename' }))
  expect((await within(dialog).findByRole('alert')).textContent).toBe('That name is not allowed.')
  expect(screen.getByRole('dialog')).toBeTruthy()
})

it('plans a delete, confirms it by plan id and reports a partial failure', async () => {
  const cat = file('cat.png')
  const clip = file('clip.mp4', 'video')
  vi.mocked(gateway.galleryDeletePlan).mockResolvedValue({
    planId: 'plan-1',
    totalBytes: 4096,
    items: [
      { entry: refOf(cat), kind: 'file', bytes: 2048 },
      { entry: refOf(clip), kind: 'file', bytes: 2048 },
    ],
  })
  vi.mocked(gateway.galleryDelete).mockResolvedValue(
    report(
      result(cat),
      result(clip, { outcome: 'failed', error: { code: 'io', message: 'The file is in use.' } }),
    ),
  )
  mount()
  await select('cat.png', 'clip.mp4')
  fireEvent.click(screen.getByRole('button', { name: 'Delete' }))
  const dialog = await screen.findByRole('dialog')
  expect(await within(dialog).findByText('Delete 2 items?')).toBeTruthy()
  expect(gateway.galleryDeletePlan).toHaveBeenCalledWith([refOf(cat), refOf(clip)])
  expect(within(dialog).getByText(/4\.0 KiB in total/)).toBeTruthy()
  expect(gateway.galleryDelete).not.toHaveBeenCalled()
  fireEvent.click(within(dialog).getByRole('button', { name: 'Delete' }))
  await waitFor(() => expect(gateway.galleryDelete).toHaveBeenCalledWith('plan-1'))
  await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  expect(toast.success).toHaveBeenCalledWith('Deleted 1 item')
  expect(toast.error).toHaveBeenCalledWith('1 item was not fully processed', {
    description: 'clip.mp4: The file is in use.',
  })
})

it('asks to refresh when the folder changed under a delete plan', async () => {
  vi.mocked(gateway.galleryDeletePlan).mockRejectedValue(
    new IpcClientError('gallery_stale', 'raw stale message'),
  )
  mount()
  await select('cat.png')
  fireEvent.click(screen.getByRole('button', { name: 'Delete' }))
  const dialog = await screen.findByRole('dialog')
  expect(await within(dialog).findByText('Could not delete')).toBeTruthy()
  expect(within(dialog).getByText('The folder changed since it was listed. Refresh and try again.')).toBeTruthy()
  expect(gateway.galleryDelete).not.toHaveBeenCalled()
})

it('moves into the folder browsed in the dialog, which lists folders only', async () => {
  const cat = file('cat.png')
  vi.mocked(gateway.galleryMove).mockResolvedValue(
    report(result(cat, { destination: { area: 'input', path: 'archive/cat.png' } })),
  )
  mount()
  await select('cat.png')
  fireEvent.click(screen.getByRole('button', { name: 'Move to...' }))
  const dialog = await screen.findByRole('dialog')
  expect(within(dialog).getByText('Move 1 item')).toBeTruthy()
  const folders = await within(dialog).findByRole('list', { name: 'Folders' })
  expect(within(folders).getByText('archive')).toBeTruthy()
  expect(within(folders).getByText('renders')).toBeTruthy()
  expect(within(folders).queryByText('cat.png')).toBeNull()
  // A single click only highlights; entering a folder takes a double-click or Enter.
  fireEvent.click(within(folders).getByText('archive'))
  expect(within(dialog).queryByRole('button', { name: 'old' })).toBeNull()
  fireEvent.doubleClick(within(folders).getByText('archive'))
  expect(await within(dialog).findByText('old')).toBeTruthy()
  expect(within(dialog).getByRole('navigation', { name: 'Folder path' })).toBeTruthy()
  fireEvent.click(within(dialog).getByRole('button', { name: 'Move here' }))
  await waitFor(() =>
    expect(gateway.galleryMove).toHaveBeenCalledWith({
      items: [refOf(cat)],
      destination: { area: 'input', path: 'archive' },
    }),
  )
  await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  expect(toast.success).toHaveBeenCalledWith('Moved 1 item')
})

it('copies into another area and enters folders with the enter key', async () => {
  tree.set('output:', [folder('batch', '', 'output')])
  vi.mocked(gateway.galleryCopy).mockResolvedValue(report(result(file('cat.png'))))
  mount()
  await select('cat.png')
  fireEvent.click(screen.getByRole('button', { name: 'Copy to...' }))
  const dialog = await screen.findByRole('dialog')
  fireEvent.click(within(dialog).getByRole('radio', { name: 'Output' }))
  const batch = await within(dialog).findByText('batch')
  fireEvent.keyDown(batch, { key: 'Enter' })
  await waitFor(() =>
    expect(gateway.galleryList).toHaveBeenCalledWith({ area: 'output', path: 'batch' }),
  )
  await waitFor(() =>
    expect(within(dialog).getByRole<HTMLButtonElement>('button', { name: 'Copy here' }).disabled).toBe(false),
  )
  fireEvent.click(within(dialog).getByRole('button', { name: 'Copy here' }))
  await waitFor(() =>
    expect(gateway.galleryCopy).toHaveBeenCalledWith({
      items: [refOf(file('cat.png'))],
      destination: { area: 'output', path: 'batch' },
    }),
  )
  expect(gateway.galleryMove).not.toHaveBeenCalled()
  await waitFor(() => expect(toast.success).toHaveBeenCalledWith('Copied 1 item'))
})

it('names every item a move could not finish, including a retained source', async () => {
  const cat = file('cat.png')
  const clip = file('clip.mp4', 'video')
  vi.mocked(gateway.galleryMove).mockResolvedValue(
    report(
      result(cat, { outcome: 'source_retained', error: { code: 'io', message: 'The original is locked.' } }),
      result(clip, { outcome: 'failed', error: null }),
    ),
  )
  mount()
  await select('cat.png', 'clip.mp4')
  fireEvent.click(screen.getByRole('button', { name: 'Move to...' }))
  const dialog = await screen.findByRole('dialog')
  await waitFor(() =>
    expect(within(dialog).getByRole<HTMLButtonElement>('button', { name: 'Move here' }).disabled).toBe(false),
  )
  fireEvent.click(within(dialog).getByRole('button', { name: 'Move here' }))
  await waitFor(() =>
    expect(toast.error).toHaveBeenCalledWith('2 items were not fully processed', {
      description: 'cat.png: The original is locked.; clip.mp4: The operation could not be completed.',
    }),
  )
  expect(toast.success).not.toHaveBeenCalled()
})

it('toasts a busy backend and leaves the move dialog open to retry', async () => {
  vi.mocked(gateway.galleryMove).mockRejectedValue(
    new IpcClientError('gallery_busy', 'Another gallery operation is running.'),
  )
  mount()
  await select('cat.png')
  fireEvent.click(screen.getByRole('button', { name: 'Move to...' }))
  const dialog = await screen.findByRole('dialog')
  await waitFor(() =>
    expect(within(dialog).getByRole<HTMLButtonElement>('button', { name: 'Move here' }).disabled).toBe(false),
  )
  fireEvent.click(within(dialog).getByRole('button', { name: 'Move here' }))
  await waitFor(() =>
    expect(toast.error).toHaveBeenCalledWith('Another gallery operation is running.'),
  )
  expect(within(dialog).queryByRole('alert')).toBeNull()
  expect(screen.getByRole('dialog')).toBeTruthy()
})

it('previews an image at its gallery URL and steps through files, skipping folders', async () => {
  mount()
  fireEvent.doubleClick(await findTile('cat.png'))
  const dialog = await screen.findByRole('dialog')
  expect(within(dialog).getByText('cat.png')).toBeTruthy()
  expect(within(dialog).getByText(/2\.0 KiB · 1 of 4/)).toBeTruthy()
  expect(within(dialog).getByRole('img', { name: 'cat.png' }).getAttribute('src')).toBe(
    'http://gallery.localhost/input/cat.png?v=rev-cat.png',
  )
  expect(within(dialog).getByRole<HTMLButtonElement>('button', { name: 'Previous' }).disabled).toBe(true)

  fireEvent.keyDown(dialog, { key: 'ArrowRight' })
  expect(within(dialog).getByText('clip.mp4')).toBeTruthy()
  expect(dialog.querySelector('video')?.getAttribute('src')).toBe(
    'http://gallery.localhost/input/clip.mp4?v=rev-clip.mp4',
  )
  expect(dialog.querySelector('video')?.hasAttribute('autoplay')).toBe(false)

  fireEvent.click(within(dialog).getByRole('button', { name: 'Next' }))
  expect(await within(dialog).findByText('3D viewer for robot.glb')).toBeTruthy()

  fireEvent.keyDown(dialog, { key: 'ArrowRight' })
  expect(within(dialog).getByText('No preview for this file type')).toBeTruthy()
  expect(within(dialog).getByRole<HTMLButtonElement>('button', { name: 'Next' }).disabled).toBe(true)
  fireEvent.keyDown(dialog, { key: 'ArrowRight' })
  expect(within(dialog).getByText('notes.txt')).toBeTruthy()

  fireEvent.keyDown(dialog, { key: 'ArrowLeft' })
  expect(within(dialog).getByText('3D viewer for robot.glb')).toBeTruthy()
})

it('opens a preview with the enter key and shows the file in the system file manager', async () => {
  mount()
  fireEvent.keyDown(await findTile('notes.txt'), { key: 'Enter' })
  const dialog = await screen.findByRole('dialog')
  expect(within(dialog).getByText('No preview for this file type')).toBeTruthy()
  fireEvent.click(within(dialog).getByRole('button', { name: 'Show in Explorer' }))
  expect(gateway.galleryReveal).toHaveBeenCalledWith(file('notes.txt', 'other').location)
})

it('falls back with a message when an image, video or 3D file cannot be previewed', async () => {
  viewer.fail = true
  mount()
  fireEvent.doubleClick(await findTile('cat.png'))
  const dialog = await screen.findByRole('dialog')
  fireEvent.error(within(dialog).getByRole('img', { name: 'cat.png' }))
  expect(within(dialog).getByText("This file can't be previewed.")).toBeTruthy()

  fireEvent.keyDown(dialog, { key: 'ArrowRight' })
  fireEvent.error(dialog.querySelector('video') as HTMLVideoElement)
  expect(within(dialog).getByText("This file can't be previewed.")).toBeTruthy()

  fireEvent.keyDown(dialog, { key: 'ArrowRight' })
  expect(await within(dialog).findByText("This 3D file can't be previewed.")).toBeTruthy()
  expect(within(dialog).getByRole('button', { name: 'Show in Explorer' })).toBeTruthy()
})

it('tells the user to install ComfyUI when the gallery has no install', async () => {
  vi.mocked(gateway.galleryList).mockRejectedValue(
    new IpcClientError('gallery_not_installed', 'ComfyUI is not installed.'),
  )
  mount()
  expect(await screen.findByText('ComfyUI is not installed yet')).toBeTruthy()
  expect(screen.getByText(/Install ComfyUI first/)).toBeTruthy()
  expect(screen.queryByText('Unable to complete the request')).toBeNull()
  expect(screen.getByRole<HTMLButtonElement>('button', { name: 'Open in Explorer' }).disabled).toBe(true)
})

it('shows other failures inline and retries', async () => {
  vi.mocked(gateway.galleryList)
    .mockRejectedValueOnce(new IpcClientError('io', 'The folder could not be read.'))
    .mockResolvedValue(listing([file('cat.png')]))
  mount()
  expect(await screen.findByText('The folder could not be read.')).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: 'Try again' }))
  expect(await findTile('cat.png')).toBeTruthy()
})

it('shows an empty folder and a muted count of entries that could not be listed', async () => {
  tree.set('input:', [])
  vi.mocked(gateway.galleryList).mockResolvedValue(
    listing([], { area: 'input', path: '' }, [
      { name: 'bad?', code: 'unsafe_name' },
      { name: null, code: 'link' },
    ]),
  )
  mount()
  expect(await screen.findByText('This folder is empty')).toBeTruthy()
  expect(screen.getByText('2 items could not be shown.')).toBeTruthy()
  expect(screen.queryByText(/bad\?/)).toBeNull()
})

it('refreshes the listing on demand', async () => {
  mount()
  await findTile('cat.png')
  const before = vi.mocked(gateway.galleryList).mock.calls.length
  fireEvent.click(screen.getByRole('button', { name: 'Refresh' }))
  await waitFor(() => expect(vi.mocked(gateway.galleryList).mock.calls.length).toBeGreaterThan(before))
})
