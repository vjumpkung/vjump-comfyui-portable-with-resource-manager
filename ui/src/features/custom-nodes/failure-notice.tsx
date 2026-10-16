import { TriangleAlert } from 'lucide-react'

import { errorMessage } from '@/shared/lib/display'
import { Alert, AlertDescription, AlertTitle } from '@/shared/ui/alert'
import { Button } from '@/shared/ui/button'

import { FAILURE_GUIDANCE, type NodePreparation } from './model'
import type { ActionFailure } from './use-node-actions'
import { GitStatusView, useGitStatus } from './update-dialog'

function GitRefusalDetails({ nodeId }: { nodeId: string }) {
  const status = useGitStatus(nodeId)
  if (status.isPending) {
    return <p role="status" className="text-xs">Reading the Git status…</p>
  }
  if (status.isError) {
    return <p className="text-xs">The Git status could not be read: {errorMessage(status.error)}</p>
  }
  return (
    <div className="mt-2 flex flex-col gap-1">
      <p className="font-semibold">Git status</p>
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
  const informational = failure.kind === 'up_to_date' || failure.kind === 'busy' || failure.kind === 'cancelled'
  const guidance = FAILURE_GUIDANCE[failure.kind]
  const prepare = failure.prepare
  return (
    <Alert variant={informational ? 'default' : 'destructive'}>
      <TriangleAlert aria-hidden="true" />
      <AlertTitle>
        {failure.kind === 'up_to_date' ? 'Already up to date' : `${failure.context} did not complete`}
      </AlertTitle>
      <AlertDescription>
        <p className="break-words [overflow-wrap:anywhere]">{failure.message}</p>
        {guidance !== '' && <p>{guidance}</p>}
        {failure.kind === 'git_refused' && failure.nodeId !== null && (
          <GitRefusalDetails nodeId={failure.nodeId} />
        )}
        <div className="mt-2 flex flex-wrap gap-2">
          {failure.kind === 'comfyui_running' && failure.retry !== null && (
            <Button size="sm" disabled={stopping} onClick={onStopAndRetry}>
              {stopping ? 'Stopping ComfyUI…' : 'Stop ComfyUI and retry'}
            </Button>
          )}
          {failure.kind === 'busy' && failure.retry !== null && (
            <Button variant="outline" size="sm" onClick={onRetry}>Try again</Button>
          )}
          {failure.kind === 'stale' && prepare !== null && (
            <Button size="sm" onClick={() => onPrepareAgain(prepare)}>
              Prepare again
            </Button>
          )}
          {failure.kind === 'stale' && prepare === null && (
            <p className="text-xs">Start the install or update again to review a fresh plan.</p>
          )}
          {(failure.kind === 'dependencies' || failure.kind === 'cancelled') && (
            <Button variant="outline" size="sm" onClick={onRepair}>Go to Repair</Button>
          )}
          {failure.kind === 'recovery' && (
            <Button variant="outline" size="sm" onClick={onRescan}>Rescan nodes</Button>
          )}
          <Button variant="ghost" size="sm" onClick={onDismiss}>Dismiss</Button>
        </div>
      </AlertDescription>
    </Alert>
  )
}
