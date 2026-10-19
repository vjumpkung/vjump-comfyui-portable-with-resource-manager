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

import {
  gateway,
  type ComfyEventDto,
  type ComfyStatusDto,
} from '@/shared/ipc/gateway'

import { ComfyPage } from './comfy-page'
import { CloseRequestDialog } from './close-request-dialog'

vi.mock('@/shared/ui/log-view', () => ({
  StreamLogView: ({ text }: { text: string }) => (
    <pre aria-label="ComfyUI log">{text}</pre>
  ),
}))
vi.mock('@/shared/ipc/gateway', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/ipc/gateway')>()),
  gateway: {
    comfySubscribe: vi.fn(),
    comfyUnsubscribe: vi.fn(),
    comfyStatus: vi.fn(),
    comfyStart: vi.fn(),
    comfyStop: vi.fn(),
    comfyRestart: vi.fn(),
    comfyGetLaunchOptions: vi.fn(),
    comfySetLaunchOptions: vi.fn(),
    comfyListVersions: vi.fn(),
    comfyUpdate: vi.fn(),
    comfyReadExtraModelPaths: vi.fn(),
    comfyOpenWindow: vi.fn(),
    comfyOpenBrowser: vi.fn(),
    appCopyDiagnostics: vi.fn(),
    appCloseDecide: vi.fn(),
    envInfo: vi.fn(),
  },
}))

const stopped: ComfyStatusDto = {
  lifecycle: 'stopped',
  session: null,
  failure: null,
  pendingOperation: null,
  recovery: null,
  origin: null,
  pendingCloseRequest: null,
}
const options = {
  port: 8188,
  vram: 'auto' as const,
  attention: 'default' as const,
  preview: 'auto' as const,
  enableManager: false,
  listenLan: false,
  extraArgs: [],
}
let emit: (event: ComfyEventDto) => void = () => undefined

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
  vi.mocked(gateway.comfySubscribe).mockImplementation((_after, handler) => {
    emit = handler
    return Promise.resolve(1)
  })
  vi.mocked(gateway.comfyUnsubscribe).mockResolvedValue(null)
  vi.mocked(gateway.comfyStatus).mockResolvedValue(stopped)
  vi.mocked(gateway.comfyGetLaunchOptions).mockResolvedValue(options)
  vi.mocked(gateway.comfyListVersions).mockResolvedValue({
    current: { commit: 'a'.repeat(40), tag: 'v0.40.0', branch: null },
    tags: ['v0.41.0', 'v0.40.0'],
    pendingUpdate: null,
  })
  vi.mocked(gateway.comfyReadExtraModelPaths).mockResolvedValue([])
})
afterEach(cleanup)

function mount(children: React.ReactNode) {
  return render(
    <QueryClientProvider client={new QueryClient()}>
      {children}
    </QueryClientProvider>,
  )
}

async function snapshot(status = stopped) {
  await waitFor(() => expect(gateway.comfySubscribe).toHaveBeenCalled())
  emit({ kind: 'snapshot', cursor: 1, status, logs: [], gap: false })
}

it('offers the suggested port after a conflict and starts only after acceptance', async () => {
  vi.mocked(gateway.comfyStart)
    .mockResolvedValueOnce({
      kind: 'port_conflict',
      requested: 8188,
      suggested: 8189,
    })
    .mockResolvedValueOnce({
      kind: 'started',
      status: { ...stopped, lifecycle: 'running' },
    })
  vi.mocked(gateway.comfySetLaunchOptions).mockImplementation((next) =>
    Promise.resolve(next),
  )
  mount(<ComfyPage />)
  await snapshot()
  fireEvent.click(await screen.findByRole('button', { name: /^Start$/ }))
  fireEvent.click(await screen.findByRole('button', { name: 'Use port 8189' }))
  await waitFor(() =>
    expect(gateway.comfySetLaunchOptions).toHaveBeenCalledWith({
      ...options,
      port: 8189,
    }),
  )
  await waitFor(() => expect(gateway.comfyStart).toHaveBeenCalledTimes(2))
})

it('shows the crash reason and live log text', async () => {
  mount(<ComfyPage />)
  await snapshot({
    ...stopped,
    lifecycle: 'crashed',
    failure: {
      kind: 'unexpected_exit',
      exitCode: 3,
      message: 'ComfyUI exited unexpectedly',
    },
  })
  emit({
    kind: 'comfy:log',
    chunk: { cursor: 2, sessionId: 's', text: 'Traceback line\n' },
  })
  expect(await screen.findByText('ComfyUI stopped unexpectedly')).toBeTruthy()
  expect(screen.getByText(/exit code 3/)).toBeTruthy()
  expect(await screen.findByText(/Traceback line/)).toBeTruthy()
})

it('refuses to enable network exposure without acknowledgement', async () => {
  vi.mocked(gateway.comfySetLaunchOptions).mockImplementation((next) =>
    Promise.resolve(next),
  )
  mount(<ComfyPage />)
  await snapshot()
  fireEvent.click(
    await screen.findByLabelText(/Listen on the local network/),
  )
  const save = screen.getByRole('button', { name: 'Save launch options' })
  expect((save as HTMLButtonElement).disabled).toBe(true)
  fireEvent.click(screen.getByLabelText('I understand the risk'))
  expect((save as HTMLButtonElement).disabled).toBe(false)
})

it('shows the git status when an update is refused and nothing changed', async () => {
  vi.mocked(gateway.comfyUpdate).mockResolvedValue({
    kind: 'refused',
    reason: 'the working tree has local changes',
    status: ' M main.py',
  })
  mount(<ComfyPage />)
  await snapshot()
  fireEvent.click(
    await screen.findByRole('button', { name: 'Update to latest release' }),
  )
  fireEvent.click(await screen.findByRole('button', { name: 'Update' }))
  expect(
    await screen.findByText(/Update refused: the working tree has local changes/),
  ).toBeTruthy()
  expect(screen.getByText('M main.py')).toBeTruthy()
})

it('copies the redacted app report and falls back to selectable text', async () => {
  vi.mocked(gateway.appCopyDiagnostics)
    .mockResolvedValueOnce({ text: 'REPORT', copied: true })
    .mockResolvedValueOnce({ text: 'REPORT', copied: false })
  mount(<ComfyPage />)
  await snapshot()
  const copy = await screen.findByRole('button', { name: 'Copy diagnostics' })

  fireEvent.click(copy)
  await waitFor(() => expect(gateway.appCopyDiagnostics).toHaveBeenCalledTimes(1))
  expect(screen.queryByLabelText('Diagnostics report')).toBeNull()

  await waitFor(() => expect((copy as HTMLButtonElement).disabled).toBe(false))
  fireEvent.click(copy)
  const report = await screen.findByLabelText<HTMLTextAreaElement>(
    'Diagnostics report',
  )
  expect(report.readOnly).toBe(true)
  expect(report.value).toBe('REPORT')
})

it('asks before closing and sends the chosen answer', async () => {
  vi.mocked(gateway.appCloseDecide).mockResolvedValue(null)
  mount(<CloseRequestDialog />)
  await snapshot({
    ...stopped,
    lifecycle: 'running',
    pendingCloseRequest: 'close-1',
  })
  fireEvent.click(
    await screen.findByRole('button', { name: 'Keep running in background' }),
  )
  await waitFor(() =>
    expect(gateway.appCloseDecide).toHaveBeenCalledWith(
      'close-1',
      'keep_running',
    ),
  )
})
