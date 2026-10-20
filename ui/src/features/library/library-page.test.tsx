// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'

import {
  gateway,
  IpcClientError,
  type DeleteReportDto,
  type ModelEventDto,
} from '@/shared/ipc/gateway'

import { many, model, snapshot, status } from './fixtures'
import { LibraryPage } from './library-page'
import { createLibraryStore } from './library-store'

vi.mock('@/shared/ipc/gateway', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/ipc/gateway')>()),
  gateway: {
    modelSubscribe: vi.fn(),
    modelUnsubscribe: vi.fn(),
    modelList: vi.fn(),
    modelRescan: vi.fn(),
    modelDetails: vi.fn(),
    modelRename: vi.fn(),
    modelMove: vi.fn(),
    modelDeletePlan: vi.fn(),
    modelDelete: vi.fn(),
    modelDeletePermanently: vi.fn(),
    modelIdentify: vi.fn(),
    modelOperationStatus: vi.fn(),
  },
}))
vi.mock('sonner', () => ({
  toast: { error: vi.fn(), success: vi.fn(), info: vi.fn() },
}))

const stopComfy = vi.fn()
let emit: (event: ModelEventDto) => void = () => undefined

beforeEach(() => {
  vi.stubGlobal(
    'ResizeObserver',
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  )
  vi.resetAllMocks()
  stopComfy.mockResolvedValue(undefined)
  vi.mocked(gateway.modelSubscribe).mockImplementation((handler) => {
    emit = handler
    return Promise.resolve(1)
  })
  vi.mocked(gateway.modelUnsubscribe).mockResolvedValue(null)
  vi.mocked(gateway.modelOperationStatus).mockResolvedValue([])
  vi.mocked(gateway.modelDetails).mockResolvedValue({
    civitai: null,
    previewDataUrl: null,
  })
})
afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

function mount(list = snapshot([model()])) {
  vi.mocked(gateway.modelList).mockResolvedValue(list)
  const store = createLibraryStore({
    modelSubscribe: gateway.modelSubscribe,
    modelUnsubscribe: gateway.modelUnsubscribe,
    modelList: gateway.modelList,
    modelRescan: gateway.modelRescan,
  })
  render(
    <QueryClientProvider client={new QueryClient()}>
      <LibraryPage onStopComfy={stopComfy} store={store} />
    </QueryClientProvider>,
  )
  return store
}

const row = (name: string) =>
  screen.getByRole('row', { name: new RegExp(name.replace('.', '\\.')) })

it('lists models and filters them by search text and category', async () => {
  mount(
    snapshot([
      model({ name: 'alpha.safetensors' }),
      model({ name: 'beta.pt', category: 'loras' }),
    ]),
  )
  expect(await screen.findByText('alpha.safetensors')).toBeTruthy()
  expect(screen.getByText('beta.pt')).toBeTruthy()
  fireEvent.change(screen.getByLabelText('Search models'), {
    target: { value: 'BETA' },
  })
  expect(screen.queryByText('alpha.safetensors')).toBeNull()
  fireEvent.change(screen.getByLabelText('Search models'), {
    target: { value: '' },
  })
  fireEvent.change(screen.getByLabelText('Category'), {
    target: { value: 'checkpoints' },
  })
  expect(screen.queryByText('beta.pt')).toBeNull()
  fireEvent.change(screen.getByLabelText('Search models'), {
    target: { value: 'zzz' },
  })
  expect(await screen.findByText('No models match')).toBeTruthy()
})

it('shows the empty state and the watcher-degraded banner', async () => {
  mount(snapshot([], { watcher: 'polling' }))
  expect(await screen.findByText('No models found')).toBeTruthy()
  expect(screen.getByText('File watching is degraded')).toBeTruthy()
  expect(screen.getByText(/checked every 30 seconds/)).toBeTruthy()
})

it('does not show a banner while native watching works', async () => {
  mount(snapshot([model()]))
  await screen.findByText('model.safetensors')
  expect(screen.queryByText('File watching is degraded')).toBeNull()
})

it('renders only a window of a long list and keeps the full row count', async () => {
  const sizes = {
    offsetHeight: Object.getOwnPropertyDescriptor(
      HTMLElement.prototype,
      'offsetHeight',
    ),
    offsetWidth: Object.getOwnPropertyDescriptor(
      HTMLElement.prototype,
      'offsetWidth',
    ),
  }
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', {
    configurable: true,
    value: 480,
  })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', {
    configurable: true,
    value: 900,
  })
  try {
    mount(snapshot(many(1000)))
    await screen.findByRole('grid', { name: 'Models' })
    await waitFor(() => {
      const rendered = screen
        .getAllByRole('row')
        .filter((r) => r.hasAttribute('data-key')).length
      expect(rendered).toBeGreaterThan(0)
      expect(rendered).toBeLessThan(60)
    })
    expect(screen.getByRole('grid', { name: 'Models' }).getAttribute('aria-rowcount')).toBe(
      '1001',
    )
  } finally {
    for (const [name, descriptor] of Object.entries(sizes)) {
      if (descriptor) Object.defineProperty(HTMLElement.prototype, name, descriptor)
    }
  }
})

it('moves focus with the arrow keys and opens delete with the Delete key', async () => {
  vi.mocked(gateway.modelDeletePlan).mockResolvedValue({
    id: 'plan-1',
    permanent: false,
    totalBytes: 100,
    items: [
      { location: model({ name: 'b.pt' }).location, name: 'b.pt', size: 100 },
    ],
  })
  mount(snapshot([model({ name: 'a.pt' }), model({ name: 'b.pt' })]))
  const first = await screen.findByRole('row', { name: /a\.pt/ })
  first.focus()
  fireEvent.click(first)
  fireEvent.keyDown(first, { key: 'ArrowDown' })
  await waitFor(() => expect(document.activeElement).toBe(row('b.pt')))
  fireEvent.keyDown(row('b.pt'), { key: 'Delete' })
  expect(await screen.findByText(/Delete 1 file\?/)).toBeTruthy()
})

it('shows CivitAI details and preview of the selected model and identifies it', async () => {
  vi.mocked(gateway.modelDetails).mockResolvedValue({
    civitai: {
      versionId: 5,
      modelId: 6,
      modelName: 'Cool Model',
      versionName: 'v1',
      baseModel: 'SDXL 1.0',
      triggerWords: ['zzz style'],
    },
    previewDataUrl: 'data:image/png;base64,AAAA',
  })
  vi.mocked(gateway.modelIdentify)
    .mockResolvedValueOnce({
      kind: 'sidecars_exist',
      version: {
        versionId: 5,
        modelId: 6,
        modelName: 'Cool Model',
        versionName: 'v1',
        baseModel: null,
        triggerWords: [],
      },
      sha256: 'a'.repeat(64),
    })
    .mockResolvedValueOnce({
      kind: 'identified',
      version: {
        versionId: 5,
        modelId: 6,
        modelName: 'Cool Model',
        versionName: 'v1',
        baseModel: null,
        triggerWords: [],
      },
      sha256: 'a'.repeat(64),
      preview: 'written',
    })
  mount(
    snapshot([
      model({
        sidecars: { preview: true, civitaiInfo: true, json: false },
        civitaiVersionId: 5,
      }),
    ]),
  )
  fireEvent.click(await screen.findByRole('row', { name: /model\.safetensors/ }))
  expect(await screen.findByText('Cool Model')).toBeTruthy()
  expect(screen.getByText('SDXL 1.0')).toBeTruthy()
  expect(screen.getByText('zzz style')).toBeTruthy()
  expect(screen.getByRole('img').getAttribute('src')).toBe(
    'data:image/png;base64,AAAA',
  )
  fireEvent.click(screen.getByRole('button', { name: /Identify on CivitAI/ }))
  expect(await screen.findByText('Replace existing metadata?')).toBeTruthy()
  expect(gateway.modelIdentify).toHaveBeenLastCalledWith(
    expect.objectContaining({ replaceExisting: false }),
  )
  fireEvent.click(screen.getByRole('button', { name: 'Replace' }))
  await waitFor(() =>
    expect(gateway.modelIdentify).toHaveBeenLastCalledWith(
      expect.objectContaining({ replaceExisting: true }),
    ),
  )
})

it('warns when a rename changes the extension and requires acknowledgement', async () => {
  vi.mocked(gateway.modelRename).mockResolvedValue(
    model({ name: 'new.ckpt' }).location,
  )
  mount()
  fireEvent.click(await screen.findByRole('row', { name: /model\.safetensors/ }))
  fireEvent.click(screen.getByRole('button', { name: 'Rename' }))
  const input = await screen.findByLabelText('New name')
  fireEvent.change(input, { target: { value: 'new.ckpt' } })
  expect(await screen.findByText('The file extension changes')).toBeTruthy()
  const submit = screen.getByRole('button', { name: 'Rename', hidden: false })
  expect((submit as HTMLButtonElement).disabled).toBe(true)
  fireEvent.click(screen.getByLabelText('Change the extension anyway'))
  await waitFor(() => expect((submit as HTMLButtonElement).disabled).toBe(false))
  fireEvent.click(submit)
  await waitFor(() =>
    expect(gateway.modelRename).toHaveBeenCalledWith({
      location: model().location,
      newName: 'new.ckpt',
      allowExtensionChange: true,
    }),
  )
})

it('offers Retry and Stop ComfyUI and retry when the file is locked', async () => {
  vi.mocked(gateway.modelRename)
    .mockRejectedValueOnce(new IpcClientError('model_locked', 'in use'))
    .mockResolvedValueOnce(model({ name: 'renamed.safetensors' }).location)
  mount()
  fireEvent.click(await screen.findByRole('row', { name: /model\.safetensors/ }))
  fireEvent.click(screen.getByRole('button', { name: 'Rename' }))
  fireEvent.change(await screen.findByLabelText('New name'), {
    target: { value: 'renamed.safetensors' },
  })
  fireEvent.click(screen.getByRole('button', { name: 'Rename' }))
  expect(await screen.findByText('The file is in use')).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: 'Stop ComfyUI and retry' }))
  await waitFor(() => expect(gateway.modelRename).toHaveBeenCalledTimes(2))
  expect(stopComfy).toHaveBeenCalledTimes(1)
  await waitFor(() => expect(screen.queryByText('The file is in use')).toBeNull())
})

it('moves a model to another folder', async () => {
  vi.mocked(gateway.modelMove).mockResolvedValue(
    model({ name: 'model.safetensors', category: 'loras' }).location,
  )
  mount()
  fireEvent.click(await screen.findByRole('row', { name: /model\.safetensors/ }))
  fireEvent.click(screen.getByRole('button', { name: 'Move' }))
  const directory = await screen.findByLabelText('Category and sub-folder')
  fireEvent.change(directory, { target: { value: 'loras/styles' } })
  fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Move' }))
  await waitFor(() =>
    expect(gateway.modelMove).toHaveBeenCalledWith({
      location: model().location,
      targetRootId: 'models',
      targetDirectory: 'loras/styles',
    }),
  )
})

function plan(names: string[], permanent = false, size = 1024 ** 3) {
  return {
    id: permanent ? 'perm-plan' : 'plan-1',
    permanent,
    totalBytes: size * names.length,
    items: names.map((name) => ({
      location: model({ name }).location,
      name,
      size,
    })),
  }
}

it('confirms with the total size, reports every file and separates permanent deletion', async () => {
  vi.mocked(gateway.modelDeletePlan).mockResolvedValue(plan(['a.pt', 'b.pt']))
  const report: DeleteReportDto = {
    results: [
      { location: model({ name: 'a.pt' }).location, outcome: { kind: 'deleted' } },
      {
        location: model({ name: 'b.pt' }).location,
        outcome: { kind: 'trash_failed' },
      },
    ],
    permanentPlan: plan(['b.pt'], true),
  }
  vi.mocked(gateway.modelDelete).mockResolvedValue(report)
  vi.mocked(gateway.modelDeletePermanently).mockResolvedValue({
    results: [
      { location: model({ name: 'b.pt' }).location, outcome: { kind: 'deleted' } },
    ],
    permanentPlan: null,
  })
  mount(snapshot([model({ name: 'a.pt' }), model({ name: 'b.pt' })]))
  await screen.findByText('a.pt')
  fireEvent.click(screen.getByLabelText('Select all'))
  fireEvent.click(screen.getByRole('button', { name: /Delete 2 selected/ }))
  const confirm = await screen.findByRole('button', {
    name: /Move 2 files \(2\.0 GiB\) to (Recycle Bin|Trash)/,
  })
  fireEvent.click(confirm)
  // Per-file results; nothing is unlinked automatically after the trash failure.
  expect(await screen.findByText('1 of 2 files deleted')).toBeTruthy()
  expect(screen.getByText(/Could not be moved; nothing was deleted/)).toBeTruthy()
  expect(gateway.modelDeletePermanently).not.toHaveBeenCalled()
  fireEvent.click(screen.getByRole('button', { name: 'Review permanent deletion' }))
  const permanent = await screen.findByRole('button', {
    name: /Delete permanently \(1\.0 GiB\)/,
  })
  expect((permanent as HTMLButtonElement).disabled).toBe(true)
  fireEvent.click(screen.getByLabelText('I understand this cannot be undone'))
  await waitFor(() => expect((permanent as HTMLButtonElement).disabled).toBe(false))
  fireEvent.click(permanent)
  await waitFor(() =>
    expect(gateway.modelDeletePermanently).toHaveBeenCalledWith('perm-plan'),
  )
})

it('retries locked files after stopping ComfyUI with a fresh plan', async () => {
  vi.mocked(gateway.modelDeletePlan)
    .mockResolvedValueOnce(plan(['a.pt']))
    .mockResolvedValueOnce({ ...plan(['a.pt']), id: 'plan-2' })
  vi.mocked(gateway.modelDelete)
    .mockResolvedValueOnce({
      results: [
        { location: model({ name: 'a.pt' }).location, outcome: { kind: 'locked' } },
      ],
      permanentPlan: null,
    })
    .mockResolvedValueOnce({
      results: [
        { location: model({ name: 'a.pt' }).location, outcome: { kind: 'deleted' } },
      ],
      permanentPlan: null,
    })
  mount(snapshot([model({ name: 'a.pt' })]))
  fireEvent.click(await screen.findByRole('row', { name: /a\.pt/ }))
  fireEvent.click(screen.getByRole('button', { name: 'Delete' }))
  fireEvent.click(
    await screen.findByRole('button', { name: /Move 1 file .* to (Recycle Bin|Trash)/ }),
  )
  expect(await screen.findByText(/In use by another program/)).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: 'Stop ComfyUI and retry' }))
  expect(await screen.findByText('1 of 1 file deleted')).toBeTruthy()
  expect(stopComfy).toHaveBeenCalledTimes(1)
  expect(gateway.modelDelete).toHaveBeenLastCalledWith('plan-2')
})

it('refetches the list when the backend announces a change', async () => {
  mount(snapshot([model({ name: 'old.pt' })]))
  await screen.findByText('old.pt')
  vi.mocked(gateway.modelList).mockResolvedValue(snapshot([model({ name: 'new.pt' })]))
  emit({ kind: 'models:changed', epoch: 'e', status: status({ sequence: 9 }) })
  expect(await screen.findByText('new.pt')).toBeTruthy()
  expect(screen.queryByText('old.pt')).toBeNull()
})

it('model_table_active_row_keeps_readable_contrast', async () => {
  mount(snapshot([model({ name: 'a.pt' }), model({ name: 'b.pt' })]))
  fireEvent.click(await screen.findByRole('row', { name: /a\.pt/ }))
  // No brand fill behind table text: the active row is muted with an accent bar.
  const active = row('a.pt')
  expect(active.classList.contains('bg-muted')).toBe(true)
  expect(active.classList.contains('bg-accent')).toBe(false)
  expect(row('b.pt').classList.contains('bg-accent')).toBe(false)
})
