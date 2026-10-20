// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from '@testing-library/react'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'

import { gateway, type DownloadEventDto } from '@/shared/ipc/gateway'

import { DownloadsPage } from './downloads-page'
import { job } from './fixtures'

vi.mock('@/shared/ipc/gateway', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/ipc/gateway')>()),
  gateway: {
    dlSubscribe: vi.fn(),
    dlUnsubscribe: vi.fn(),
    dlResolve: vi.fn(),
    dlEnqueue: vi.fn(),
    dlPause: vi.fn(),
    dlResume: vi.fn(),
    dlCancel: vi.fn(),
    dlRetry: vi.fn(),
    dlImportPack: vi.fn(),
    dlExport: vi.fn(),
  },
}))

let emit: (event: DownloadEventDto) => void = () => undefined
const openSettings = vi.fn()

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
  vi.mocked(gateway.dlSubscribe).mockImplementation((_after, handler) => {
    emit = handler
    return Promise.resolve(1)
  })
  vi.mocked(gateway.dlUnsubscribe).mockResolvedValue(null)
})
afterEach(cleanup)

function mount() {
  return render(
    <QueryClientProvider client={new QueryClient()}>
      <DownloadsPage onOpenSettings={openSettings} />
    </QueryClientProvider>,
  )
}

async function snapshot(jobs = [job()]) {
  await waitFor(() => expect(gateway.dlSubscribe).toHaveBeenCalled())
  emit({
    kind: 'snapshot',
    cursor: { epoch: 'e', sequence: 1 },
    jobs,
    gap: false,
  })
}

it('shows the empty state with an Add download action', async () => {
  mount()
  await snapshot([])
  expect(await screen.findByText('No downloads yet')).toBeTruthy()
  expect(screen.getByRole('button', { name: /Add download/ })).toBeTruthy()
})

it('shows progress, speed and ETA and pauses a running job', async () => {
  vi.mocked(gateway.dlPause).mockResolvedValue(job({ state: 'paused' }))
  mount()
  await snapshot([job({ state: 'downloading' })])
  emit({
    kind: 'job:progress',
    cursor: { epoch: 'e', sequence: 2 },
    progress: {
      id: 'job-1',
      generation: 1,
      bytes: 500,
      total: { kind: 'exact', bytes: 1000 },
      bytesPerSecond: 100,
      etaSeconds: 5,
      estimated: false,
    },
  })
  expect(await screen.findByText(/500 B \/ 1000 B/)).toBeTruthy()
  expect(screen.getByText(/100 B\/s/)).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: 'Pause model.safetensors' }))
  await waitFor(() => expect(gateway.dlPause).toHaveBeenCalledWith('job-1'))
})

it('warns that resuming a tool-driven job restarts from 0%', async () => {
  vi.mocked(gateway.dlResume).mockResolvedValue(job({ state: 'queued' }))
  mount()
  await snapshot([job({ state: 'paused', restartsFromZero: true })])
  expect(await screen.findByText(/restarts this download from 0%/)).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: 'Resume model.safetensors' }))
  await waitFor(() =>
    expect(gateway.dlResume).toHaveBeenCalledWith('job-1', null),
  )
})

it('links a missing-token failure to Settings and offers retry', async () => {
  vi.mocked(gateway.dlRetry).mockResolvedValue(job({ state: 'queued' }))
  mount()
  await snapshot([
    job({
      state: 'failed',
      error: {
        code: 'token_required',
        message: 'This download needs an access token. Add one in Settings.',
      },
    }),
  ])
  fireEvent.click(await screen.findByRole('button', { name: 'Open Settings' }))
  expect(openSettings).toHaveBeenCalled()
  fireEvent.click(screen.getByRole('button', { name: 'Retry model.safetensors' }))
  await waitFor(() => expect(gateway.dlRetry).toHaveBeenCalledWith('job-1'))
})

it('resolves a link, requires a folder and queues the chosen file', async () => {
  vi.mocked(gateway.dlResolve).mockResolvedValue({
    id: 7,
    selectionRequired: false,
    files: [
      {
        index: 0,
        provider: 'civitai',
        label: 'CivitAI version 1, file 2',
        filename: 'real.safetensors',
        size: { kind: 'exact', bytes: 2048 },
        sha256: 'a'.repeat(64),
        suggestedCategory: 'loras',
        primary: true,
      },
    ],
  })
  vi.mocked(gateway.dlEnqueue).mockResolvedValue([
    { kind: 'queued', jobId: 'job-9' },
  ])
  mount()
  await snapshot([])
  fireEvent.click(screen.getByRole('button', { name: /Add download/ }))
  fireEvent.change(await screen.findByLabelText('Link'), {
    target: { value: 'https://civitai.com/models/1' },
  })
  fireEvent.click(screen.getByRole('button', { name: 'Look up' }))
  expect(await screen.findByText('real.safetensors')).toBeTruthy()
  expect(screen.getByLabelText<HTMLSelectElement>('Model folder').value).toBe(
    'loras',
  )
  fireEvent.change(screen.getByLabelText('Sub-folder (optional)'), {
    target: { value: 'sdxl' },
  })
  fireEvent.click(screen.getByRole('button', { name: 'Download' }))
  await waitFor(() =>
    expect(gateway.dlEnqueue).toHaveBeenCalledWith([
      {
        resolutionId: 7,
        fileIndex: 0,
        category: 'loras',
        subdirectory: 'sdxl',
        filename: null,
        conflict: null,
        force: false,
      },
    ]),
  )
})

it('asks about a name conflict and re-queues with the chosen answer', async () => {
  vi.mocked(gateway.dlResolve).mockResolvedValue({
    id: 3,
    selectionRequired: false,
    files: [
      {
        index: 0,
        provider: 'direct',
        label: 'https://example.com/a.bin',
        filename: 'a.bin',
        size: { kind: 'unknown', bytes: null },
        sha256: null,
        suggestedCategory: null,
        primary: false,
      },
    ],
  })
  const current = {
    size: 1,
    modifiedNs: '5',
    platformId: 'x',
    digest: null,
  }
  vi.mocked(gateway.dlEnqueue)
    .mockResolvedValueOnce([
      {
        kind: 'conflict',
        destination: {
          rootId: 'models',
          category: 'vae',
          subdirectory: '',
          filename: 'a.bin',
          relativePath: 'vae/a.bin',
        },
        current,
      },
    ])
    .mockResolvedValueOnce([{ kind: 'queued', jobId: 'job-2' }])
  mount()
  await snapshot([])
  fireEvent.click(screen.getByRole('button', { name: /Add download/ }))
  fireEvent.change(await screen.findByLabelText('Link'), {
    target: { value: 'https://example.com/a.bin' },
  })
  fireEvent.click(screen.getByRole('button', { name: 'Look up' }))
  await screen.findByText('a.bin')
  fireEvent.change(screen.getByLabelText('Model folder'), {
    target: { value: 'vae' },
  })
  fireEvent.click(screen.getByRole('button', { name: 'Download' }))
  fireEvent.click(await screen.findByRole('button', { name: 'Keep both' }))
  await waitFor(() => expect(gateway.dlEnqueue).toHaveBeenCalledTimes(2))
  expect(vi.mocked(gateway.dlEnqueue).mock.calls[1][0][0].conflict).toEqual({
    kind: 'keep_both',
  })
})

it('previews a pack with sizes and queues only the selected rows', async () => {
  const file = (name: string, bytes: number) => ({
    index: 0,
    provider: 'direct' as const,
    label: name,
    filename: name,
    size: { kind: 'exact' as const, bytes },
    sha256: null,
    suggestedCategory: null,
    primary: false,
  })
  vi.mocked(gateway.dlImportPack).mockResolvedValue([
    {
      name: 'one',
      category: 'loras',
      resolution: { id: 1, selectionRequired: false, files: [file('one.bin', 1024)] },
      error: null,
    },
    {
      name: 'two',
      category: 'vae',
      resolution: { id: 2, selectionRequired: false, files: [file('two.bin', 2048)] },
      error: null,
    },
    {
      name: 'broken',
      category: 'vae',
      resolution: null,
      error: { code: 'not_found', message: 'The download was not found.' },
    },
  ])
  vi.mocked(gateway.dlEnqueue).mockResolvedValue([
    { kind: 'queued', jobId: 'job-a' },
  ])
  mount()
  await snapshot([])
  fireEvent.click(screen.getByRole('button', { name: /Import pack/ }))
  fireEvent.change(await screen.findByLabelText('From a link (https)'), {
    target: { value: 'https://example.com/list.json' },
  })
  fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
  expect(await screen.findByText('The download was not found.')).toBeTruthy()
  fireEvent.click(screen.getByRole('checkbox', { name: 'Include two' }))
  fireEvent.click(await screen.findByRole('button', { name: /Queue 1/ }))
  await waitFor(() =>
    expect(gateway.dlEnqueue).toHaveBeenCalledWith([
      {
        resolutionId: 1,
        fileIndex: 0,
        category: 'loras',
        subdirectory: '',
        filename: null,
        conflict: null,
        force: false,
      },
    ]),
  )
})

it('echoes the observed destination identity when overwriting after a late conflict', async () => {
  const observed = {
    size: 5,
    modifiedNs: '123',
    platformId: 'vol:1',
    digest: null,
  }
  vi.mocked(gateway.dlResume).mockResolvedValue(job({ state: 'queued' }))
  mount()
  await snapshot([
    job({
      state: 'paused',
      decisionId: 'decision-1',
      conflictCurrent: observed,
      error: { code: 'conflict', message: 'A file with that name already exists.' },
    }),
  ])
  fireEvent.click(await screen.findByRole('button', { name: /Resolve conflict/ }))
  fireEvent.click(
    await screen.findByRole('button', { name: /Overwrite \(move old to/ }),
  )
  await waitFor(() =>
    expect(gateway.dlResume).toHaveBeenCalledWith('job-1', {
      decisionId: 'decision-1',
      choice: { kind: 'overwrite', approved: observed },
    }),
  )
})
