import { expect, it } from 'vitest'

import { many, model } from './fixtures'
import {
  categoriesOf,
  DEFAULT_FILTERS,
  filterModels,
  formatBytes,
  keyOf,
  sourceLabel,
} from './model-list'

it('filters by search text, category and root, case-insensitively', () => {
  const models = [
    model({ name: 'Dreamshaper.safetensors' }),
    model({ name: 'detail.safetensors', category: 'loras', subfolder: 'style' }),
    model({
      name: 'other.pt',
      location: { rootId: 'extra-1', path: 'other.pt' },
      category: 'vae',
    }),
  ]
  const names = (filters: Partial<typeof DEFAULT_FILTERS>) =>
    filterModels(models, { ...DEFAULT_FILTERS, ...filters }).map((m) => m.name)
  expect(names({ search: 'DREAM' })).toEqual(['Dreamshaper.safetensors'])
  expect(names({ search: 'style' })).toEqual(['detail.safetensors'])
  expect(names({ category: 'loras' })).toEqual(['detail.safetensors'])
  expect(names({ root: 'extra-1' })).toEqual(['other.pt'])
  expect(names({ search: 'nothing' })).toEqual([])
})

it('sorts by name naturally, size and modified time in both directions', () => {
  const models = [
    model({ name: 'm10.pt', size: 5, modifiedMs: 30 }),
    model({ name: 'm2.pt', size: 9, modifiedMs: 10 }),
    model({ name: 'm1.pt', size: 7, modifiedMs: 20 }),
  ]
  const order = (sort: 'name' | 'size' | 'modified', descending = false) =>
    filterModels(models, { ...DEFAULT_FILTERS, sort, descending }).map(
      (m) => m.name,
    )
  expect(order('name')).toEqual(['m1.pt', 'm2.pt', 'm10.pt'])
  expect(order('size')).toEqual(['m10.pt', 'm1.pt', 'm2.pt'])
  expect(order('modified', true)).toEqual(['m10.pt', 'm1.pt', 'm2.pt'])
})

it('lists categories, formats sizes and names the source', () => {
  expect(categoriesOf(many(4))).toEqual(['checkpoints', 'loras'])
  expect(formatBytes(1536)).toBe('1.5 KiB')
  expect(formatBytes(null)).toBe('0 B')
  expect(sourceLabel(model({ civitaiVersionId: 9 }))).toBe('CivitAI v9')
  expect(sourceLabel(model({ sourceJobId: 'j' }))).toBe('Downloaded')
  expect(sourceLabel(model())).toBe('Local file')
  expect(keyOf(model().location)).toBe('models:checkpoints/model.safetensors')
})
