// @vitest-environment jsdom
import { cleanup, configure, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'

import type { PlanReviewDto } from '@/shared/ipc/gateway'

import { plan } from './fixtures'
import { ReviewDialog } from './review-dialog'

beforeEach(() => {
  vi.clearAllMocks()
  vi.stubGlobal(
    'ResizeObserver',
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  )
})
afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

// Rendering many rows in jsdom is slow when the whole suite runs in parallel.
vi.setConfig({ testTimeout: 30_000 })
configure({ asyncUtilTimeout: 5000 })

const onCommit = vi.fn()
const onClose = vi.fn()
const onCancel = vi.fn()

function ui(plans: PlanReviewDto[], busy = false, cancellable = true) {
  return (
    <ReviewDialog
      plans={plans}
      busy={busy}
      onCommit={onCommit}
      onClose={onClose}
      onCancel={cancellable ? onCancel : null}
    />
  )
}

const box = (name: RegExp) => screen.getByRole('checkbox', { name })
const checked = (name: RegExp) => box(name).getAttribute('aria-checked') === 'true'
const apply = () => screen.getByRole('button', { name: /Apply reviewed plans|Applying/ })

it('binding_change_resets_all_consent', () => {
  const first = plan({ hasInstallPy: true, binding: 'b1' })
  const view = render(ui([first]))
  fireEvent.click(box(/execute third-party code/))
  fireEvent.click(box(/install\.py script/))
  expect(checked(/execute third-party code/)).toBe(true)
  expect(checked(/install\.py script/)).toBe(true)
  view.rerender(ui([{ ...first, binding: 'b2' }]))
  expect(checked(/execute third-party code/)).toBe(false)
  expect(checked(/install\.py script/)).toBe(false)
  expect((apply() as HTMLButtonElement).disabled).toBe(true)
})

it('returning_to_an_old_binding_does_not_restore_consent', () => {
  const first = plan({ binding: 'b1' })
  const view = render(ui([first]))
  fireEvent.click(box(/execute third-party code/))
  expect(checked(/execute third-party code/)).toBe(true)
  view.rerender(ui([{ ...first, binding: 'b2' }]))
  view.rerender(ui([first]))
  expect(checked(/execute third-party code/)).toBe(false)
})

it('remembered_hook_starts_unchecked', () => {
  render(ui([plan({ trustedSource: true, trustedInstallPy: true, hasInstallPy: true })]))
  expect(checked(/execute third-party code/)).toBe(false)
  expect(checked(/install\.py script/)).toBe(false)
  expect(screen.getByText(/starts with script execution unchecked/)).toBeTruthy()
  expect(screen.queryByRole('checkbox', { name: /remember permission to run install\.py/ })).toBeNull()
  expect((apply() as HTMLButtonElement).disabled).toBe(true)
})

it('approved_hook_discloses_install_py_before_requirements', () => {
  render(ui([plan({ hasInstallPy: true })]))
  const hook = screen.getByRole('checkbox', {
    name: 'Run the root install.py script with the selected Python environment before requirements.txt',
  })
  for (const checkbox of screen.getAllByRole('checkbox'))
    expect(checkbox.getAttribute('aria-checked')).toBe('false')
  expect((apply() as HTMLButtonElement).disabled).toBe(true)
  fireEvent.click(box(/execute third-party code/))
  fireEvent.click(hook)
  fireEvent.click(apply())
  expect(onCommit).toHaveBeenCalledWith([expect.objectContaining({
    executeNodeCode: true,
    runInstallPy: true,
    rememberSource: false,
    rememberInstallPy: false,
  })])
})

it('warnings_and_pin_changes_require_explicit_confirmation', () => {
  const changes = [{ name: 'torch', from: '2.14.1', to: '2.15.0' }]
  render(ui([plan({ id: 'p1', binding: 'b9', warnings: ['Flagged by the registry'], protectedChanges: changes })]))
  expect(screen.getByText('Flagged by the registry')).toBeTruthy()
  expect(screen.getByText(/torch: 2\.14\.1/)).toBeTruthy()
  fireEvent.click(box(/execute third-party code/))
  expect((apply() as HTMLButtonElement).disabled).toBe(true)
  fireEvent.click(box(/acknowledge all displayed warnings/))
  expect((apply() as HTMLButtonElement).disabled).toBe(true)
  fireEvent.click(box(/protected-package changes/))
  expect((apply() as HTMLButtonElement).disabled).toBe(false)
  fireEvent.click(apply())
  expect(onCommit).toHaveBeenCalledWith([
    expect.objectContaining({
      planId: 'p1',
      binding: 'b9',
      executeNodeCode: true,
      acknowledgeWarnings: true,
      confirmedChanges: changes,
    }),
  ])
})

it('busy_review_cannot_be_discarded', () => {
  render(ui([plan()], true))
  expect(screen.getByRole<HTMLButtonElement>('button', { name: 'Discard plans' }).disabled).toBe(true)
  fireEvent.keyDown(document, { key: 'Escape' })
  expect(onClose).not.toHaveBeenCalled()
  expect(screen.queryByRole('button', { name: 'Close' })).toBeNull()
  fireEvent.click(screen.getByRole('button', { name: 'Request cancellation' }))
  expect(onCancel).toHaveBeenCalledTimes(1)
})

it('offers no cancellation request without a valid key and discards an idle review', () => {
  render(ui([plan()], true, false))
  expect(screen.queryByRole('button', { name: 'Request cancellation' })).toBeNull()
  cleanup()
  render(ui([plan()]))
  fireEvent.click(screen.getByRole('button', { name: 'Discard plans' }))
  expect(onClose).toHaveBeenCalledTimes(1)
})

it('shows the exact source, tree identity and the requirements scope', () => {
  render(ui([plan({
    kind: 'update',
    priorSource: { kind: 'manual' },
    hookDigest: 'd'.repeat(64),
    hasInstallPy: true,
  })]))
  expect(screen.getByText('Tree identity')).toBeTruthy()
  expect(screen.queryByText(/Tree SHA-256/)).toBeNull()
  expect(screen.getByText('example-pack@1.2.3')).toBeTruthy()
  expect(screen.getByText('Manual / unmanaged')).toBeTruthy()
  expect(screen.getByText('Registry dependencies and requirements.txt')).toBeTruthy()
  expect(screen.getByText('numpy>=1.26')).toBeTruthy()
})

it('offers trust only when the source has a namespace', () => {
  render(ui([plan({
    source: { kind: 'registry', id: 'p', publisher: null, version: '1', repository: null },
  })]))
  expect(screen.queryByRole('checkbox', { name: /Remember this source as trusted/ })).toBeNull()
  cleanup()
  render(ui([plan({ hasInstallPy: true })]))
  fireEvent.click(box(/install\.py script/))
  fireEvent.click(box(/Remember this source as trusted/))
  expect(box(/remember permission to run install\.py/)).toBeTruthy()
  fireEvent.click(box(/install\.py script/))
  expect(screen.queryByRole('checkbox', { name: /remember permission to run install\.py/ })).toBeNull()
})

it('setup_review_hides_trust_memory_and_keeps_decisions_unremembered', () => {
  render(<ReviewDialog plans={[plan({ warnings: ['Pinned checkout is detached'] })]}
    busy={false} onCommit={onCommit} onClose={onClose} onCancel={null}
    title="Review curated setup nodes" commitLabel="Approve and retry setup"
    closeLabel="Back to setup" allowRememberSource={false} />)
  expect(screen.getByRole('heading', { name: 'Review curated setup nodes' })).toBeTruthy()
  expect(screen.queryByRole('checkbox', { name: /Remember/ })).toBeNull()
  expect(checked(/execute third-party code/)).toBe(false)
  expect(checked(/acknowledge all displayed warnings/)).toBe(false)
  fireEvent.click(box(/execute third-party code/))
  const approve = screen.getByRole<HTMLButtonElement>('button', { name: 'Approve and retry setup' })
  expect(approve.disabled).toBe(true)
  fireEvent.click(box(/acknowledge all displayed warnings/))
  fireEvent.click(approve)
  expect(onCommit).toHaveBeenCalledWith([expect.objectContaining({
    executeNodeCode: true, acknowledgeWarnings: true,
    rememberSource: false, rememberInstallPy: false, runInstallPy: false,
  })])
  fireEvent.click(screen.getByRole('button', { name: 'Back to setup' }))
  expect(onClose).toHaveBeenCalledTimes(1)
})
