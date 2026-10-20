import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { TriangleAlert } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'

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

function describeTarget(input: UpdateInput): string {
  switch (input.target.kind) {
    case 'latest_release':
      return 'the latest published release'
    case 'latest_commit':
      return 'the latest commit on origin’s default branch'
    case 'tag':
      return `release ${input.target.name}`
    case 'branch_ff_only':
      return 'the upstream of the current branch (fast-forward only)'
  }
}

export function VersionsPanel() {
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
      toast.success('ComfyUI was updated and its environment verified.')
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
          Version
        </CardTitle>
        <CardDescription>
          Updates fetch first, refuse unsafe checkouts, and reinstall
          requirements while protecting the pinned packages.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        {data ? (
          <>
            <p className="font-mono text-xs">
              Current: {describeRevision(data.current)}
            </p>
            {data.pendingUpdate ? (
              <Alert variant="destructive">
                <TriangleAlert />
                <AlertTitle>An update was interrupted</AlertTitle>
                <AlertDescription>
                  <p>
                    The update to{' '}
                    {data.pendingUpdate.targetTag ??
                      data.pendingUpdate.targetCommit.slice(0, 8)}{' '}
                    stopped during &quot;{data.pendingUpdate.phase}&quot;
                    {data.pendingUpdate.failure
                      ? `: ${data.pendingUpdate.failure}`
                      : ''}
                    . ComfyUI cannot start until it is retried.
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
                    Retry update
                  </Button>
                </AlertDescription>
              </Alert>
            ) : null}
            {refusal ? (
              <Alert variant="destructive">
                <TriangleAlert />
                <AlertTitle>Update refused: {refusal.reason}</AlertTitle>
                <AlertDescription>
                  <p>Nothing was changed. Git reports:</p>
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
                Update to latest commit (git pull)
              </Button>
              <Button
                disabled={update.isPending}
                onClick={() => ask({ kind: 'latest_release' })}
              >
                Update to latest release
              </Button>
              {data.current.branch ? (
                <Button
                  variant="outline"
                  disabled={update.isPending}
                  onClick={() => ask({ kind: 'branch_ff_only' })}
                >
                  Fast-forward {data.current.branch}
                </Button>
              ) : null}
            </div>
            <p className="text-xs text-muted-foreground">
              Switches to origin’s default branch, normally master. Development
              code may be less stable than a release.
            </p>
            {data.tags.length > 0 ? (
              <div className="flex flex-wrap items-end gap-3">
                <div className="flex min-w-48 flex-col gap-1.5">
                  <Label htmlFor="comfy-tag">Release tag</Label>
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
                  Switch to {tag || 'tag'}
                </Button>
              </div>
            ) : null}
            <p className="text-xs text-muted-foreground">
              Switching back to an older tag changes the source only; it does
              not restore the previous package set.
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
            <DialogTitle>Update ComfyUI</DialogTitle>
            <DialogDescription>
              {request
                ? `Move ComfyUI to ${describeTarget(request)}. Its requirements are reinstalled under the protected package pins and the environment is verified again.`
                : ''}
              {request?.stopRunning || running
                ? ' ComfyUI is running and will be stopped first.'
                : ''}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button
              variant="outline"
              disabled={update.isPending}
              onClick={() => setRequest(null)}
            >
              Cancel
            </Button>
            <Button
              disabled={update.isPending || request === null}
              onClick={() => {
                if (request)
                  update.mutate({ ...request, stopRunning: running || request.stopRunning })
              }}
            >
              {update.isPending
                ? 'Updating...'
                : running
                  ? 'Stop ComfyUI and update'
                  : 'Update'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </Card>
  )
}
