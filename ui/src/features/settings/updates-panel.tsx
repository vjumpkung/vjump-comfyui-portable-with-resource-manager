import { useMutation, useQuery } from '@tanstack/react-query'
import { Download, RefreshCw } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'

import {
  gateway,
  type UpdateOfferDto,
  type UpdateProgressDto,
} from '@/shared/ipc/gateway'
import { Alert, AlertDescription, AlertTitle } from '@/shared/ui/alert'
import { Button } from '@/shared/ui/button'
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/shared/ui/card'
import { Progress } from '@/shared/ui/progress'

import { errorMessage } from './error-message'

const MIB = 1024 * 1024

const mib = (bytes: number | null) => ((bytes ?? 0) / MIB).toFixed(1)

function describeProgress(progress: UpdateProgressDto): string {
  switch (progress.kind) {
    case 'downloading':
      return progress.totalBytes
        ? `Downloading ${mib(progress.downloadedBytes)} of ${mib(progress.totalBytes)} MiB`
        : `Downloading ${mib(progress.downloadedBytes)} MiB`
    case 'installing':
      return 'Installing the update'
    case 'restarting':
      return 'Restarting the app'
    case 'failed':
      return 'The update failed'
  }
}

function percent(progress: UpdateProgressDto): number | null {
  if (progress.kind !== 'downloading' || !progress.totalBytes) return null
  return Math.min(100, ((progress.downloadedBytes ?? 0) / progress.totalBytes) * 100)
}

/** The reviewed release. Notes come from the update server and are shown as plain text. */
function Offer({
  offer,
  progress,
  installing,
  blocked,
  onInstall,
}: {
  offer: UpdateOfferDto
  progress: UpdateProgressDto | null
  installing: boolean
  blocked: boolean
  onInstall: () => void
}) {
  const value = progress ? percent(progress) : null
  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-col gap-1">
        <p className="text-sm font-semibold">Version {offer.version} is available</p>
        {offer.date && (
          <p className="text-xs text-muted-foreground">Released {offer.date}</p>
        )}
      </div>
      {offer.notes && (
        <pre className="max-h-60 overflow-auto rounded-md bg-muted p-3 text-xs whitespace-pre-wrap">
          {offer.notes}
        </pre>
      )}
      <p className="text-xs text-muted-foreground">
        ComfyUI and other maintenance work must be stopped. The app restarts
        after the update is installed.
      </p>
      {progress && (
        <div className="flex flex-col gap-2" aria-live="polite">
          <p className="text-sm">{describeProgress(progress)}</p>
          {value !== null && <Progress value={value} />}
        </div>
      )}
      <div>
        <Button disabled={installing || blocked} onClick={onInstall}>
          <Download aria-hidden="true" />
          Install and restart
        </Button>
      </div>
    </div>
  )
}

export function UpdatesPanel({
  maintenanceBusy = false,
}: {
  /** A setup run is in progress; installing is refused until it ends. */
  maintenanceBusy?: boolean
}) {
  const status = useQuery({
    queryKey: ['app-update-status'],
    queryFn: () => gateway.appUpdateStatus(),
  })
  const [progress, setProgress] = useState<UpdateProgressDto | null>(null)
  const check = useMutation({
    mutationFn: () => gateway.appUpdateCheck(),
    onMutate: () => setProgress(null),
    onError: (error) => toast.error(errorMessage(error)),
  })
  const install = useMutation({
    // The id echoed back is the one of the offer the user reviewed.
    mutationFn: (offer: UpdateOfferDto) =>
      gateway.appUpdateInstall(offer.updateId, setProgress),
    onError: () => {
      setProgress(null)
      // The installer consumed the reviewed offer; a new check starts a new review.
      check.reset()
    },
  })
  const configured = status.data?.configured ?? false
  const offer = check.data ?? null

  return (
    <Card>
      <CardHeader>
        <CardTitle>App updates</CardTitle>
        <CardDescription>
          {status.data
            ? `Installed version ${status.data.currentVersion}.`
            : 'Signed updates for this app.'}
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        {status.data && !configured && (
          <p className="text-sm text-muted-foreground">
            This build has no update source configured, so it cannot update
            itself.
          </p>
        )}
        {configured && (
          <>
            <div>
              <Button
                variant="outline"
                disabled={check.isPending || install.isPending}
                onClick={() => check.mutate()}
              >
                <RefreshCw aria-hidden="true" />
                Check for updates
              </Button>
            </div>
            {check.isSuccess && offer === null && (
              <p className="text-sm text-muted-foreground">
                You are up to date.
              </p>
            )}
            {offer && (
              <Offer
                offer={offer}
                progress={progress}
                installing={install.isPending}
                blocked={maintenanceBusy}
                onInstall={() => install.mutate(offer)}
              />
            )}
            {install.isError && (
              <Alert variant="destructive">
                <AlertTitle>The update was not installed</AlertTitle>
                <AlertDescription>{errorMessage(install.error)}</AlertDescription>
              </Alert>
            )}
          </>
        )}
      </CardContent>
    </Card>
  )
}
