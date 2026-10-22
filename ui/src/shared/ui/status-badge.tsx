import { Check, CircleX, Clock, LoaderCircle, Wrench } from 'lucide-react'

import { useTranslation } from '@/shared/i18n'
import { label } from '@/shared/lib/display'
import { Badge } from './badge'

const STATUS_LABELS = {
  verified: 'status.verified',
  succeeded: 'status.succeeded',
  ready: 'status.ready',
  running: 'status.running',
  registered: 'status.registered',
  provisioning: 'status.provisioning',
  failed: 'status.failed',
  error: 'status.error',
  repair_required: 'status.repairRequired',
  warning: 'status.warning',
  interrupted: 'status.interrupted',
  pending: 'status.pending',
  skipped: 'status.skipped',
  cancelled: 'status.cancelled',
  prepared: 'status.prepared',
  installed: 'status.installed',
  queued: 'status.queued',
  paused: 'status.paused',
  completed: 'status.completed',
  starting: 'status.starting',
  stopping: 'status.stopping',
  stopped: 'status.stopped',
  unverified: 'status.unverified',
  downloading: 'status.downloading',
} as const satisfies Record<string, keyof typeof import('@/shared/i18n/translations.json').en>

export function StatusBadge({ status }: { status: string }) {
  const { t } = useTranslation()
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
      {Object.hasOwn(STATUS_LABELS, status)
        ? t(STATUS_LABELS[status as keyof typeof STATUS_LABELS])
        : label(status)}
    </Badge>
  )
}
