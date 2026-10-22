import { useMutation, useQuery } from '@tanstack/react-query'
import { useState } from 'react'
import { Info, RefreshCw } from 'lucide-react'

import { useTranslation } from '@/shared/i18n'
import {
  gateway,
  type SetupModeDto,
  type SetupStatusDto,
} from '@/shared/ipc/gateway'
import { label } from '@/shared/lib/display'
import { environmentKeys } from '@/shared/lib/environment-queries'
import { Alert, AlertTitle, AlertDescription } from '@/shared/ui/alert'
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
import { EmptyState } from '@/shared/ui/empty-state'
import {
  Field,
  FieldDescription,
  FieldGroup,
  FieldLabel,
} from '@/shared/ui/field'
import { Input } from '@/shared/ui/input'
import { Skeleton } from '@/shared/ui/skeleton'
import { StatusBadge } from '@/shared/ui/status-badge'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/shared/ui/table'
import { PackageChanges } from './package-changes'

const modes = [
  {
    mode: 'repair',
    title: 'common.repair',
    description: 'environment.mode.repairHint',
  },
  {
    mode: 'change_torch',
    title: 'environment.mode.torch',
    description: 'environment.mode.torchHint',
  },
  {
    mode: 'change_python',
    title: 'environment.mode.python',
    description: 'environment.mode.pythonHint',
  },
  {
    mode: 'switch_venv',
    title: 'environment.mode.venv',
    description: 'environment.mode.venvHint',
  },
] as const satisfies readonly { mode: SetupModeDto; title: keyof typeof import('@/shared/i18n/translations.json').en; description: keyof typeof import('@/shared/i18n/translations.json').en }[]

const SOURCE_KEYS = {
  managed: 'environment.source.managed',
  custom_venv: 'environment.source.customVenv',
  custom_interpreter: 'environment.source.customInterpreter',
} as const

const INSTALL_STATUS_KEYS = {
  installed: 'status.installed',
  failed: 'status.failed',
  skipped: 'status.skipped',
} as const

export function EnvironmentPage({
  status,
  onSetup,
  onResume,
}: {
  status: SetupStatusDto
  onSetup: (mode: SetupModeDto) => void
  onResume: () => void
}) {
  const { t } = useTranslation()
  const info = useQuery({
    queryKey: [...environmentKeys.info, status.activeEnv?.id],
    queryFn: gateway.envInfo,
    enabled: !!status.activeEnv,
  })
  const packages = useQuery({
    queryKey: [...environmentKeys.packages, status.activeEnv?.id],
    queryFn: gateway.envPackages,
    enabled: !!status.activeEnv,
  })
  const [search, setSearch] = useState('')
  const [page, setPage] = useState(0)
  const [deleteId, setDeleteId] = useState('')
  const [deleteOpen, setDeleteOpen] = useState(false)
  const [confirmedId, setConfirmedId] = useState('')
  const deletion = useMutation({
    mutationFn: gateway.envDelete,
    onSuccess: () => {
      setDeleteOpen(false)
      setDeleteId('')
      setConfirmedId('')
    },
  })
  const busy = status.phase === 'run_in_progress'
  const env = info.data?.env ?? status.activeEnv
  const needsRepair =
    env?.state === 'repair_required' ||
    status.activeEnv?.state === 'repair_required'
  const filtered = (packages.data ?? []).filter((pkg) =>
    `${pkg.name} ${pkg.version}`.toLowerCase().includes(search.toLowerCase()),
  )
  const pageCount = Math.max(1, Math.ceil(filtered.length / 100))
  const currentPage = Math.min(page, pageCount - 1)
  const visible = filtered.slice(currentPage * 100, (currentPage + 1) * 100)
  const canDelete =
    /^[a-z0-9][a-z0-9-]{0,63}$/.test(deleteId.trim()) &&
    deleteId.trim() !== status.activeEnv?.id &&
    !busy
  return (
    <div className="flex flex-col gap-6">
      <header>
        <h1 className="text-2xl font-extrabold tracking-tighter sm:text-3xl">
          {t('environment.title')}
        </h1>
        <p className="text-sm text-muted-foreground">
          {t('environment.description')}
        </p>
      </header>
      {needsRepair && (
        <Alert variant="destructive">
          <AlertTitle>
            {t('environment.repair.title')}
          </AlertTitle>
          <AlertDescription>
            {t('environment.repair.hint')}
            <Button onClick={() => onSetup('repair')} disabled={busy}>
              {t('environment.repair.action')}
            </Button>
          </AlertDescription>
        </Alert>
      )}
      {status.run && ['failed', 'interrupted'].includes(status.run.status) && (
        <Alert>
          <AlertTitle>{t('environment.setupAttention')}</AlertTitle>
          <AlertDescription>
            {t('environment.runStopped', { id: status.run.id })}
            <Button variant="outline" onClick={onResume}>
              {t('environment.reviewFailure')}
            </Button>
          </AlertDescription>
        </Alert>
      )}
      {info.isError && (
        <ErrorNotice
          error={info.error}
          retry={() => {
            void info.refetch()
          }}
        />
      )}
      {info.isPending && <Skeleton className="h-40" />}
      {env && (
        <Card>
          <CardHeader>
            <CardTitle>
              {t('environment.selected')}{' '}
              <StatusBadge
                status={needsRepair ? 'repair_required' : env.state}
              />
            </CardTitle>
            <CardDescription>
              {env.id} ·{' '}
              {env.ownership === 'app_owned'
                ? t('environment.appOwned')
                : t('environment.adopted')}{' '}
              · {Object.hasOwn(SOURCE_KEYS, env.source) ? t(SOURCE_KEYS[env.source]) : label(env.source)}
            </CardDescription>
          </CardHeader>
          <CardContent>
            <dl className="grid grid-cols-1 gap-3 text-sm sm:grid-cols-2">
              <div>
                <dt>Python</dt>
                <dd className="font-mono">{env.pythonVersion}</dd>
              </div>
              <div>
                <dt>{t('environment.path')}</dt>
                <dd className="break-all font-mono">{env.path}</dd>
              </div>
              <div>
                <dt>{t('environment.profile')}</dt>
                <dd>
                  {t('environment.profileRevision', { profile: info.data?.profileLabel ?? env.profileId, revision: env.profileRevision })}
                </dd>
              </div>
              <div>
                <dt>Torch / CUDA</dt>
                <dd className="font-mono">
                  {info.data?.torchVersion ?? t('common.unknownValue')} /{' '}
                  {info.data?.cudaVariant ?? t('common.unknownValue')}
                </dd>
              </div>
              <div>
                <dt>Triton</dt>
                <dd className="font-mono">
                  {info.data?.tritonVersion ?? t('common.unknownValue')}
                </dd>
              </div>
              <div>
                <dt>{t('environment.acceleratorsReported')}</dt>
                <dd>
                  {info.data
                    ? info.data.accelerators.join(', ') || t('environment.noneReported')
                    : t('common.unknownValue')}
                </dd>
              </div>
              <div>
                <dt>{t('environment.comfyReference')}</dt>
                <dd className="break-all font-mono">
                  {info.data?.comfyuiReference ?? t('common.unknownValue')}
                </dd>
              </div>
              <div>
                <dt>{t('environment.comfyCommit')}</dt>
                <dd className="break-all font-mono">
                  {info.data?.comfyuiCommit ?? t('common.unknownValue')}
                </dd>
              </div>
            </dl>
            <details>
              <summary>{t('environment.verification')}</summary>
              <ul className="font-mono text-xs">
                {env.pins.map((pin) => (
                  <li key={pin.name}>
                    {pin.name}=={pin.version}
                  </li>
                ))}
              </ul>
              <pre className="max-h-64 overflow-auto whitespace-pre-wrap break-all font-mono text-xs">
                {env.verification ?? t('environment.noVerification')}
              </pre>
            </details>
            <Button
              variant="outline"
              onClick={() => {
                void info.refetch()
                void packages.refetch()
              }}
            >
              <RefreshCw data-icon="inline-start" />
              {t('environment.refresh')}
            </Button>
          </CardContent>
        </Card>
      )}
      {info.data && (
        <Card>
          <CardHeader>
            <CardTitle>{t('environment.accelerators.title')}</CardTitle>
            <CardDescription>
              {t('environment.accelerators.hint')}
            </CardDescription>
          </CardHeader>
          <CardContent>
            {info.data.components.accelerators.length === 0 ? (
              <EmptyState
                title={t('environment.accelerators.empty')}
                description={t('environment.accelerators.emptyHint')}
              />
            ) : (
              <ul className="flex flex-col gap-4">
                {info.data.components.accelerators.map((result) => (
                  <li key={result.id} className="flex flex-col gap-2 text-sm">
                    <p className="break-all font-mono">{result.id}</p>
                    <dl className="grid gap-2 sm:grid-cols-3">
                      <div>
                        <dt>{t('common.selected')}</dt>
                        <dd>{result.selected ? t('common.yes') : t('common.no')}</dd>
                      </div>
                      <div>
                        <dt>{t('environment.accelerators.installStatus')}</dt>
                        <dd>{Object.hasOwn(INSTALL_STATUS_KEYS, result.status) ? t(INSTALL_STATUS_KEYS[result.status]) : label(result.status)}</dd>
                      </div>
                      <div>
                        <dt>{t('environment.accelerators.importResult')}</dt>
                        <dd>
                          {result.importable === null
                            ? t('environment.accelerators.untested')
                            : result.importable ? t('common.passed') : t('common.failed')}
                        </dd>
                      </div>
                    </dl>
                    {result.message && <p className="break-words">{result.message}</p>}
                  </li>
                ))}
              </ul>
            )}
          </CardContent>
        </Card>
      )}
      <Card>
        <CardHeader>
          <CardTitle>{t('environment.packages.title')}</CardTitle>
          <CardDescription>
            {t('environment.packages.hint')}
          </CardDescription>
        </CardHeader>
        <CardContent>
          <FieldGroup>
            <Field>
              <FieldLabel htmlFor="package-search">{t('environment.packages.search')}</FieldLabel>
              <Input
                id="package-search"
                value={search}
                onChange={(event) => {
                  setSearch(event.target.value)
                  setPage(0)
                }}
              />
            </Field>
          </FieldGroup>
          {packages.isPending && <Skeleton className="h-40" />}
          {packages.isError && (
            <ErrorNotice
              error={packages.error}
              retry={() => {
                void packages.refetch()
              }}
            />
          )}
          {packages.data && (
            <>
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>{t('environment.packages.package')}</TableHead>
                    <TableHead>{t('common.version')}</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {visible.map((pkg) => (
                    <TableRow key={pkg.name}>
                      <TableCell className="break-all font-mono">
                        {pkg.name}
                      </TableCell>
                      <TableCell className="font-mono">{pkg.version}</TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
              {filtered.length === 0 && <p>{t('environment.packages.noMatch')}</p>}
              <div className="flex flex-wrap items-center gap-3">
                <Button
                  variant="outline"
                  size="sm"
                  disabled={currentPage === 0}
                  onClick={() => setPage(currentPage - 1)}
                >
                  {t('common.previous')}
                </Button>
                <span className="font-mono text-xs">
                  {t('environment.packages.pagination', { count: filtered.length, page: currentPage + 1, pages: pageCount })}
                </span>
                <Button
                  variant="outline"
                  size="sm"
                  disabled={currentPage + 1 >= pageCount}
                  onClick={() => setPage(currentPage + 1)}
                >
                  {t('common.next')}
                </Button>
              </div>
            </>
          )}
        </CardContent>
      </Card>
      {env && (
        <PackageChanges key={env.id} envId={env.id} maintenanceBusy={busy} />
      )}
      <Card>
        <CardHeader>
          <CardTitle>{t('environment.reSetup')}</CardTitle>
          <CardDescription>
            {t('environment.reSetupHint')}
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid gap-3 sm:grid-cols-2">
            {modes.map((item) => (
              <div key={item.mode} className="flex flex-col items-start gap-2">
                <Button
                  variant="outline"
                  disabled={busy}
                  onClick={() => onSetup(item.mode)}
                >
                  {t(item.title)}
                </Button>
                <p className="text-sm text-muted-foreground">
                  {t(item.description)}
                </p>
              </div>
            ))}
          </div>
        </CardContent>
      </Card>
      <Card>
        <CardHeader>
          <CardTitle>{t('environment.delete.title')}</CardTitle>
          <CardDescription>
            {t('environment.delete.hint')}
          </CardDescription>
        </CardHeader>
        <CardContent>
          <Alert>
            <Info />
            <AlertTitle>{t('environment.delete.byId')}</AlertTitle>
            <AlertDescription>
              {t('environment.delete.idHint')}
            </AlertDescription>
          </Alert>
          <FieldGroup>
            <Field>
              <FieldLabel htmlFor="delete-env-id">
                {t('environment.delete.id')}
              </FieldLabel>
              <Input
                id="delete-env-id"
                className="font-mono"
                value={deleteId}
                disabled={deletion.isPending}
                onChange={(event) => {
                  setDeleteId(event.target.value)
                  deletion.reset()
                }}
                placeholder="env-…"
              />
              <FieldDescription>
                {t('environment.delete.disclosure', { id: status.activeEnv?.id ?? '' })}
              </FieldDescription>
            </Field>
          </FieldGroup>
          <Button
            variant="destructive"
            disabled={!canDelete || deletion.isPending}
            onClick={() => {
              setConfirmedId('')
              setDeleteOpen(true)
            }}
          >
            {t('environment.delete.review')}
          </Button>
          {deletion.isError && <ErrorNotice error={deletion.error} />}
          {deletion.isSuccess && (
            <p role="status">{t('environment.delete.success')}</p>
          )}
        </CardContent>
      </Card>
      <Dialog
        open={deleteOpen}
        onOpenChange={(open) => {
          if (!deletion.isPending) setDeleteOpen(open)
        }}
      >
        <DialogContent showCloseButton={!deletion.isPending}>
          <DialogHeader>
            <DialogTitle>{t('environment.delete.confirmTitle', { id: deleteId.trim() })}</DialogTitle>
            <DialogDescription>
              {t('environment.delete.confirmHint')}
            </DialogDescription>
          </DialogHeader>
          <FieldGroup>
            <Field>
              <FieldLabel htmlFor="confirm-delete-id">
                {t('environment.delete.confirmId')}
              </FieldLabel>
              <Input
                id="confirm-delete-id"
                className="font-mono"
                value={confirmedId}
                disabled={deletion.isPending}
                onChange={(event) => setConfirmedId(event.target.value)}
              />
            </Field>
          </FieldGroup>
          {deletion.isError && <ErrorNotice error={deletion.error} />}
          <DialogFooter>
            <Button
              variant="outline"
              disabled={deletion.isPending}
              onClick={() => setDeleteOpen(false)}
            >
              {t('common.cancel')}
            </Button>
            <Button
              variant="destructive"
              disabled={
                !canDelete ||
                confirmedId !== deleteId.trim() ||
                deletion.isPending
              }
              onClick={() => deletion.mutate(deleteId.trim())}
            >
              {deletion.isPending ? t('environment.delete.deleting') : t('environment.delete.action')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
