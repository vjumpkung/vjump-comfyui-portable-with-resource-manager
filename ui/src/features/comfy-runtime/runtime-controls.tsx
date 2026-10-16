import { Play, Square } from 'lucide-react'
import { toast } from 'sonner'

import { errorMessage } from '@/shared/lib/display'
import { Button } from '@/shared/ui/button'

import { LifecycleBadge } from './lifecycle-badge'
import { useRuntime, useRuntimeActions } from './use-runtime'

/** Live status chip and Start/Stop button for the top bar. */
export function RuntimeControls() {
  const runtime = useRuntime()
  const { start, stop } = useRuntimeActions()
  const lifecycle = runtime.status?.lifecycle
  const canStop = lifecycle === 'starting' || lifecycle === 'running'

  function onStart() {
    start.mutate(undefined, {
      onSuccess: (result) => {
        if (result.kind === 'port_conflict') {
          toast.error(
            `Port ${result.requested} is in use. Open the ComfyUI page to choose another port.`,
          )
        }
      },
      onError: (error) => toast.error(errorMessage(error)),
    })
  }

  return (
    <div className="flex items-center gap-3">
      <LifecycleBadge lifecycle={lifecycle} />
      {canStop ? (
        <Button
          variant="outline"
          size="sm"
          disabled={stop.isPending}
          onClick={() =>
            stop.mutate(undefined, {
              onError: (error) => toast.error(errorMessage(error)),
            })
          }
        >
          <Square aria-hidden="true" />
          Stop
        </Button>
      ) : (
        <Button
          size="sm"
          disabled={
            start.isPending || !runtime.status || lifecycle === 'stopping'
          }
          onClick={onStart}
        >
          <Play aria-hidden="true" />
          Start
        </Button>
      )}
    </div>
  )
}
