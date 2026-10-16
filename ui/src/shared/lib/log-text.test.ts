import { expect, it } from 'vitest'

import { plainStream } from './log-text'

it('keeps progress carriage returns and Unicode but removes terminal commands', () => {
  expect(plainStream('50%\r100%\nไทย\t\x1b[31mred\x1b[0m')).toBe(
    '50%\r100%\nไทย\tred',
  )
  expect(plainStream('\x1b]0;title\x07ok\x00\x08')).toBe('ok')
  expect(plainStream('\x1b]8;;https://untrusted.example\x1b\\x\x1b]8;;\x1b\\')).toBe(
    'x',
  )
})
