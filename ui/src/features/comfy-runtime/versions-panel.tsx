import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { TriangleAlert } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'

import { t as translate, useTranslation } from '@/shared/i18n'
import {
  gateway,
  type RevisionDto,
  type UpdateInput,
  type UpdateResultDto,
} from '@/shared/ipc/gateway'
import { errorMessage } from '@/shared/lib/display'
import { environmentKeys } from '@/shared/lib/environment-queries'
import { Alert, AlertDescription, AlertTitle } from '@/shared/ui/alert'
import { Button } from '@/shared/ui/button'
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/shared/ui/card'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shared/ui/dialog'
import { ErrorNotice } from '@/shared/ui/feedback'
import { Label } from '@/shared/ui/label'
import { Skeleton } from '@/shared/ui/skeleton'

import { NativeSelect } from './native-select'
import { comfyKeys, useRuntime } from './use-runtime'

export function describeRevision(revision: RevisionDto): string {
  const short = revision.commit.slice(0, 8)
  if (revision.tag) return `${revision.tag} (${short})`
  if (revision.branch) return `${revision.branch} (${short})`
  return short
}

function describeTarget(input: UpdateInput, stopRunning: boolean): string {
  switch (input.target.kind) {
    case 'latest_release':
      return translate(stopRunning ? 'runtime.versions.confirm.latestReleaseStop' : 'runtime.versions.confirm.latestRelease')
    case 'latest_commit':
      return translate(stopRunning ? 'runtime.versions.confirm.latestCommitStop' : 'runtime.versions.confirm.latestCommit')
    case 'tag':
      return translate(stopRunning ? 'runtime.versions.confirm.tagStop' : 'runtime.versions.confirm.tag', { name: input.target.name })
    case 'branch_ff_only':
      return translate(stopRunning ? 'runtime.versions.confirm.upstreamStop' : 'runtime.versions.confirm.upstream')
  }
}

export function VersionsPanel() {
  const { t } = useTranslation()
  const client = useQueryClient()
  const runtime = useRuntime()
  const versions = useQuery({
    queryKey: comfyKeys.versions,
    queryFn: () => gateway.comfyListVersions(),
  })
  const [selectedTag, setSelectedTag] = useState('')
  const [request, setRequest] = useState<UpdateInput | null>(null)
  const [refusal, setRefusal] = useState<{
    reason: string
    status: string
  } | null>(null)
  const update = useMutation({
    mutationFn: (input: UpdateInput) => gateway.comfyUpdate(input),
    onSuccess: (result: UpdateResultDto, input) => {
      if (result.kind === 'needs_stop') {
        // Keep the dialog open, now asking for explicit authorization to stop ComfyUI.
        setRequest({ ...input, stopRunning: true })
        return
      }
      setRequest(null)
      if (result.kind === 'refused') {
        setRefusal({ reason: result.reason, status: result.status })
        return
      }
      setRefusal(null)
      toast.success(translate('runtime.versions.updated'))
    },
    onError: (error) => {
      setRequest(null)
      toast.error(errorMessage(error))
    },
    onSettled: () => {
      void client.invalidateQueries({ queryKey: comfyKeys.versions })
      void client.invalidateQueries({ queryKey: environmentKeys.info })
    },
  })
  const running =
    runtime.status?.lifecycle === 'running' ||
    runtime.status?.lifecycle === 'starting'
  const data = versions.data
  const tag = selectedTag || data?.tags[0] || ''
  const ask = (target: UpdateInput['target']) => {
    setRefusal(null)
    setRequest({ target, stopRunning: false, retry: null })
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-lg font-bold tracking-tight">
          {t('common.version')}
        </CardTitle>
        <CardDescription>
          {t('runtime.versions.hint')}
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        {data ? (
          <>
            <p className="font-mono text-xs">
              {t('runtime.versions.current', { revision: describeRevision(data.current) })}
            </p>
            {data.pendingUpdate ? (
              <Alert variant="destructive">
                <TriangleAlert />
                <AlertTitle>{t('runtime.versions.interrupted')}</AlertTitle>
                <AlertDescription>
                  <p>
                    {t('runtime.versions.interruptedHint', { target: data.pendingUpdate.targetTag ?? data.pendingUpdate.targetCommit.slice(0, 8), phase: data.pendingUpdate.phase, failure: data.pendingUpdate.failure ? `: ${data.pendingUpdate.failure}` : '' })}
                  </p>
                  <Button
                    variant="outline"
                    size="sm"
                    disabled={update.isPending}
                    onClick={() =>
                      update.mutate({
                        target: { kind: 'latest_release' },
                        stopRunning: false,
                        retry: data.pendingUpdate?.id ?? null,
                      })
                    }
                  >
                    {t('runtime.versions.retry')}
                  </Button>
                </AlertDescription>
              </Alert>
            ) : null}
            {refusal ? (
              <Alert variant="destructive">
                <TriangleAlert />
                <AlertTitle>{t('runtime.versions.refused', { reason: refusal.reason })}</AlertTitle>
                <AlertDescription>
                  <p>{t('runtime.versions.unchanged')}</p>
                  <pre className="max-h-48 overflow-auto rounded-md bg-muted p-2 font-mono text-xs whitespace-pre-wrap">
                    {refusal.status}
                  </pre>
                </AlertDescription>
              </Alert>
            ) : null}
            <div className="flex flex-wrap items-end gap-3">
              <Button
                variant="outline"
                disabled={update.isPending}
                onClick={() => ask({ kind: 'latest_commit' })}
              >
                {t('runtime.versions.updateCommit')}
              </Button>
              <Button
                disabled={update.isPending}
                onClick={() => ask({ kind: 'latest_release' })}
              >
                {t('runtime.versions.updateRelease')}
              </Button>
              {data.current.branch ? (
                <Button
                  variant="outline"
                  disabled={update.isPending}
                  onClick={() => ask({ kind: 'branch_ff_only' })}
                >
                  {t('runtime.versions.fastForward', { branch: data.current.branch })}
                </Button>
              ) : null}
            </div>
            <p className="text-xs text-muted-foreground">
              {t('runtime.versions.branchHint')}
            </p>
            {data.tags.length > 0 ? (
              <div className="flex flex-wrap items-end gap-3">
                <div className="flex min-w-48 flex-col gap-1.5">
                  <Label htmlFor="comfy-tag">{t('runtime.versions.releaseTag')}</Label>
                  <NativeSelect
                    id="comfy-tag"
                    value={tag}
                    options={data.tags.map((value) => ({
                      value,
                      label: value,
                    }))}
                    onChange={setSelectedTag}
                  />
                </div>
                <Button
                  variant="outline"
                  disabled={update.isPending || !tag}
                  onClick={() => ask({ kind: 'tag', name: tag })}
                >
                  {t('runtime.versions.switchTag', { tag: tag || 'tag' })}
                </Button>
              </div>
            ) : null}
            <p className="text-xs text-muted-foreground">
              {t('runtime.versions.olderHint')}
            </p>
          </>
        ) : versions.isError ? (
          <ErrorNotice
            error={versions.error}
            retry={() => void versions.refetch()}
          />
        ) : (
          <Skeleton className="h-24" />
        )}
      </CardContent>
      <Dialog
        open={request !== null}
        onOpenChange={(open) => {
          if (!open && !update.isPending) setRequest(null)
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('runtime.versions.update')}</DialogTitle>
            <DialogDescription>
              {request ? describeTarget(request, request.stopRunning || running) : ''}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button
              variant="outline"
              disabled={update.isPending}
              onClick={() => setRequest(null)}
            >
              {t('common.cancel')}
            </Button>
            <Button
              disabled={update.isPending || request === null}
              onClick={() => {
                if (request)
                  update.mutate({ ...request, stopRunning: running || request.stopRunning })
              }}
            >
              {update.isPending
                ? t('runtime.versions.updating')
                : running
                  ? t('runtime.versions.stopUpdate')
                  : t('common.update')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </Card>
  )
}
