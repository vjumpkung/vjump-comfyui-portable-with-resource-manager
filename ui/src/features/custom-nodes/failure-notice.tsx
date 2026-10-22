import { TriangleAlert } from 'lucide-react'

import { useTranslation } from '@/shared/i18n'
import { errorMessage } from '@/shared/lib/display'
import { Alert, AlertDescription, AlertTitle } from '@/shared/ui/alert'
import { Button } from '@/shared/ui/button'

import { actionContextLabel, FAILURE_GUIDANCE, nodeText, type NodePreparation } from './model'
import type { ActionFailure } from './use-node-actions'
import { GitStatusView, useGitStatus } from './update-dialog'

function GitRefusalDetails({ nodeId }: { nodeId: string }) {
  const { t } = useTranslation()
  const status = useGitStatus(nodeId)
  if (status.isPending) {
    return <p role="status" className="text-xs">{t('customNodes.readingGit')}</p>
  }
  if (status.isError) {
    return <p className="text-xs">{t('customNodes.gitReadError', { error: errorMessage(status.error) })}</p>
  }
  return (
    <div className="mt-2 flex flex-col gap-1">
      <p className="font-semibold">{t('customNodes.gitStatus')}</p>
      <GitStatusView status={status.data} />
    </div>
  )
}

/** Turns a refusal into the one next step the backend allows; nothing is overridden silently. */
export function FailureNotice({
  failure,
  stopping,
  onStopAndRetry,
  onRetry,
  onPrepareAgain,
  onRepair,
  onRescan,
  onDismiss,
}: {
  failure: ActionFailure
  stopping: boolean
  onStopAndRetry: () => void
  onRetry: () => void
  onPrepareAgain: (preparation: NodePreparation) => void
  onRepair: () => void
  onRescan: () => void
  onDismiss: () => void
}) {
  const { t } = useTranslation()
  const informational = failure.kind === 'up_to_date' || failure.kind === 'busy' || failure.kind === 'cancelled'
  const guidanceKey = FAILURE_GUIDANCE[failure.kind]
  const prepare = failure.prepare
  return (
    <Alert variant={informational ? 'default' : 'destructive'}>
      <TriangleAlert aria-hidden="true" />
      <AlertTitle>
        {failure.kind === 'up_to_date' ? t('customNodes.upToDate') : t('customNodes.didNotComplete', { context: actionContextLabel(failure.context) })}
      </AlertTitle>
      <AlertDescription>
        <p className="break-words [overflow-wrap:anywhere]">{nodeText(failure.message)}</p>
        {guidanceKey !== null && <p>{t(guidanceKey)}</p>}
        {failure.kind === 'git_refused' && failure.nodeId !== null && (
          <GitRefusalDetails nodeId={failure.nodeId} />
        )}
        <div className="mt-2 flex flex-wrap gap-2">
          {failure.kind === 'comfyui_running' && failure.retry !== null && (
            <Button size="sm" disabled={stopping} onClick={onStopAndRetry}>
              {stopping ? t('customNodes.stoppingComfy') : t('customNodes.stopAndRetry')}
            </Button>
          )}
          {failure.kind === 'busy' && failure.retry !== null && (
            <Button variant="outline" size="sm" onClick={onRetry}>{t('common.tryAgain')}</Button>
          )}
          {failure.kind === 'stale' && prepare !== null && (
            <Button size="sm" onClick={() => onPrepareAgain(prepare)}>
              {t('customNodes.prepareAgain')}
            </Button>
          )}
          {failure.kind === 'stale' && prepare === null && (
            <p className="text-xs">{t('customNodes.freshPlanHint')}</p>
          )}
          {(failure.kind === 'dependencies' || failure.kind === 'cancelled') && (
            <Button variant="outline" size="sm" onClick={onRepair}>{t('customNodes.goRepair')}</Button>
          )}
          {failure.kind === 'recovery' && (
            <Button variant="outline" size="sm" onClick={onRescan}>{t('customNodes.rescan')}</Button>
          )}
          <Button variant="ghost" size="sm" onClick={onDismiss}>{t('common.dismiss')}</Button>
        </div>
      </AlertDescription>
    </Alert>
  )
}
