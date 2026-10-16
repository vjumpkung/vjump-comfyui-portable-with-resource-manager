// @vitest-environment jsdom
import { expect, it, vi } from 'vitest'
vi.mock('@xterm/xterm', () => ({ Terminal: vi.fn() }))
import { plainLog } from './log-view'

it('removes terminal commands and controls without damaging Unicode log text', () => {
  expect(plainLog('\x1b[31mError\x1b[0m: โมเดล\r\n')).toBe('Error: โมเดล\n')
  expect(plainLog('\x1b]0;malicious title\x07package installed\x00')).toBe(
    'package installed',
  )
  expect(
    plainLog('\x1b]8;;https://untrusted.example\x1b\\click\x1b]8;;\x1b\\'),
  ).toBe('click')
})
