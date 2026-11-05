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
  type StartResultDto,
} from '@/shared/ipc/gateway'

import { RuntimeControls } from './runtime-controls'

vi.mock('@/shared/ipc/gateway', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/ipc/gateway')>()),
  gateway: {
    comfySubscribe: vi.fn(),
    comfyUnsubscribe: vi.fn(),
    comfyStatus: vi.fn(),
    comfyStart: vi.fn(),
    comfyStop: vi.fn(),
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
let emit: (event: ComfyEventDto) => void = () => undefined

beforeEach(() => {
  vi.resetAllMocks()
  vi.mocked(gateway.comfySubscribe).mockImplementation((_after, handler) => {
    emit = handler
    return Promise.resolve(1)
  })
  vi.mocked(gateway.comfyUnsubscribe).mockResolvedValue(null)
  vi.mocked(gateway.comfyStatus).mockResolvedValue(stopped)
})
afterEach(cleanup)

it('marks the top-bar Start busy and disabled while a launch is pending', async () => {
  let finishStart: (result: StartResultDto) => void = () => undefined
  vi.mocked(gateway.comfyStart).mockReturnValueOnce(
    new Promise<StartResultDto>((resolve) => {
      finishStart = resolve
    }),
  )
  render(
    <QueryClientProvider client={new QueryClient()}>
      <RuntimeControls />
    </QueryClientProvider>,
  )
  await waitFor(() => expect(gateway.comfySubscribe).toHaveBeenCalled())
  emit({ kind: 'snapshot', cursor: 1, status: stopped, logs: [], gap: false })

  const start = await screen.findByRole('button', { name: /^Start$/ })
  await waitFor(() => expect(start.hasAttribute('disabled')).toBe(false))
  expect(start.getAttribute('aria-busy')).toBe('false')

  fireEvent.click(start)
  await waitFor(() => expect(start.getAttribute('aria-busy')).toBe('true'))
  expect(start.hasAttribute('disabled')).toBe(true)

  finishStart({ kind: 'started', status: { ...stopped, lifecycle: 'running' } })
  await waitFor(() => expect(start.getAttribute('aria-busy')).toBe('false'))
})
