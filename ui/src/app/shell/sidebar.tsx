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
        <span className="text-sm font-bold text-foreground">
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
                'flex h-10 items-center justify-center gap-3 rounded-full border px-3 text-sm transition-colors outline-none md:justify-start',
                isActive
                  ? 'border-fill-edge bg-primary font-semibold text-primary-foreground'
                  : 'border-transparent font-medium text-muted-foreground hover:bg-muted hover:text-foreground',
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
