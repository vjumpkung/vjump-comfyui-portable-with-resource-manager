import { RuntimeControls } from '@/features/comfy-runtime'

import { ThemeToggle } from './theme-toggle'

export function TopBar() {
  return (
    <header className="flex h-14 shrink-0 items-center justify-between border-b bg-background/80 px-6 backdrop-blur-md">
      <RuntimeControls />
      <ThemeToggle />
    </header>
  )
}
