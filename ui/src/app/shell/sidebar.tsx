import { useTranslation } from '@/shared/i18n'
import { cn } from '@/shared/lib/utils'

import { NAV_ITEMS, type NavId } from './nav'

interface SidebarProps {
  active: NavId
  onSelect: (id: NavId) => void
}

export function Sidebar({ active, onSelect }: SidebarProps) {
  const { t } = useTranslation()
  return (
    <aside className="flex w-16 shrink-0 flex-col border-r bg-card md:w-56">
      <div className="flex h-14 items-center border-b px-4">
        <span className="font-mono text-xs font-semibold uppercase tracking-wider text-muted-foreground">
          <span className="md:hidden">VC</span>
          <span className="hidden md:inline">vjumpkung ComfyUI</span>
        </span>
      </div>
      <nav aria-label={t('shell.main')} className="flex flex-col gap-1 p-2">
        {NAV_ITEMS.map((item) => {
          const isActive = item.id === active
          return (
            <button
              key={item.id}
              type="button"
              aria-current={isActive ? 'page' : undefined}
              aria-label={t(item.labelKey)}
              onClick={() => onSelect(item.id)}
              className={cn(
                'flex items-center gap-3 rounded-md border-l-2 px-3 py-2 text-sm outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring',
                isActive
                  ? 'border-transparent bg-brand-yellow font-semibold text-[#2a313a]'
                  : 'border-transparent font-medium text-muted-foreground hover:border-brand-pink hover:bg-muted hover:text-foreground',
              )}
            >
              <item.icon className="size-4 shrink-0" aria-hidden="true" />
              <span className="hidden md:inline">{t(item.labelKey)}</span>
            </button>
          )
        })}
      </nav>
    </aside>
  )
}
