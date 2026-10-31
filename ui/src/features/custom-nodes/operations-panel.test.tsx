// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, expect, it, vi } from 'vitest'

import { operation } from './fixtures'
import { OperationsPanel } from './operations-panel'

afterEach(cleanup)

const props = {
  unresolved: [
    operation({ id: 'op-blocked', phase: 'prepared', nodeName: 'Blocked Pack', blocksStart: true }),
    operation({ id: 'op-cleanup', phase: 'cleanup_pending', nodeName: 'Cleanup Pack' }),
  ],
  recent: [operation({ id: 'op-finished', nodeName: 'Finished Pack' })],
  logs: [],
  selectedId: null,
  onSelect: vi.fn(),
  rescanning: false,
  rescanError: null,
  onRescan: vi.fn(),
  onKeepFiles: vi.fn(),
  resolving: false,
  resolveError: null,
}

it('offers_keep_files_only_for_operations_that_block_start', () => {
  render(<OperationsPanel {...props} />)
  expect(screen.getAllByRole('button', { name: /Keep the files of/ })).toHaveLength(1)
  fireEvent.click(screen.getByRole('button', { name: 'Keep the files of Blocked Pack as they are' }))
  expect(props.onKeepFiles).toHaveBeenCalledExactlyOnceWith(props.unresolved[0])
})

it.each(['rescanning', 'resolving'] as const)('disables_recovery_buttons_while_%s', (busy) => {
  render(<OperationsPanel {...props} {...{ [busy]: true }} />)
  expect(screen.getByRole<HTMLButtonElement>('button', { name: /Keep the files of/ }).disabled).toBe(true)
  expect(screen.getByRole<HTMLButtonElement>('button', { name: 'Rescan / finish recovery' }).disabled).toBe(true)
})
