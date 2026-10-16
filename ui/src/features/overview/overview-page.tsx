import type { ReactNode } from 'react'

import { IpcClientError } from '@/shared/ipc/gateway'
import { Badge } from '@/shared/ui/badge'
import { Card, CardContent, CardHeader, CardTitle } from '@/shared/ui/card'

import { useAppInfo } from './queries'

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-1 sm:flex-row sm:items-baseline sm:gap-4">
      <dt className="w-40 shrink-0 font-mono text-xs font-semibold uppercase tracking-wider text-muted-foreground">
        {label}
      </dt>
      <dd className="font-mono text-xs break-words [overflow-wrap:anywhere]">{children}</dd>
    </div>
  )
}

export function OverviewPage() {
  const info = useAppInfo()

  return (
    <div className="flex flex-col gap-6">
      <header className="flex flex-col gap-1">
        <h1 className="text-2xl font-extrabold tracking-tighter sm:text-3xl">Overview</h1>
        <p className="text-sm font-medium leading-relaxed text-muted-foreground">
          Manage ComfyUI, models and custom nodes from one place.
        </p>
      </header>
      <Card className="transition-all duration-300 hover:border-brand-cyan">
        <CardHeader>
          <CardTitle className="text-lg font-bold tracking-tight">Application</CardTitle>
        </CardHeader>
        <CardContent>
          {info.data ? (
            <dl className="flex flex-col gap-3">
              <Row label="Version">{info.data.version}</Row>
              <Row label="Data directory">{info.data.dataDir}</Row>
              <Row label="Credential store">
                <Badge variant="outline" className="font-mono text-[9px] uppercase tracking-widest">
                  {info.data.credentialBackend === 'keyring' ? 'OS keyring' : 'Session only'}
                </Badge>
              </Row>
            </dl>
          ) : info.isError ? (
            <p className="text-sm text-destructive">
              {info.error instanceof IpcClientError ? info.error.message : 'Something went wrong.'}
            </p>
          ) : (
            <p className="text-sm text-muted-foreground">Loading...</p>
          )}
        </CardContent>
      </Card>
    </div>
  )
}
