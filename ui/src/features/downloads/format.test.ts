import { expect, it } from 'vitest'

import { guessProvider } from './categories'
import {
  formatBytes,
  formatEta,
  percent,
  progressText,
} from './format'
import { job } from './fixtures'

it('formats bytes, ETA and percent', () => {
  expect(formatBytes(0)).toBe('0 B')
  expect(formatBytes(1536)).toBe('1.5 KiB')
  expect(formatBytes(5 * 1024 ** 3)).toBe('5.0 GiB')
  expect(formatEta(null)).toBe('ETA unknown')
  expect(formatEta(75)).toBe('1m 15s left')
  expect(formatEta(3700)).toBe('1h 1m left')
  expect(percent(250, { kind: 'exact', bytes: 1000 })).toBe(25)
  expect(percent(10, { kind: 'unknown', bytes: null })).toBeNull()
})

it('marks hf CLI progress as estimated and shows speed and ETA only while downloading', () => {
  const running = job({ state: 'downloading', estimated: true })
  const text = progressText(running, {
    id: running.id,
    generation: 1,
    bytes: 500,
    total: { kind: 'exact', bytes: 1000 },
    bytesPerSecond: 100,
    etaSeconds: 5,
    estimated: true,
  })
  expect(text).toContain('500 B / 1000 B')
  expect(text).toContain('100 B/s')
  expect(text).toContain('5s left')
  expect(text).toContain('estimated')
  expect(progressText(job({ state: 'paused', bytesDone: 500 }), undefined)).toBe(
    '500 B / 1000 B',
  )
})

it('guesses the provider for the chip', () => {
  expect(guessProvider('https://civitai.com/models/1')).toBe('civitai')
  expect(guessProvider('https://huggingface.co/a/b')).toBe('huggingface')
  expect(guessProvider('https://drive.google.com/file/d/x/view')).toBe(
    'google_drive',
  )
  expect(guessProvider('https://example.com/a.bin')).toBe('direct')
  expect(guessProvider('not a url')).toBeNull()
})
