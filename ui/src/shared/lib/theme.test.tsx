// @vitest-environment jsdom
import { act, cleanup, render } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import { readCachedTheme, resolveTheme, ThemeProvider, useTheme } from './theme'

describe('resolveTheme', () => {
  it('follows the system preference only for the system setting', () => {
    expect(resolveTheme('system', true)).toBe('dark')
    expect(resolveTheme('system', false)).toBe('light')
    expect(resolveTheme('light', true)).toBe('light')
    expect(resolveTheme('dark', false)).toBe('dark')
  })
})

type Listener = (event: MediaQueryListEvent) => void

function installMatchMedia(initialDark: boolean) {
  let dark = initialDark
  const listeners = new Set<Listener>()
  window.matchMedia = ((query: string) => ({
    get matches() {
      return dark
    },
    media: query,
    addEventListener: (_type: string, listener: Listener) => listeners.add(listener),
    removeEventListener: (_type: string, listener: Listener) => listeners.delete(listener),
  })) as unknown as typeof window.matchMedia
  return (next: boolean) => {
    dark = next
    for (const listener of listeners) listener({ matches: next } as MediaQueryListEvent)
  }
}

function Probe({ onTheme }: { onTheme: (value: ReturnType<typeof useTheme>) => void }) {
  onTheme(useTheme())
  return null
}

describe('ThemeProvider', () => {
  beforeEach(() => {
    window.localStorage.clear()
    document.documentElement.classList.remove('dark')
  })

  afterEach(cleanup)

  it('tracks live system changes while the setting is system', () => {
    const setSystemDark = installMatchMedia(false)
    render(
      <ThemeProvider>
        <Probe onTheme={() => {}} />
      </ThemeProvider>,
    )
    expect(document.documentElement.classList.contains('dark')).toBe(false)

    act(() => setSystemDark(true))
    expect(document.documentElement.classList.contains('dark')).toBe(true)
  })

  it('applies an explicit setting immediately and caches it', () => {
    installMatchMedia(false)
    let current: ReturnType<typeof useTheme> | undefined
    render(
      <ThemeProvider>
        <Probe onTheme={(value) => (current = value)} />
      </ThemeProvider>,
    )

    act(() => current?.setSetting('dark'))
    expect(document.documentElement.classList.contains('dark')).toBe(true)
    expect(readCachedTheme()).toBe('dark')

    act(() => current?.setSetting('light'))
    expect(document.documentElement.classList.contains('dark')).toBe(false)
    expect(readCachedTheme()).toBe('light')
  })

  it('falls back to system for an unknown cached value', () => {
    window.localStorage.setItem('vjumpkung-comfyui.theme', 'neon')
    expect(readCachedTheme()).toBe('system')
  })
})
