import { Check, CircleX, LoaderCircle, Square } from 'lucide-react'

import { t, useTranslation } from '@/shared/i18n'
import type { LifecycleDto } from '@/shared/ipc/gateway'
import { Badge } from '@/shared/ui/badge'

const LABEL_KEYS = {
  stopped: 'runtime.lifecycle.stopped',
  starting: 'runtime.lifecycle.starting',
  running: 'runtime.lifecycle.running',
  stopping: 'runtime.lifecycle.stopping',
  crashed: 'runtime.lifecycle.crashed',
} as const satisfies Record<LifecycleDto, keyof typeof import('@/shared/i18n/translations.json').en>

export function lifecycleLabel(lifecycle: LifecycleDto | undefined, translate = t): string {
  return lifecycle ? translate(LABEL_KEYS[lifecycle]) : translate('runtime.lifecycle.connecting')
}

export function LifecycleBadge({
  lifecycle,
}: {
  lifecycle: LifecycleDto | undefined
}) {
  const { t } = useTranslation()
  const busy = lifecycle === 'starting' || lifecycle === 'stopping'
  const Icon =
    lifecycle === 'running'
      ? Check
      : busy || !lifecycle
        ? LoaderCircle
        : lifecycle === 'crashed'
          ? CircleX
          : Square
  return (
    <Badge
      variant={
        lifecycle === 'running'
          ? 'default'
          : lifecycle === 'crashed'
            ? 'destructive'
            : busy || !lifecycle
              ? 'secondary'
              : 'outline'
      }
      data-lifecycle={lifecycle ?? 'connecting'}
    >
      <Icon aria-hidden="true" />
      {lifecycleLabel(lifecycle, t)}
    </Badge>
  )
}
