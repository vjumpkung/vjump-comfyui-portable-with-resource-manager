import { useMutation, useQuery } from '@tanstack/react-query'
import { useState } from 'react'
import { Info, RefreshCw } from 'lucide-react'

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

const modes: { mode: SetupModeDto; title: string; description: string }[] = [
  {
    mode: 'repair',
    title: 'Repair',
    description: 'Verify first; build a replacement if needed.',
  },
  {
    mode: 'change_torch',
    title: 'Change torch / CUDA',
    description: 'Create a new venv using the same Python minor.',
  },
  {
    mode: 'change_python',
    title: 'Change Python',
    description: 'Choose a different Python minor in a new venv.',
  },
  {
    mode: 'switch_venv',
    title: 'Switch to custom venv',
    description: 'Validate a separate existing venv or empty folder.',
  },
]

export function EnvironmentPage({
  status,
  onSetup,
  onResume,
}: {
  status: SetupStatusDto
  onSetup: (mode: SetupModeDto) => void
  onResume: () => void
}) {
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
          Environment
        </h1>
        <p className="text-sm text-muted-foreground">
          Selected Python environment, protected packages and re-setup.
        </p>
      </header>
      {needsRepair && (
        <Alert variant="destructive">
          <AlertTitle>
            RepairRequired — selected environment is not runnable
          </AlertTitle>
          <AlertDescription>
            A dependency change or verification failure left this environment
            selected but unverified. Package rollback is unavailable. Run Repair
            before using ComfyUI.
            <Button onClick={() => onSetup('repair')} disabled={busy}>
              Repair environment
            </Button>
          </AlertDescription>
        </Alert>
      )}
      {status.run && ['failed', 'interrupted'].includes(status.run.status) && (
        <Alert>
          <AlertTitle>Setup needs attention</AlertTitle>
          <AlertDescription>
            Run {status.run.id} stopped before completion. The selected
            environment has not been replaced.
            <Button variant="outline" onClick={onResume}>
              Review setup failure
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
              Selected environment{' '}
              <StatusBadge
                status={needsRepair ? 'repair_required' : env.state}
              />
            </CardTitle>
            <CardDescription>
              {env.id} ·{' '}
              {env.ownership === 'app_owned'
                ? 'App-owned'
                : 'Adopted (never deleted)'}{' '}
              · {label(env.source)}
            </CardDescription>
          </CardHeader>
          <CardContent>
            <dl className="grid grid-cols-1 gap-3 text-sm sm:grid-cols-2">
              <div>
                <dt>Python</dt>
                <dd className="font-mono">{env.pythonVersion}</dd>
              </div>
              <div>
                <dt>Environment path</dt>
                <dd className="break-all font-mono">{env.path}</dd>
              </div>
              <div>
                <dt>Profile</dt>
                <dd>
                  {info.data?.profileLabel ?? env.profileId} · revision{' '}
                  {env.profileRevision}
                </dd>
              </div>
              <div>
                <dt>Torch / CUDA</dt>
                <dd className="font-mono">
                  {info.data?.torchVersion ?? 'Unknown'} /{' '}
                  {info.data?.cudaVariant ?? 'Unknown'}
                </dd>
              </div>
              <div>
                <dt>Triton</dt>
                <dd className="font-mono">
                  {info.data?.tritonVersion ?? 'Unknown'}
                </dd>
              </div>
              <div>
                <dt>Accelerators reported</dt>
                <dd>
                  {info.data
                    ? info.data.accelerators.join(', ') || 'None reported'
                    : 'Unknown'}
                </dd>
              </div>
              <div>
                <dt>ComfyUI reference</dt>
                <dd className="break-all font-mono">
                  {info.data?.comfyuiReference ?? 'Unknown'}
                </dd>
              </div>
              <div>
                <dt>ComfyUI commit</dt>
                <dd className="break-all font-mono">
                  {info.data?.comfyuiCommit ?? 'Unknown'}
                </dd>
              </div>
            </dl>
            <details>
              <summary>Protected pins and verification report</summary>
              <ul className="font-mono text-xs">
                {env.pins.map((pin) => (
                  <li key={pin.name}>
                    {pin.name}=={pin.version}
                  </li>
                ))}
              </ul>
              <pre className="max-h-64 overflow-auto whitespace-pre-wrap break-all font-mono text-xs">
                {env.verification ?? 'No verification report available.'}
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
              Refresh environment
            </Button>
          </CardContent>
        </Card>
      )}
      <Card>
        <CardHeader>
          <CardTitle>Installed packages</CardTitle>
          <CardDescription>
            Search package names and versions. Results are paginated to 100
            rows.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <FieldGroup>
            <Field>
              <FieldLabel htmlFor="package-search">Search packages</FieldLabel>
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
                    <TableHead>Package</TableHead>
                    <TableHead>Version</TableHead>
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
              {filtered.length === 0 && <p>No packages match your search.</p>}
              <div className="flex flex-wrap items-center gap-3">
                <Button
                  variant="outline"
                  size="sm"
                  disabled={currentPage === 0}
                  onClick={() => setPage(currentPage - 1)}
                >
                  Previous
                </Button>
                <span className="font-mono text-xs">
                  {filtered.length} packages · page {currentPage + 1} /{' '}
                  {pageCount}
                </span>
                <Button
                  variant="outline"
                  size="sm"
                  disabled={currentPage + 1 >= pageCount}
                  onClick={() => setPage(currentPage + 1)}
                >
                  Next
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
          <CardTitle>Re-setup</CardTitle>
          <CardDescription>
            All modes retain the previous environment until you explicitly
            delete it. ComfyUI must be stopped; M1 cannot stop externally
            started processes for you.
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
                  {item.title}
                </Button>
                <p className="text-sm text-muted-foreground">
                  {item.description}
                </p>
              </div>
            ))}
          </div>
        </CardContent>
      </Card>
      <Card>
        <CardHeader>
          <CardTitle>Delete an inactive environment</CardTitle>
          <CardDescription>
            Only an inactive, app-owned environment can be removed. The backend
            rejects selected, adopted, or in-use environments.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <Alert>
            <Info />
            <AlertTitle>Delete by recorded environment ID</AlertTitle>
            <AlertDescription>
              The M1 API exposes the selected environment and latest run, but
              does not list retained environments. Use the ID recorded in an
              earlier setup. Review the exact ID below before deletion.
            </AlertDescription>
          </Alert>
          <FieldGroup>
            <Field>
              <FieldLabel htmlFor="delete-env-id">
                Inactive app-owned environment ID
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
                Selected environment {status.activeEnv?.id} cannot be deleted.
                Removal deletes the venv folder; user data and the selected
                environment are preserved.
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
            Review environment deletion
          </Button>
          {deletion.isError && <ErrorNotice error={deletion.error} />}
          {deletion.isSuccess && (
            <p role="status">Inactive app-owned environment deleted.</p>
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
            <DialogTitle>Delete environment {deleteId.trim()}?</DialogTitle>
            <DialogDescription>
              This removes the inactive venv folder and its packages. Adopted
              and selected environments are protected by the backend. Type the
              exact ID to confirm.
            </DialogDescription>
          </DialogHeader>
          <FieldGroup>
            <Field>
              <FieldLabel htmlFor="confirm-delete-id">
                Confirm environment ID
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
              Cancel
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
              {deletion.isPending ? 'Deleting…' : 'Delete environment'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
