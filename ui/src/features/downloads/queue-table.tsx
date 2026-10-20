import { useMutation } from '@tanstack/react-query'
import { Pause, Play, RotateCw, X } from 'lucide-react'
import { useState, type KeyboardEvent } from 'react'
import { toast } from 'sonner'

import {
  gateway,
  type ConflictChoiceDto,
  type DownloadJobDto,
  type ProgressDto,
} from '@/shared/ipc/gateway'
import { errorMessage } from '@/shared/lib/display'
import { cn } from '@/shared/lib/utils'
import { Button } from '@/shared/ui/button'
import { Progress } from '@/shared/ui/progress'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/shared/ui/table'

import { ChoiceDialog } from './choice-dialog'
import { isActive, percent, progressText } from './format'
import { IntegrityChip, StatusChip } from './status-chip'
import { CREDENTIAL_CODES } from './use-enqueue'

const ROW_HEIGHT = 92
const WINDOW_AFTER = 200
const VIEWPORT_ROWS = 14

interface Props {
  jobs: DownloadJobDto[]
  progress: Record<string, ProgressDto>
  onOpenSettings: () => void
}

function useJobAction<T>(fn: (input: T) => Promise<unknown>) {
  return useMutation({
    mutationFn: fn,
    onError: (error) => toast.error(errorMessage(error)),
  })
}

/** Newest active work first, then history by last change. */
export function sortJobs(jobs: DownloadJobDto[]): DownloadJobDto[] {
  return [...jobs].sort((a, b) => {
    const activeDelta = Number(isActive(b)) - Number(isActive(a))
    if (activeDelta !== 0) return activeDelta
    return isActive(a)
      ? (a.createdAt ?? 0) - (b.createdAt ?? 0)
      : (b.updatedAt ?? 0) - (a.updatedAt ?? 0)
  })
}

function Row({
  job,
  progress,
  onOpenSettings,
  onResolve,
}: {
  job: DownloadJobDto
  progress: ProgressDto | undefined
  onOpenSettings: () => void
  onResolve: (job: DownloadJobDto) => void
}) {
  const pause = useJobAction((id: string) => gateway.dlPause(id))
  const resume = useJobAction((id: string) => gateway.dlResume(id, null))
  const cancel = useJobAction((id: string) => gateway.dlCancel(id))
  const retry = useJobAction((id: string) => gateway.dlRetry(id))

  const done = progress?.bytes ?? job.bytesDone ?? 0
  const total = progress?.total ?? job.size
  const value = percent(done, total)
  const showBar = ['downloading', 'verifying', 'paused'].includes(job.state)
  const busy =
    pause.isPending || resume.isPending || cancel.isPending || retry.isPending
  const waitingForDecision = job.state === 'paused' && job.decisionId !== null
  const label = job.destination.filename

  return (
    <TableRow
      data-row=""
      tabIndex={0}
      className="focus-visible:ring-2 focus-visible:ring-ring"
      style={{ height: ROW_HEIGHT }}
    >
      <TableCell className="max-w-72 align-top whitespace-normal">
        <p className="text-sm font-semibold break-words [overflow-wrap:anywhere]">
          {label}
        </p>
        <p className="font-mono text-xs text-muted-foreground break-words [overflow-wrap:anywhere]">
          {job.destination.relativePath}
        </p>
        <p className="font-mono text-xs text-muted-foreground break-words [overflow-wrap:anywhere]">
          {job.label}
        </p>
      </TableCell>
      <TableCell className="align-top">
        <div className="flex flex-col items-start gap-1">
          <StatusChip state={job.state} />
          {job.state === 'completed' ? (
            <IntegrityChip verified={job.verified} />
          ) : null}
        </div>
      </TableCell>
      <TableCell className="min-w-56 align-top whitespace-normal">
        {showBar ? (
          <div className="flex flex-col gap-1">
            <Progress
              value={value ?? 0}
              aria-label={`Progress of ${label}`}
              className={cn(job.state === 'paused' && '[&>div]:bg-brand-pink')}
            />
            <p className="font-mono text-xs text-muted-foreground">
              {job.finalizing
                ? 'Verifying and publishing'
                : progressText(job, progress)}
            </p>
          </div>
        ) : null}
        {job.state === 'queued' && job.retryCount > 0 ? (
          <p className="font-mono text-xs text-muted-foreground">
            Retrying after a network error (attempt {job.retryCount + 1})
          </p>
        ) : null}
        {job.error && job.state !== 'completed' ? (
          <div className="flex flex-col items-start gap-1">
            <p className="text-xs text-destructive">{job.error.message}</p>
            {CREDENTIAL_CODES.includes(job.error.code) ? (
              <Button variant="outline" size="xs" onClick={onOpenSettings}>
                Open Settings
              </Button>
            ) : null}
          </div>
        ) : null}
        {job.state === 'paused' && job.restartsFromZero && !job.finalizing ? (
          <p className="text-xs text-muted-foreground">
            Resuming restarts this download from 0%.
          </p>
        ) : null}
      </TableCell>
      <TableCell className="align-top">
        <div className="flex items-center justify-end gap-1">
          {['queued', 'resolving', 'downloading'].includes(job.state) ? (
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label={`Pause ${label}`}
              title={
                job.restartsFromZero
                  ? 'Pause (this download will restart from 0%)'
                  : 'Pause'
              }
              disabled={busy}
              onClick={() => pause.mutate(job.id)}
            >
              <Pause aria-hidden="true" />
            </Button>
          ) : null}
          {job.state === 'paused' ? (
            waitingForDecision ? (
              <Button
                variant="outline"
                size="xs"
                disabled={busy}
                onClick={() => onResolve(job)}
              >
                Resolve conflict
              </Button>
            ) : (
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label={`Resume ${label}`}
                title={
                  job.restartsFromZero
                    ? 'Resume (restarts from 0%)'
                    : 'Resume'
                }
                disabled={busy}
                onClick={() => resume.mutate(job.id)}
              >
                <Play aria-hidden="true" />
              </Button>
            )
          ) : null}
          {job.state === 'failed' ? (
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label={`Retry ${label}`}
              title="Retry"
              disabled={busy}
              onClick={() => retry.mutate(job.id)}
            >
              <RotateCw aria-hidden="true" />
            </Button>
          ) : null}
          {isActive(job) && !job.finalizing ? (
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label={`Cancel ${label}`}
              title="Cancel"
              disabled={busy}
              onClick={() => cancel.mutate(job.id)}
            >
              <X aria-hidden="true" />
            </Button>
          ) : null}
        </div>
      </TableCell>
    </TableRow>
  )
}

/** The queue and history. Past 200 rows only the visible window is rendered. */
export function QueueTable({ jobs, progress, onOpenSettings }: Props) {
  const [scrollTop, setScrollTop] = useState(0)
  const [conflictId, setConflictId] = useState<string | null>(null)
  // Derived from the live list so a refreshed prompt carries the newest observed identity.
  const conflictJob = jobs.find((job) => job.id === conflictId) ?? null
  const sorted = sortJobs(jobs)
  const windowed = sorted.length > WINDOW_AFTER

  const decide = useMutation({
    mutationFn: ({
      job,
      choice,
    }: {
      job: DownloadJobDto
      choice: ConflictChoiceDto
    }) =>
      gateway.dlResume(job.id, {
        decisionId: job.decisionId ?? '',
        choice,
      }),
    // A stale decision keeps the dialog open: the job now carries a fresh prompt.
    onError: (error) => toast.error(errorMessage(error)),
    onSuccess: () => setConflictId(null),
  })
  const abandon = useMutation({
    mutationFn: (job: DownloadJobDto) => gateway.dlCancel(job.id),
    onError: (error) => toast.error(errorMessage(error)),
    onSettled: () => setConflictId(null),
  })

  const first = windowed ? Math.floor(scrollTop / ROW_HEIGHT) : 0
  const start = windowed ? Math.max(0, first - 4) : 0
  const end = windowed
    ? Math.min(sorted.length, first + VIEWPORT_ROWS + 4)
    : sorted.length
  const visible = sorted.slice(start, end)

  function onKeyDown(event: KeyboardEvent<HTMLTableSectionElement>) {
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return
    const target = event.target as HTMLElement
    const row = target.closest<HTMLElement>('[data-row]')
    if (!row || target !== row) return
    const sibling =
      event.key === 'ArrowDown'
        ? row.nextElementSibling
        : row.previousElementSibling
    if (sibling instanceof HTMLElement && sibling.hasAttribute('data-row')) {
      event.preventDefault()
      sibling.focus()
    }
  }

  return (
    <>
      <div
        className={cn(
          'overflow-x-auto rounded-lg border bg-card',
          windowed && 'max-h-[70vh] overflow-y-auto',
        )}
        onScroll={(event) => {
          if (windowed) setScrollTop(event.currentTarget.scrollTop)
        }}
      >
        <Table>
          <TableHeader>
            <TableRow className="bg-brand-cyan text-[#2a313a] hover:bg-brand-cyan">
              <TableHead className="text-[#2a313a]">Download</TableHead>
              <TableHead className="text-[#2a313a]">Status</TableHead>
              <TableHead className="text-[#2a313a]">Progress</TableHead>
              <TableHead className="text-right text-[#2a313a]">
                Actions
              </TableHead>
            </TableRow>
          </TableHeader>
          <TableBody onKeyDown={onKeyDown}>
            {windowed && start > 0 ? (
              <tr aria-hidden="true" style={{ height: start * ROW_HEIGHT }} />
            ) : null}
            {visible.map((job) => (
              <Row
                key={job.id}
                job={job}
                progress={progress[job.id]}
                onOpenSettings={onOpenSettings}
                onResolve={(job) => setConflictId(job.id)}
              />
            ))}
            {windowed && end < sorted.length ? (
              <tr
                aria-hidden="true"
                style={{ height: (sorted.length - end) * ROW_HEIGHT }}
              />
            ) : null}
          </TableBody>
        </Table>
      </div>
      {conflictJob ? (
        <ChoiceDialog
          title="A file with this name appeared"
          description={`${conflictJob.destination.relativePath} now exists, so the finished download was held back.`}
          note="Overwrite moves the existing file to the Recycle Bin or Trash; it is not deleted permanently."
          busy={decide.isPending || abandon.isPending}
          choices={[
            { id: 'cancel', label: 'Cancel download' },
            { id: 'keep_both', label: 'Keep both' },
            {
              id: 'overwrite',
              label: 'Overwrite (move old to Recycle Bin)',
              variant: 'destructive',
            },
          ]}
          onChoose={(choice) => {
            if (choice === 'cancel') abandon.mutate(conflictJob)
            else
              decide.mutate({
                job: conflictJob,
                choice:
                  choice === 'keep_both'
                    ? { kind: 'keep_both' }
                    : {
                        kind: 'overwrite',
                        approved: conflictJob.conflictCurrent,
                      },
              })
          }}
          onDismiss={() => setConflictId(null)}
        />
      ) : null}
    </>
  )
}
