import type { ReactNode } from 'react'

import { cn } from '@/shared/lib/utils'

interface EmptyStateProps {
  icon?: ReactNode
  title: string
  description?: string
  className?: string
}

export function EmptyState({ icon, title, description, className }: EmptyStateProps) {
  return (
    <div
      className={cn(
        'flex flex-col items-center gap-2 rounded-xl border border-dashed p-12 text-center text-sm text-muted-foreground',
        className,
      )}
    >
      {icon}
      <p className="text-lg font-bold tracking-tight text-foreground">{title}</p>
      {description ? <p>{description}</p> : null}
    </div>
  )
}
