import { useQuery } from '@tanstack/react-query'
import { useId, useState } from 'react'

import { useTranslation } from '@/shared/i18n'
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
  const { t } = useTranslation()
  return (
    <div className="flex flex-col gap-2">
      <dl className="grid grid-cols-2 gap-x-4 gap-y-1 font-mono text-xs sm:grid-cols-4">
        <div>
          <dt className="text-muted-foreground">{t('customNodes.branch')}</dt>
          <dd>{status.branch ?? t('customNodes.detached')}</dd>
        </div>
        <div>
          <dt className="text-muted-foreground">{t('customNodes.commit')}</dt>
          <dd className="break-all">{status.commit.slice(0, 12)}</dd>
        </div>
        <div>
          <dt className="text-muted-foreground">{t('customNodes.upstream')}</dt>
          <dd>{status.hasUpstream ? t('customNodes.update.selected') : t('common.none')}</dd>
        </div>
        <div>
          <dt className="text-muted-foreground">{t('customNodes.aheadBehind')}</dt>
          <dd>{status.ahead ?? '—'} / {status.behind ?? '—'}</dd>
        </div>
      </dl>
      <pre
        aria-label={t('customNodes.gitStatus')}
        className="max-h-40 overflow-auto rounded-md border bg-muted p-2 font-mono text-xs whitespace-pre-wrap break-all"
      >
        {status.status.trim() === '' ? t('customNodes.update.cleanTree') : status.status}
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
  const { t } = useTranslation()
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
          <DialogTitle>{t('customNodes.updateName', { name: node.name })}</DialogTitle>
          <DialogDescription>
            {isGit
              ? t('customNodes.update.gitHint')
              : t('customNodes.update.registryHint')}
          </DialogDescription>
        </DialogHeader>
        {isGit && git.isPending && (
          <p role="status" className="text-sm text-muted-foreground">{t('customNodes.inspecting')}</p>
        )}
        {isGit && git.isError && (
          <Alert variant="destructive">
            <AlertTitle>{t('customNodes.inspectionError')}</AlertTitle>
            <AlertDescription>
              {errorMessage(git.error)}{' '}
              {git.error instanceof IpcClientError && failureKind(git.error.code) === 'git_unavailable'
                ? t(FAILURE_GUIDANCE.git_unavailable)
                : ''}
              <Button variant="outline" size="sm" onClick={() => void git.refetch()}>{t('customNodes.inspectAgain')}</Button>
            </AlertDescription>
          </Alert>
        )}
        {git.data && <GitStatusView status={git.data} />}
        {check && check.refusals.length > 0 && (
          <Alert variant="destructive">
            <AlertTitle>{t('customNodes.refusedTitle')}</AlertTitle>
            <AlertDescription>
              <ul>{check.refusals.map((reason) => <li key={reason}>{reason}</li>)}</ul>
              {t('customNodes.resolveHint')}
              <Button variant="outline" size="sm" onClick={() => void git.refetch()}>{t('customNodes.inspectAgain')}</Button>
            </AlertDescription>
          </Alert>
        )}
        {check?.needsReference && check.refusals.length === 0 && (
          <div className="flex flex-col gap-1.5">
            <Label htmlFor={`${prefix}-reference`}>{t('customNodes.targetRequired')}</Label>
            <Input
              id={`${prefix}-reference`}
              className="font-mono"
              value={reference}
              onChange={(event) => setReference(event.target.value)}
            />
            <p className="text-xs text-muted-foreground">
              {t('customNodes.targetHint')}
            </p>
          </div>
        )}
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>{t('common.cancel')}</Button>
          <Button disabled={blocked} onClick={() => onPrepare(target || null)}>{t('customNodes.prepareUpdate')}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
