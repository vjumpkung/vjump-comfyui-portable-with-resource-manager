import { ChevronRight } from 'lucide-react'

import { useTranslation } from '@/shared/i18n'
import type { GalleryAreaDto } from '@/shared/ipc/gateway'
import { Button } from '@/shared/ui/button'
import { ToggleGroup, ToggleGroupItem } from '@/shared/ui/toggle-group'

import { segmentsOf } from './format'

/** Input / Output. Choosing the active side again keeps it selected. */
export function AreaToggle({
  area,
  onChange,
}: {
  area: GalleryAreaDto
  onChange: (area: GalleryAreaDto) => void
}) {
  const { t } = useTranslation()
  return (
    <ToggleGroup
      type="single"
      variant="outline"
      aria-label={t('gallery.area')}
      value={area}
      onValueChange={(value) => {
        if (value === 'input' || value === 'output') onChange(value)
      }}
    >
      <ToggleGroupItem value="input">{t('gallery.area.input')}</ToggleGroupItem>
      <ToggleGroupItem value="output">{t('gallery.area.output')}</ToggleGroupItem>
    </ToggleGroup>
  )
}

/** The area root followed by one button per folder on the way down to `path`. */
export function Breadcrumb({
  area,
  path,
  onNavigate,
}: {
  area: GalleryAreaDto
  path: string
  onNavigate: (path: string) => void
}) {
  const { t } = useTranslation()
  const segments = segmentsOf(path)
  return (
    <nav aria-label={t('gallery.breadcrumb')}>
      <ol className="flex flex-wrap items-center gap-1">
        <li>
          <Button
            variant="ghost"
            size="sm"
            aria-current={segments.length === 0 ? 'page' : undefined}
            onClick={() => onNavigate('')}
          >
            {t(`gallery.area.${area}`)}
          </Button>
        </li>
        {segments.map((segment, index) => {
          const target = segments.slice(0, index + 1).join('/')
          return (
            <li key={target} className="flex items-center gap-1">
              <ChevronRight className="size-3.5 text-muted-foreground" aria-hidden="true" />
              <Button
                variant="ghost"
                size="sm"
                aria-current={index === segments.length - 1 ? 'page' : undefined}
                onClick={() => onNavigate(target)}
              >
                {segment}
              </Button>
            </li>
          )
        })}
      </ol>
    </nav>
  )
}
