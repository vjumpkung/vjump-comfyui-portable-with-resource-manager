import { useMemo } from 'react'
import { create } from 'zustand'

import translations from './translations.json'

export type Language = 'en' | 'th'

export const LANGUAGES: readonly { value: Language; label: string }[] = [
  { value: 'en', label: 'English' },
  { value: 'th', label: 'ไทย' },
]

type Values = Readonly<Record<string, string | number>>
type CatalogKey = keyof typeof translations.en
type TextKey = Exclude<CatalogKey, `${string}_one` | `${string}_other`>
type PluralKey = {
  [Key in CatalogKey]: Key extends `${infer Base}_one` | `${infer Base}_other` ? Base : never
}[CatalogKey]

// First-paint cache only. The settings stored by the backend are authoritative and
// reconcile this value after `settings_get` resolves.
const CACHE_KEY = 'vjumpkung-comfyui.language'
const useLanguageStore = create<{ language: Language }>()(() => ({ language: 'en' }))

function lookup(locale: Language, key: TextKey | PluralKey, values?: Values): string | undefined {
  const catalog: Readonly<Partial<Record<string, string>>> = translations[locale]
  if (typeof values?.count === 'number') {
    const category = new Intl.PluralRules(locale).select(values.count)
    const suffix = category === 'one' ? 'one' : 'other'
    return catalog[`${key}_${suffix}`] ?? catalog[`${key}_other`] ?? catalog[key]
  }
  return catalog[key]
}

function translate(locale: Language, key: TextKey | PluralKey, values?: Values): string {
  const message = lookup(locale, key, values) ?? lookup('en', key, values) ?? key
  return message.replace(/\{([^{}]+)\}/g, (placeholder: string, name: string) => {
    const value = values?.[name]
    return value === undefined ? placeholder : String(value)
  })
}

export function t(key: TextKey, values?: Values): string
export function t(key: PluralKey, values: Values & { count: number }): string
export function t(key: TextKey | PluralKey, values?: Values): string {
  return translate(getLanguage(), key, values)
}

export function getLanguage(): Language {
  return useLanguageStore.getState().language
}

export function isLanguage(value: unknown): value is Language {
  return value === 'en' || value === 'th'
}

/** Applies the cached language before React renders to limit first-paint flashing. */
export function initLanguage(): void {
  let cached: Language = 'en'
  try {
    const value = window.localStorage.getItem(CACHE_KEY)
    if (isLanguage(value)) cached = value
  } catch {
    // Storage can be unavailable; the cache is only a first-paint convenience.
  }
  setLanguage(cached)
}

export function setLanguage(next: Language): void {
  useLanguageStore.setState({ language: next })
  try {
    window.localStorage.setItem(CACHE_KEY, next)
  } catch {
    // Storage can be unavailable; the cache is only a first-paint convenience.
  }
  if (typeof document !== 'undefined') document.documentElement.lang = next
}

export function useTranslation(): { language: Language; t: typeof t } {
  const current = useLanguageStore((state) => state.language)
  const translator = useMemo<typeof t>(
    () => (key: TextKey | PluralKey, values?: Values) => translate(current, key, values),
    [current],
  )
  return { language: current, t: translator }
}
