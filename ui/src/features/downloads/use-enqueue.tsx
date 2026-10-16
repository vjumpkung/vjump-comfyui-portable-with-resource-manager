import { useCallback, useState, type ReactNode } from 'react'
import { toast } from 'sonner'

import {
  gateway,
  type DestinationDto,
  type EnqueueItemInput,
  type EnqueueOutcomeDto,
  type FileIdentityDto,
} from '@/shared/ipc/gateway'
import { errorMessage } from '@/shared/lib/display'

import { ChoiceDialog } from './choice-dialog'

/** Provider errors that the user fixes by adding or replacing a token in Settings. */
export const CREDENTIAL_CODES = ['token_required', 'invalid_token']

type Prompt =
  | { kind: 'already'; item: EnqueueItemInput; destination: DestinationDto }
  | {
      kind: 'conflict'
      item: EnqueueItemInput
      destination: DestinationDto
      current: FileIdentityDto | null
    }

export interface EnqueueFlow {
  submit: (items: EnqueueItemInput[]) => Promise<number>
  /** Render once; shows the next pending decision. */
  prompts: ReactNode
}

/**
 * Queues items and walks the user through duplicate, already-downloaded and name-conflict
 * outcomes. Every decision re-queues exactly the affected item.
 */
export function useEnqueue(onOpenSettings: () => void): EnqueueFlow {
  const [queue, setQueue] = useState<Prompt[]>([])
  const [busy, setBusy] = useState(false)

  const run = useCallback(
    async (items: EnqueueItemInput[]): Promise<number> => {
      let outcomes: EnqueueOutcomeDto[]
      try {
        outcomes = await gateway.dlEnqueue(items)
      } catch (error) {
        toast.error(errorMessage(error))
        return 0
      }
      let queued = 0
      const next: Prompt[] = []
      outcomes.forEach((outcome, index) => {
        const item = items[index]
        switch (outcome.kind) {
          case 'queued':
            queued += 1
            break
          case 'duplicate':
            toast.info('That download is already in the queue.')
            break
          case 'already_downloaded':
            next.push({
              kind: 'already',
              item,
              destination: outcome.destination,
            })
            break
          case 'conflict':
            next.push({
              kind: 'conflict',
              item,
              destination: outcome.destination,
              current: outcome.current,
            })
            break
          case 'skipped':
            break
          case 'invalid':
            toast.error(outcome.error.message, {
              action: CREDENTIAL_CODES.includes(outcome.error.code)
                ? { label: 'Open Settings', onClick: onOpenSettings }
                : undefined,
            })
            break
        }
      })
      if (queued > 0) {
        toast.success(
          queued === 1 ? 'Download queued.' : `${queued} downloads queued.`,
        )
      }
      if (next.length > 0) setQueue((current) => [...current, ...next])
      return queued
    },
    [onOpenSettings],
  )

  const prompt = queue[0]
  const settle = (): void => setQueue((current) => current.slice(1))

  async function choose(next: Prompt, choice: string): Promise<void> {
    setBusy(true)
    try {
      if (next.kind === 'already' && choice === 'again') {
        await run([{ ...next.item, force: true }])
      } else if (next.kind === 'conflict' && choice === 'overwrite') {
        await run([
          {
            ...next.item,
            conflict: { kind: 'overwrite', approved: next.current },
          },
        ])
      } else if (next.kind === 'conflict' && choice === 'keep_both') {
        await run([{ ...next.item, conflict: { kind: 'keep_both' } }])
      }
    } finally {
      setBusy(false)
      settle()
    }
  }

  let dialog: ReactNode = null
  if (prompt?.kind === 'already') {
    dialog = (
      <ChoiceDialog
        title="Already downloaded"
        description={`A file with the same checksum is already in your model folders (${prompt.destination.relativePath}).`}
        busy={busy}
        choices={[
          { id: 'skip', label: 'Skip' },
          { id: 'again', label: 'Download again', variant: 'default' },
        ]}
        onChoose={(choice) => void choose(prompt, choice)}
        onDismiss={settle}
      />
    )
  } else if (prompt?.kind === 'conflict') {
    dialog = (
      <ChoiceDialog
        title="A file with this name exists"
        description={`${prompt.destination.relativePath} already exists.`}
        busy={busy}
        note={
          prompt.current
            ? 'Overwrite moves the existing file to the Recycle Bin or Trash; it is not deleted permanently.'
            : 'Another download is already using this name, so it cannot be overwritten.'
        }
        choices={[
          { id: 'skip', label: 'Skip' },
          { id: 'keep_both', label: 'Keep both' },
          {
            id: 'overwrite',
            label: 'Overwrite (move old to Recycle Bin)',
            variant: 'destructive',
            disabled: prompt.current === null,
          },
        ]}
        onChoose={(choice) => void choose(prompt, choice)}
        onDismiss={settle}
      />
    )
  }

  return { submit: run, prompts: dialog }
}
