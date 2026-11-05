/// <reference types="node" />

import { existsSync, readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { expect, test } from 'vitest'

test('the static splash uses an external stylesheet', () => {
  const html = readFileSync(fileURLToPath(new URL('../../index.html', import.meta.url)), 'utf8')
  // Tauri adds a nonce to inline style elements, disabling CSP unsafe-inline for
  // runtime styles injected by Sonner and Radix scroll lock in release builds.
  expect(html).not.toMatch(/<style\b/i)
  expect(html).not.toMatch(/\sstyle\s*=/i)
  expect(html).toContain('<link rel="stylesheet" href="/splash.css" />')
  expect(existsSync(fileURLToPath(new URL('../../public/splash.css', import.meta.url)))).toBe(true)
})
