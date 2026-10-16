import { useQuery } from '@tanstack/react-query'
import { useId, useState } from 'react'

import { gateway, IpcClientError, type InstalledNodeDto, type NodeGitStatusDto } from '@/shared/ipc/gateway'
import { errorMessage } from '@/shared/lib/display'
import { Alert, AlertDescription, AlertTitle } from '@/shared/ui/alert'
import { Button } from '@/shared/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shared/ui/dialog'
import { Input } from '@/shared/ui/input'
import { Label } from '@/shared/ui/label'

import { checkGitStatus, FAILURE_GUIDANCE, failureKind } from './model'

export const GIT_STATUS_KEY = ['node-git-status'] as const

/** The inspected Git status of one node. It never fetches, updates or runs node code. */
export function useGitStatus(nodeId: string, enabled = true) {
  return useQuery({
    queryKey: [...GIT_STATUS_KEY, nodeId],
    queryFn: () => gateway.nodeGitStatus(nodeId),
    enabled,
    gcTime: 0,
    staleTime: 0,
  })
}

export function GitStatusView({ status }: { status: NodeGitStatusDto }) {
  return (
    <div className="flex flex-col gap-2">
      <dl className="grid grid-cols-2 gap-x-4 gap-y-1 font-mono text-xs sm:grid-cols-4">
        <div>
          <dt className="text-muted-foreground">Branch</dt>
          <dd>{status.branch ?? 'detached'}</dd>
        </div>
        <div>
          <dt className="text-muted-foreground">Commit</dt>
          <dd className="break-all">{status.commit.slice(0, 12)}</dd>
        </div>
        <div>
          <dt className="text-muted-foreground">Upstream</dt>
          <dd>{status.hasUpstream ? 'Selected' : 'None'}</dd>
        </div>
        <div>
          <dt className="text-muted-foreground">Ahead / behind</dt>
          <dd>{status.ahead ?? '—'} / {status.behind ?? '—'}</dd>
        </div>
      </dl>
      <pre
        aria-label="Git status"
        className="max-h-40 overflow-auto rounded-md border bg-muted p-2 font-mono text-xs whitespace-pre-wrap break-all"
      >
        {status.status.trim() === '' ? 'Clean working tree' : status.status}
      </pre>
    </div>
  )
}

/** Inspects the repository first, then asks only for what an update still needs. */
export function UpdateDialog({ node, onPrepare, onClose }: {
  node: InstalledNodeDto
  onPrepare: (reference: string | null) => void
  onClose: () => void
}) {
  const prefix = useId()
  const isGit = node.source.kind === 'git'
  const git = useGitStatus(node.id, isGit)
  const [reference, setReference] = useState('')
  const check = git.data ? checkGitStatus(git.data) : null
  const target = reference.trim()
  const blocked = isGit && (check === null || check.refusals.length > 0 ||
    (check.needsReference && target === ''))

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Update {node.name}</DialogTitle>
          <DialogDescription>
            {isGit
              ? 'The repository is inspected first. Nothing is fetched or changed until you review the exact plan.'
              : 'The latest registry release is resolved once and shown in the review.'}
          </DialogDescription>
        </DialogHeader>
        {isGit && git.isPending && (
          <p role="status" className="text-sm text-muted-foreground">Inspecting the repository…</p>
        )}
        {isGit && git.isError && (
          <Alert variant="destructive">
            <AlertTitle>The repository could not be inspected</AlertTitle>
            <AlertDescription>
              {errorMessage(git.error)}{' '}
              {git.error instanceof IpcClientError && failureKind(git.error.code) === 'git_unavailable'
                ? FAILURE_GUIDANCE.git_unavailable
                : ''}
              <Button variant="outline" size="sm" onClick={() => void git.refetch()}>Inspect again</Button>
            </AlertDescription>
          </Alert>
        )}
        {git.data && <GitStatusView status={git.data} />}
        {check && check.refusals.length > 0 && (
          <Alert variant="destructive">
            <AlertTitle>This repository will not be updated</AlertTitle>
            <AlertDescription>
              <ul>{check.refusals.map((reason) => <li key={reason}>{reason}</li>)}</ul>
              Resolve this manually in the node folder, then inspect again.
              <Button variant="outline" size="sm" onClick={() => void git.refetch()}>Inspect again</Button>
            </AlertDescription>
          </Alert>
        )}
        {check?.needsReference && check.refusals.length === 0 && (
          <div className="flex flex-col gap-1.5">
            <Label htmlFor={`${prefix}-reference`}>Branch, tag or commit (required)</Label>
            <Input
              id={`${prefix}-reference`}
              className="font-mono"
              value={reference}
              onChange={(event) => setReference(event.target.value)}
            />
            <p className="text-xs text-muted-foreground">
              This repository is detached or has no selected upstream, so an explicit target is required.
            </p>
          </div>
        )}
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>Cancel</Button>
          <Button disabled={blocked} onClick={() => onPrepare(target || null)}>Prepare update</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
