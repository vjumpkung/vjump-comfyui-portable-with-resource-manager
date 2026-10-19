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

import { gateway, IpcClientError } from '@/shared/ipc/gateway'

import { DiagnosticsPanel } from './diagnostics-panel'

vi.mock('@/shared/ipc/gateway', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/ipc/gateway')>()),
  gateway: { appCopyDiagnostics: vi.fn() },
}))

beforeEach(() => vi.resetAllMocks())
afterEach(cleanup)

function copy() {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <DiagnosticsPanel />
    </QueryClientProvider>,
  )
  fireEvent.click(screen.getByRole('button', { name: /copy diagnostics/i }))
}

it('diagnostics_copy_failure_exposes_read_only_fallback', async () => {
  vi.mocked(gateway.appCopyDiagnostics).mockResolvedValue({
    text: 'REDACTED REPORT',
    copied: false,
  })
  copy()

  const report = await screen.findByLabelText<HTMLTextAreaElement>(
    'Diagnostics report',
  )
  expect(report.readOnly).toBe(true)
  expect(report.value).toBe('REDACTED REPORT')
  expect(gateway.appCopyDiagnostics).toHaveBeenCalledWith()
})

it('a_copied_report_shows_no_fallback', async () => {
  vi.mocked(gateway.appCopyDiagnostics).mockResolvedValue({
    text: 'REDACTED REPORT',
    copied: true,
  })
  copy()

  await waitFor(() => expect(gateway.appCopyDiagnostics).toHaveBeenCalled())
  await waitFor(() =>
    expect(
      screen.getByRole<HTMLButtonElement>('button', { name: /copy diagnostics/i })
        .disabled,
    ).toBe(false),
  )
  expect(screen.queryByLabelText('Diagnostics report')).toBeNull()
})

it('a_failed_collection_shows_no_fallback', async () => {
  vi.mocked(gateway.appCopyDiagnostics).mockRejectedValue(
    new IpcClientError('transport', 'Could not reach the application backend.'),
  )
  copy()

  await waitFor(() => expect(gateway.appCopyDiagnostics).toHaveBeenCalled())
  expect(screen.queryByLabelText('Diagnostics report')).toBeNull()
})
