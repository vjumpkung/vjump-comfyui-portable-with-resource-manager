import { Check, CircleX, LoaderCircle, Square } from 'lucide-react'

import type { LifecycleDto } from '@/shared/ipc/gateway'
import { Badge } from '@/shared/ui/badge'

const LABELS: Record<LifecycleDto, string> = {
  stopped: 'Stopped',
  starting: 'Starting',
  running: 'Running',
  stopping: 'Stopping',
  crashed: 'Crashed',
}

export function lifecycleLabel(lifecycle: LifecycleDto | undefined): string {
  return lifecycle ? LABELS[lifecycle] : 'Connecting'
}

export function LifecycleBadge({
  lifecycle,
}: {
  lifecycle: LifecycleDto | undefined
}) {
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
      {lifecycleLabel(lifecycle)}
    </Badge>
  )
}
