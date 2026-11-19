import type { ReactNode } from 'react'

import { useTranslation } from '@/shared/i18n'
import { IpcClientError } from '@/shared/ipc/gateway'
import { cn } from '@/shared/lib/utils'
import { Badge } from '@/shared/ui/badge'
import { Card, CardContent, CardHeader, CardTitle } from '@/shared/ui/card'

import { useAppInfo } from './queries'

function Row({
  label,
  mono = false,
  children,
}: {
  label: string
  mono?: boolean
  children: ReactNode
}) {
  return (
    <div className="flex flex-col gap-1 sm:flex-row sm:items-baseline sm:gap-4">
      <dt className="w-40 shrink-0 type-label text-muted-foreground">{label}</dt>
      <dd
        className={cn(
          'text-sm [overflow-wrap:anywhere] break-words tabular-nums',
          mono && 'font-mono text-xs',
        )}
      >
        {children}
      </dd>
    </div>
  )
}

export function OverviewPage() {
  const { t } = useTranslation()
  const info = useAppInfo()

  return (
    <div className="flex flex-col gap-6">
      <header className="flex flex-col gap-1">
        <h1 className="type-page-title">{t('overview.title')}</h1>
        <p className="text-[0.9375rem] leading-[1.65] text-muted-foreground">
          {t('overview.description')}
        </p>
      </header>
      <Card>
        <CardHeader>
          <CardTitle>{t('overview.application')}</CardTitle>
        </CardHeader>
        <CardContent>
          {info.data ? (
            <dl className="flex flex-col gap-3">
              <Row label={t('common.version')} mono>
                {info.data.version}
              </Row>
              <Row label={t('overview.dataDirectory')} mono>
                {info.data.dataDir}
              </Row>
              <Row label={t('overview.credentials')}>
                <Badge variant="muted" className="type-tag">
                  {info.data.credentialBackend === 'keyring'
                    ? t('overview.keyring')
                    : t('overview.session')}
                </Badge>
              </Row>
            </dl>
          ) : info.isError ? (
            <p className="text-sm text-destructive-ink">
              {info.error instanceof IpcClientError
                ? info.error.message
                : t('common.somethingWrong')}
            </p>
          ) : (
            <p className="text-sm text-muted-foreground">{t('common.loading')}</p>
          )}
        </CardContent>
      </Card>
    </div>
  )
}
