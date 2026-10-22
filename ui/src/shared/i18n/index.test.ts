// @vitest-environment jsdom
import { act, cleanup, renderHook } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { getLanguage, initLanguage, isLanguage, LANGUAGES, setLanguage, t, useTranslation } from './index'
import translations from './translations.json'

const originalTranslations = {
  en: { ...translations.en },
  th: { ...translations.th },
}

beforeEach(() => {
  window.localStorage.clear()
})

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  Object.assign(translations.en, originalTranslations.en)
  Object.assign(translations.th, originalTranslations.th)
  setLanguage('en')
})

describe('translations', () => {
  it('defines identical English and Thai key sets with both plural forms', () => {
    const keys = Object.keys(translations.en).sort()
    expect(Object.keys(translations.th).sort()).toEqual(keys)
    for (const key of keys) {
      if (key.endsWith('_one')) expect(keys).toContain(key.replace(/_one$/, '_other'))
      if (key.endsWith('_other')) expect(keys).toContain(key.replace(/_other$/, '_one'))
    }
  })

  it('contains no empty values', () => {
    for (const catalog of Object.values(translations)) {
      for (const message of Object.values(catalog)) expect(message.trim()).not.toBe('')
    }
  })

  it('uses matching placeholder sets for each key', () => {
    const placeholders = (message: string) =>
      [...new Set(message.match(/\{[^{}]+\}/g) ?? [])].sort()
    for (const key of Object.keys(translations.en) as (keyof typeof translations.en)[]) {
      expect(placeholders(translations.th[key])).toEqual(placeholders(translations.en[key]))
    }
  })
})

describe('t', () => {
  it('translates using the current language', () => {
    expect(t('common.language')).toBe('Language')
    setLanguage('th')
    expect(t('common.language')).toBe('ภาษา')
  })

  it('interpolates named string and number values and preserves missing placeholders', () => {
    translations.en['common.language'] = '{name}: {count}; {missing}; {name}'
    expect(t('common.language', { name: 'ComfyUI', count: 0 })).toBe('ComfyUI: 0; {missing}; ComfyUI')
    expect(t('common.language')).toBe('{name}: {count}; {missing}; {name}')
  })

  it('selects English one and other plural forms', () => {
    expect(t('common.itemCount', { count: 1 })).toBe('1 item')
    expect(t('common.itemCount', { count: 0 })).toBe('0 items')
    expect(t('common.itemCount', { count: 2 })).toBe('2 items')
    expect(t('common.itemCount', { count: 1.5 })).toBe('1.5 items')
  })

  it('selects the Thai other plural form for all counts', () => {
    setLanguage('th')
    translations.th['common.itemCount_one'] = 'unexpected one'
    expect(t('common.itemCount', { count: 0 })).toBe('0 รายการ')
    expect(t('common.itemCount', { count: 1 })).toBe('1 รายการ')
    expect(t('common.itemCount', { count: 2 })).toBe('2 รายการ')
  })

  it('falls back to the other plural form when the selected form is missing', () => {
    Reflect.deleteProperty(translations.en, 'common.itemCount_one')
    expect(t('common.itemCount', { count: 1 })).toBe('1 items')
  })

  it('falls back to English when a translation is missing', () => {
    setLanguage('th')
    Reflect.deleteProperty(translations.th, 'common.language')
    expect(t('common.language')).toBe('Language')
  })

  it('uses English plural rules when falling back to English', () => {
    setLanguage('th')
    Reflect.deleteProperty(translations.th, 'common.itemCount_one')
    Reflect.deleteProperty(translations.th, 'common.itemCount_other')
    expect(t('common.itemCount', { count: 1 })).toBe('1 item')
    expect(t('common.itemCount', { count: 2 })).toBe('2 items')
  })

  it('falls back to the key when both translations are missing', () => {
    setLanguage('th')
    Reflect.deleteProperty(translations.th, 'common.language')
    Reflect.deleteProperty(translations.en, 'common.language')
    expect(t('common.language')).toBe('common.language')
  })

  it('rejects unknown keys and plural calls without a numeric count at compile time', () => {
    // @ts-expect-error Unknown translation keys are rejected.
    expect(t('common.unknown')).toBe('common.unknown')
    // @ts-expect-error Plural base keys require a values argument with count.
    expect(t('common.itemCount')).toBe('common.itemCount')
    // @ts-expect-error Plural base keys require count even when other values are present.
    expect(t('common.itemCount', { name: 'ComfyUI' })).toBe('common.itemCount')
    // @ts-expect-error Plural count must be a number.
    expect(t('common.itemCount', { count: '1' })).toBe('common.itemCount')
    // @ts-expect-error Plural suffixes are not public translation keys.
    expect(t('common.itemCount_one')).toBe('{count} item')
  })
})

describe('language preferences', () => {
  it('lists supported languages using endonyms', () => {
    expect(LANGUAGES).toEqual([
      { value: 'en', label: 'English' },
      { value: 'th', label: 'ไทย' },
    ])
    expect(isLanguage('en')).toBe(true)
    expect(isLanguage('th')).toBe(true)
    for (const value of ['EN', 'fr', '', null, undefined, 1, {}]) {
      expect(isLanguage(value)).toBe(false)
    }
  })

  it('initializes from a valid cached language and updates html lang', () => {
    window.localStorage.setItem('vjumpkung-comfyui.language', 'th')
    initLanguage()
    expect(getLanguage()).toBe('th')
    expect(document.documentElement.lang).toBe('th')
  })

  it.each([null, 'invalid'])('defaults to English for a missing or invalid cache (%s)', (cached) => {
    setLanguage('th')
    if (cached === null) window.localStorage.removeItem('vjumpkung-comfyui.language')
    else window.localStorage.setItem('vjumpkung-comfyui.language', cached)
    initLanguage()
    expect(getLanguage()).toBe('en')
    expect(document.documentElement.lang).toBe('en')
  })

  it('defaults to English when storage access is unavailable', () => {
    setLanguage('th')
    vi.spyOn(window, 'localStorage', 'get').mockImplementation(() => {
      throw new Error('Storage unavailable')
    })
    expect(() => initLanguage()).not.toThrow()
    expect(getLanguage()).toBe('en')
    expect(document.documentElement.lang).toBe('en')
  })

  it('sets the language, html lang, and first-paint cache', () => {
    setLanguage('th')
    expect(getLanguage()).toBe('th')
    expect(document.documentElement.lang).toBe('th')
    expect(window.localStorage.getItem('vjumpkung-comfyui.language')).toBe('th')
    setLanguage('en')
    expect(document.documentElement.lang).toBe('en')
    expect(window.localStorage.getItem('vjumpkung-comfyui.language')).toBe('en')
  })

  it('still applies the language when caching fails', () => {
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('Storage unavailable')
    })
    expect(() => setLanguage('th')).not.toThrow()
    expect(getLanguage()).toBe('th')
    expect(document.documentElement.lang).toBe('th')
  })

  it('imports safely and defaults to English without browser globals', async () => {
    vi.resetModules()
    vi.stubGlobal('window', undefined)
    vi.stubGlobal('document', undefined)
    vi.stubGlobal('localStorage', undefined)
    const i18n = await import('./index')
    expect(i18n.getLanguage()).toBe('en')
    expect(i18n.t('common.language')).toBe('Language')
    expect(() => i18n.initLanguage()).not.toThrow()
    expect(() => i18n.setLanguage('th')).not.toThrow()
    expect(i18n.t('common.language')).toBe('ภาษา')
    i18n.setLanguage('en')
  })
})

describe('useTranslation', () => {
  it('re-renders on language changes and updates the translation function identity', () => {
    const { result, rerender, unmount } = renderHook(() => useTranslation())
    const englishT = result.current.t
    expect(result.current.language).toBe('en')
    expect(englishT('common.language')).toBe('Language')
    rerender()
    expect(result.current.t).toBe(englishT)

    act(() => setLanguage('th'))
    const thaiT = result.current.t
    expect(result.current.language).toBe('th')
    expect(thaiT('common.language')).toBe('ภาษา')
    expect(thaiT).not.toBe(englishT)

    act(() => setLanguage('th'))
    expect(result.current.t).toBe(thaiT)
    act(() => setLanguage('en'))
    expect(result.current.language).toBe('en')
    expect(result.current.t('common.language')).toBe('Language')
    expect(result.current.t).not.toBe(thaiT)
    unmount()
    expect(() => setLanguage('th')).not.toThrow()
  })
})
