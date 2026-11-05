import { afterEach, expect, it } from 'vitest'

import { setLanguage } from '@/shared/i18n'
import { IpcClientError } from '@/shared/ipc/gateway'

import { errorMessage } from './display'

afterEach(() => setLanguage('en'))

it('translates repository file protection failures for both areas', () => {
  for (const code of ['model_repository_file', 'gallery_repository_file']) {
    const error = new IpcClientError(code, 'backend message')
    expect(errorMessage(error)).toBe(
      "This file is part of the ComfyUI installation and can't be changed here.",
    )
    setLanguage('th')
    expect(errorMessage(error)).toBe(
      'ไฟล์นี้เป็นส่วนหนึ่งของการติดตั้ง ComfyUI และไม่สามารถแก้ไขได้ที่นี่',
    )
    setLanguage('en')
  }
})
