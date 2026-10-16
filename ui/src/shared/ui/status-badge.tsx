import { Check, CircleX, Clock, LoaderCircle, Wrench } from 'lucide-react'

import { label } from '@/shared/lib/display'
import { Badge } from './badge'

export function StatusBadge({ status }: { status: string }) {
  const good = ['verified', 'succeeded', 'ready'].includes(status)
  const busy = ['running', 'registered', 'provisioning'].includes(status)
  const bad = ['failed', 'error'].includes(status)
  const attention = ['repair_required', 'warning', 'interrupted'].includes(
    status,
  )
  const Icon = good
    ? Check
    : busy
      ? LoaderCircle
      : bad
        ? CircleX
        : attention
          ? Wrench
          : Clock
  return (
    <Badge
      variant={
        good ? 'default' : busy ? 'secondary' : bad ? 'destructive' : 'outline'
      }
    >
      <Icon aria-hidden="true" />
      {label(status)}
    </Badge>
  )
}
