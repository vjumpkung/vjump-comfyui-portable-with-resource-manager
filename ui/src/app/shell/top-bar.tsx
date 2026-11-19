import { RuntimeControls } from '@/features/comfy-runtime/runtime-controls'

import { ThemeToggle } from './theme-toggle'

export function TopBar() {
  return (
    <header className="flex h-14 shrink-0 items-center justify-between border-b bg-background px-6">
      <RuntimeControls />
      <ThemeToggle />
    </header>
  )
}
