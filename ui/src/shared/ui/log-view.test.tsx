// @vitest-environment jsdom
import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'

const { open, observe } = vi.hoisted(() => ({
  open: vi.fn(),
  observe: vi.fn(),
}))
vi.mock('@xterm/xterm', () => ({
  Terminal: class {
    open = open
    loadAddon = vi.fn()
    clear = vi.fn()
    write = vi.fn()
    dispose = vi.fn()
  },
}))
vi.mock('@xterm/addon-fit', () => ({
  FitAddon: class {
    fit = vi.fn()
  },
}))
import { LogView, plainLog, StreamLogView } from './log-view'

beforeEach(() => {
  vi.clearAllMocks()
  vi.stubGlobal('ResizeObserver', class {
    observe = observe
    disconnect = vi.fn()
  })
})

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

function expectTerminalFrame(label: string, height: string) {
  const host = screen.getByLabelText(label)
  expect(open).toHaveBeenCalledWith(host)
  expect(host.className).toBe('h-full min-w-0')
  const frame = host.parentElement!
  for (const name of ['border', 'border-secondary', 'p-3', 'overflow-hidden', 'setup-log', height]) {
    expect(frame.classList.contains(name)).toBe(true)
  }
  expect(observe).toHaveBeenCalledWith(host)
}

it('log_view_terminal_host_has_no_frame_padding', () => {
  render(<LogView lines={['ready']} />)
  expectTerminalFrame('Sanitized setup log', 'h-72')
})

it('stream_log_view_terminal_host_has_no_frame_padding', () => {
  render(<StreamLogView text="ready" label="ComfyUI log" />)
  expectTerminalFrame('ComfyUI log', 'h-80')
})

it('removes terminal commands and controls without damaging Unicode log text', () => {
  expect(plainLog('\x1b[31mError\x1b[0m: โมเดล\r\n')).toBe('Error: โมเดล\n')
  expect(plainLog('\x1b]0;malicious title\x07package installed\x00')).toBe(
    'package installed',
  )
  expect(
    plainLog('\x1b]8;;https://untrusted.example\x1b\\click\x1b]8;;\x1b\\'),
  ).toBe('click')
})
