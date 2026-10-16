import { ArrowUpCircle, ExternalLink, Power, PowerOff, RotateCw, Trash2, TriangleAlert } from 'lucide-react'
import { toast } from 'sonner'

import { gateway, type InstalledNodeDto } from '@/shared/ipc/gateway'
import { errorMessage } from '@/shared/lib/display'
import { Button } from '@/shared/ui/button'

import {
  canRetryDependencies,
  gitHealth,
  repositoryOf,
  retryUnavailableReason,
  sourceLabel,
  updateUnavailableReason,
} from './model'
import { Chip, DependencyChip, EnabledChip, HealthChip } from './status-chips'

/** The actions of the active node, kept beside the table instead of inside fixed-height rows. */
export function NodeDetails({
  node,
  repairRequired,
  busy,
  onUpdate,
  onRetry,
  onToggle,
  onUninstall,
}: {
  node: InstalledNodeDto
  repairRequired: boolean
  busy: boolean
  onUpdate: () => void
  onRetry: () => void
  onToggle: () => void
  onUninstall: () => void
}) {
  const repository = repositoryOf(node)
  const updateReason = updateUnavailableReason(node, repairRequired)
  const retryReason = node.dependencies === 'ok' ? null : retryUnavailableReason(node)

  async function openRepository() {
    try {
      await gateway.nodeOpenRepository(repository ?? '')
    } catch (error) {
      toast.error(errorMessage(error))
    }
  }

  return (
    <section aria-label={`Actions for ${node.name}`} className="flex flex-col gap-4 rounded-xl border bg-card p-4">
      <div className="min-w-0">
        <h2 className="text-lg font-bold tracking-tight break-words [overflow-wrap:anywhere]">{node.name}</h2>
        <p className="font-mono text-xs text-muted-foreground break-all">{sourceLabel(node.source)}</p>
      </div>
      <div className="flex flex-wrap gap-2">
        <EnabledChip enabled={node.enabled} />
        <DependencyChip state={node.dependencies} />
        <HealthChip health={gitHealth(node)} />
        {node.conflict && <Chip icon={TriangleAlert} className="bg-brand-pink">Conflict</Chip>}
      </div>
      <div className="flex flex-wrap gap-2">
        <Button variant="outline" disabled={busy || updateReason !== null} onClick={onUpdate}>
          <ArrowUpCircle aria-hidden="true" />
          Update
        </Button>
        <Button variant="outline" disabled={busy || !canRetryDependencies(node)} onClick={onRetry}>
          <RotateCw aria-hidden="true" />
          Retry dependencies
        </Button>
        <Button variant="outline" disabled={busy} onClick={onToggle}>
          {node.enabled ? <PowerOff aria-hidden="true" /> : <Power aria-hidden="true" />}
          {node.enabled ? 'Disable' : 'Enable'}
        </Button>
        <Button variant="destructive" disabled={busy} onClick={onUninstall}>
          <Trash2 aria-hidden="true" />
          Uninstall
        </Button>
        {repository !== null && (
          <Button variant="ghost" onClick={() => void openRepository()}>
            Open repository
            <ExternalLink aria-hidden="true" />
          </Button>
        )}
      </div>
      {(updateReason !== null || retryReason !== null) && (
        <ul className="flex flex-col gap-0.5 text-xs text-muted-foreground">
          {updateReason !== null && <li>Update unavailable: {updateReason}</li>}
          {retryReason !== null && <li>Retry unavailable: {retryReason}</li>}
        </ul>
      )}
    </section>
  )
}
