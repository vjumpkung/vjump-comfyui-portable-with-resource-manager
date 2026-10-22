import { useMutation, useQuery } from '@tanstack/react-query'
import { Download, RefreshCw } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'

import { t as translate, useTranslation } from '@/shared/i18n'
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

function describeProgress(progress: UpdateProgressDto, t: typeof translate): string {
  switch (progress.kind) {
    case 'downloading':
      return progress.totalBytes
        ? t('settings.updates.downloadingTotal', { downloaded: mib(progress.downloadedBytes), total: mib(progress.totalBytes) })
        : t('settings.updates.downloading', { downloaded: mib(progress.downloadedBytes) })
    case 'installing':
      return t('settings.updates.installing')
    case 'restarting':
      return t('settings.updates.restarting')
    case 'failed':
      return t('settings.updates.failed')
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
  const { t } = useTranslation()
  const value = progress ? percent(progress) : null
  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-col gap-1">
        <p className="text-sm font-semibold">{t('settings.updates.available', { version: offer.version })}</p>
        {offer.date && (
          <p className="text-xs text-muted-foreground">{t('settings.updates.released', { date: offer.date })}</p>
        )}
      </div>
      {offer.notes && (
        <pre className="max-h-60 overflow-auto rounded-md bg-muted p-3 text-xs whitespace-pre-wrap">
          {offer.notes}
        </pre>
      )}
      <p className="text-xs text-muted-foreground">
        {t('settings.updates.installHint')}
      </p>
      {progress && (
        <div className="flex flex-col gap-2" aria-live="polite">
          <p className="text-sm">{describeProgress(progress, t)}</p>
          {value !== null && <Progress value={value} />}
        </div>
      )}
      <div>
        <Button disabled={installing || blocked} onClick={onInstall}>
          <Download aria-hidden="true" />
          {t('settings.updates.installRestart')}
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
  const { t } = useTranslation()
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
        <CardTitle>{t('settings.updates.title')}</CardTitle>
        <CardDescription>
          {status.data
            ? t('settings.updates.installed', { version: status.data.currentVersion })
            : t('settings.updates.signedHint')}
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        {status.data && !configured && (
          <p className="text-sm text-muted-foreground">
            {t('settings.updates.unconfigured')}
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
                {t('settings.updates.check')}
              </Button>
            </div>
            {check.isSuccess && offer === null && (
              <p className="text-sm text-muted-foreground">
                {t('settings.updates.upToDate')}
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
                <AlertTitle>{t('settings.updates.notInstalled')}</AlertTitle>
                <AlertDescription>{errorMessage(install.error)}</AlertDescription>
              </Alert>
            )}
          </>
        )}
      </CardContent>
    </Card>
  )
}
