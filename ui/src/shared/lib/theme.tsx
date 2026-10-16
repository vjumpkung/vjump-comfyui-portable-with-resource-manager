import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react'

export type ThemeSetting = 'system' | 'light' | 'dark'
export type ResolvedTheme = 'light' | 'dark'

// First-paint cache only. The settings stored by the backend are authoritative and
// reconcile this value after `settings_get` resolves.
const CACHE_KEY = 'vjumpkung-comfyui.theme'
const DARK_QUERY = '(prefers-color-scheme: dark)'

export function resolveTheme(setting: ThemeSetting, prefersDark: boolean): ResolvedTheme {
  if (setting === 'system') return prefersDark ? 'dark' : 'light'
  return setting
}

function isThemeSetting(value: unknown): value is ThemeSetting {
  return value === 'system' || value === 'light' || value === 'dark'
}

export function readCachedTheme(): ThemeSetting {
  try {
    const value = window.localStorage.getItem(CACHE_KEY)
    return isThemeSetting(value) ? value : 'system'
  } catch {
    return 'system'
  }
}

function cacheTheme(setting: ThemeSetting) {
  try {
    window.localStorage.setItem(CACHE_KEY, setting)
  } catch {
    // Storage can be unavailable; the cache is only a first-paint convenience.
  }
}

function prefersDark(): boolean {
  return typeof window.matchMedia === 'function' && window.matchMedia(DARK_QUERY).matches
}

export function applyTheme(resolved: ResolvedTheme) {
  document.documentElement.classList.toggle('dark', resolved === 'dark')
}

/** Applies the cached theme before React renders to limit first-paint flashing. */
export function initTheme() {
  applyTheme(resolveTheme(readCachedTheme(), prefersDark()))
}

interface ThemeContextValue {
  setting: ThemeSetting
  resolved: ResolvedTheme
  setSetting: (setting: ThemeSetting) => void
}

const ThemeContext = createContext<ThemeContextValue | null>(null)

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [setting, setSettingState] = useState<ThemeSetting>(readCachedTheme)
  const [systemDark, setSystemDark] = useState<boolean>(prefersDark)

  useEffect(() => {
    if (typeof window.matchMedia !== 'function') return
    const query = window.matchMedia(DARK_QUERY)
    const onChange = (event: MediaQueryListEvent) => setSystemDark(event.matches)
    query.addEventListener('change', onChange)
    return () => query.removeEventListener('change', onChange)
  }, [])

  const resolved = resolveTheme(setting, systemDark)

  useEffect(() => {
    applyTheme(resolved)
  }, [resolved])

  const setSetting = useCallback((next: ThemeSetting) => {
    cacheTheme(next)
    setSettingState(next)
  }, [])

  const value = useMemo(
    () => ({ setting, resolved, setSetting }),
    [setting, resolved, setSetting],
  )

  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>
}

export function useTheme(): ThemeContextValue {
  const value = useContext(ThemeContext)
  if (!value) throw new Error('useTheme must be used inside ThemeProvider')
  return value
}
