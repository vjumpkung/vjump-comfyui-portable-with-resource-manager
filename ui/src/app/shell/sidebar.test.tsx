// @vitest-environment jsdom
import { act, cleanup, render, screen } from '@testing-library/react'
import { afterEach, expect, it, vi } from 'vitest'

import { setLanguage } from '@/shared/i18n'
import { StatusBadge } from '@/shared/ui/status-badge'

import { Sidebar } from './sidebar'

afterEach(() => {
  cleanup()
  setLanguage('en')
})

it('updates_mounted_navigation_and_status_when_language_changes', () => {
  render(<><Sidebar active="overview" onSelect={vi.fn()} /><StatusBadge status="running" /></>)
  const navigation = screen.getByRole('navigation', { name: 'Main' })
  const overview = screen.getByRole('button', { name: 'Overview' })
  expect(screen.getByRole('button', { name: 'Settings' })).toBeTruthy()
  expect(screen.getByText('running')).toBeTruthy()

  act(() => setLanguage('th'))
  expect(screen.getByRole('navigation', { name: 'เมนูหลัก' })).toBe(navigation)
  expect(screen.getByRole('button', { name: 'ภาพรวม' })).toBe(overview)
  expect(screen.getByRole('button', { name: 'การตั้งค่า' })).toBeTruthy()
  expect(screen.getByText('กำลังทำงาน')).toBeTruthy()
  expect(screen.queryByText('running')).toBeNull()

  act(() => setLanguage('en'))
  expect(screen.getByRole('navigation', { name: 'Main' })).toBe(navigation)
  expect(screen.getByRole('button', { name: 'Overview' })).toBe(overview)
  expect(screen.getByRole('button', { name: 'Settings' })).toBeTruthy()
  expect(screen.getByText('running')).toBeTruthy()
  expect(screen.queryByText('กำลังทำงาน')).toBeNull()
})
