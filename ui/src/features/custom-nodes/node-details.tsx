import { ArrowUpCircle, ExternalLink, FolderOpen, Power, PowerOff, RotateCw, Trash2, TriangleAlert } from 'lucide-react'
import { toast } from 'sonner'

import { useTranslation } from '@/shared/i18n'
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
  const { t } = useTranslation()
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

  async function revealFolder() {
    try {
      await gateway.nodeReveal(node.id)
    } catch (error) {
      toast.error(errorMessage(error))
    }
  }

  return (
    <section aria-label={t('customNodes.actionsFor', { name: node.name })} className="flex flex-col gap-4 rounded-xl border bg-card p-4">
      <div className="min-w-0">
        <h2 className="text-lg font-bold tracking-tight break-words [overflow-wrap:anywhere]">{node.name}</h2>
        <p className="font-mono text-xs text-muted-foreground break-all">{sourceLabel(node.source)}</p>
      </div>
      <div className="flex flex-wrap gap-2">
        <EnabledChip enabled={node.enabled} />
        <DependencyChip state={node.dependencies} />
        <HealthChip health={gitHealth(node)} />
        {node.conflict && <Chip icon={TriangleAlert} className="bg-brand-pink">{t('customNodes.conflict')}</Chip>}
      </div>
      <div className="flex flex-wrap gap-2">
        <Button variant="outline" disabled={busy || updateReason !== null} onClick={onUpdate}>
          <ArrowUpCircle aria-hidden="true" />
          {t('common.update')}
        </Button>
        <Button variant="outline" disabled={busy || !canRetryDependencies(node)} onClick={onRetry}>
          <RotateCw aria-hidden="true" />
          {t('customNodes.retryDependencies')}
        </Button>
        <Button variant="outline" disabled={busy} onClick={onToggle}>
          {node.enabled ? <PowerOff aria-hidden="true" /> : <Power aria-hidden="true" />}
          {node.enabled ? t('common.disable') : t('common.enable')}
        </Button>
        <Button variant="destructive" disabled={busy} onClick={onUninstall}>
          <Trash2 aria-hidden="true" />
          {t('common.uninstall')}
        </Button>
        <Button variant="outline" onClick={() => void revealFolder()}>
          <FolderOpen aria-hidden="true" />
          {t('common.showInExplorer')}
        </Button>
        {repository !== null && (
          <Button variant="ghost" onClick={() => void openRepository()}>
            {t('customNodes.openRepository')}
            <ExternalLink aria-hidden="true" />
          </Button>
        )}
      </div>
      {(updateReason !== null || retryReason !== null) && (
        <ul className="flex flex-col gap-0.5 text-xs text-muted-foreground">
          {updateReason !== null && <li>{t('customNodes.updateUnavailable', { reason: updateReason })}</li>}
          {retryReason !== null && <li>{t('customNodes.retryUnavailable', { reason: retryReason })}</li>}
        </ul>
      )}
    </section>
  )
}
