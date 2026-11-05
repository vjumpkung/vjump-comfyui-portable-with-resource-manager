// @vitest-environment jsdom
import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, expect, it, vi } from 'vitest'

import { file } from './fixtures'
import { PreviewDialog } from './preview-dialog'

vi.mock('@/shared/ipc/gateway', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/ipc/gateway')>()),
  gateway: {
    galleryPreviewUrl: (location: { area: string; path: string }, revision: string) =>
      `http://gallery.localhost/${location.area}/${location.path}?v=${revision}`,
  },
}))

afterEach(cleanup)

it('preview_title_truncates_without_clipping_tall_glyphs', () => {
  const entry = file('cat.png')
  render(
    <PreviewDialog
      entry={entry}
      files={[entry]}
      onNavigate={() => undefined}
      onClose={() => undefined}
    />,
  )
  const title = screen.getByRole('heading', { name: 'cat.png' })
  // `truncate` clips overflow, so the dialog's `leading-none` would cut Thai tone marks and descenders.
  expect(title.classList.contains('truncate')).toBe(true)
  expect(title.classList.contains('leading-normal')).toBe(true)
  expect(title.classList.contains('leading-none')).toBe(false)
})
