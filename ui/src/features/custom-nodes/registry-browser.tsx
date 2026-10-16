import { Archive, ExternalLink, LoaderCircle, RefreshCw, Search, Star, TriangleAlert } from 'lucide-react'
import { toast } from 'sonner'

import { gateway, IpcClientError, type RegistryNodeDto } from '@/shared/ipc/gateway'
import { errorMessage } from '@/shared/lib/display'
import { Alert, AlertDescription, AlertTitle } from '@/shared/ui/alert'
import { Button } from '@/shared/ui/button'
import { EmptyState } from '@/shared/ui/empty-state'
import { Input } from '@/shared/ui/input'

import { formatCount, formatFetched, formatList, unknownText } from './model'
import { Chip, SupportChip } from './status-chips'
import { REGISTRY_LIMITS, useRegistry, useRegistryControls } from './use-registry'

const SELECT_CLASS =
  'h-9 rounded-md border border-input bg-card px-2.5 text-sm outline-none focus-visible:border-brand-cyan focus-visible:ring-2 focus-visible:ring-brand-cyan/35'

async function openRepository(repository: string) {
  try {
    await gateway.nodeOpenRepository(repository)
  } catch (error) {
    toast.error(errorMessage(error))
  }
}

/** Remote text is rendered as plain text only. */
function RegistryRow({ node, installDisabledReason, onInstall }: {
  node: RegistryNodeDto
  installDisabledReason: string | null
  onInstall: (node: RegistryNodeDto) => void
}) {
  return (
    <li className="flex flex-col gap-3 rounded-xl border bg-card p-4 transition-colors hover:border-brand-cyan">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <h3 className="text-base font-bold tracking-tight break-words [overflow-wrap:anywhere]">
            {node.name}
          </h3>
          <p className="font-mono text-xs text-muted-foreground break-words [overflow-wrap:anywhere]">
            {node.id} · {unknownText(node.publisher)}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          {node.repository !== null && (
            <Button
              variant="outline"
              size="sm"
              aria-label={`Open the repository of ${node.name}`}
              onClick={() => void openRepository(node.repository ?? '')}
            >
              Repository
              <ExternalLink aria-hidden="true" />
            </Button>
          )}
          <Button
            size="sm"
            disabled={installDisabledReason !== null}
            title={installDisabledReason ?? undefined}
            aria-label={`Install ${node.name}`}
            onClick={() => onInstall(node)}
          >
            Install
          </Button>
        </div>
      </div>
      <p className="text-sm leading-relaxed break-words [overflow-wrap:anywhere]">
        {node.description === null || node.description.trim() === ''
          ? <span className="text-muted-foreground">No description</span>
          : node.description}
      </p>
      <dl className="grid grid-cols-2 gap-x-4 gap-y-1 font-mono text-xs sm:grid-cols-4">
        <div>
          <dt className="text-muted-foreground">Downloads</dt>
          <dd>{formatCount(node.downloads)}</dd>
        </div>
        <div>
          <dt className="text-muted-foreground">GitHub stars</dt>
          <dd className="flex items-center gap-1">
            <Star className="size-3" aria-hidden="true" />
            {formatCount(node.githubStars)}
          </dd>
        </div>
        <div>
          <dt className="text-muted-foreground">Latest version</dt>
          <dd>{unknownText(node.latestVersion)}</dd>
        </div>
        <div>
          <dt className="text-muted-foreground">Registry status</dt>
          <dd className="break-words [overflow-wrap:anywhere]">{unknownText(node.status)}</dd>
        </div>
      </dl>
      <div className="flex flex-wrap gap-2">
        <SupportChip name="OS" value={node.os} />
        <SupportChip name="Accelerator" value={node.accelerator} />
        <SupportChip name="ComfyUI" value={node.comfyui} />
        {node.flagged && <Chip icon={TriangleAlert} className="bg-brand-pink">Flagged</Chip>}
        {node.deprecated && <Chip icon={Archive} className="bg-brand-pink">Deprecated</Chip>}
      </div>
      <dl className="grid gap-x-4 gap-y-1 font-mono text-xs sm:grid-cols-3">
        <div>
          <dt className="text-muted-foreground">Supported OS</dt>
          <dd>{formatList(node.supportedOs)}</dd>
        </div>
        <div>
          <dt className="text-muted-foreground">Supported accelerators</dt>
          <dd>{formatList(node.supportedAccelerators)}</dd>
        </div>
        <div>
          <dt className="text-muted-foreground">Supported ComfyUI</dt>
          <dd className="break-words [overflow-wrap:anywhere]">{unknownText(node.supportedComfyuiVersion)}</dd>
        </div>
      </dl>
      {node.warnings.length > 0 && (
        <ul className="list-disc pl-5 text-xs break-words [overflow-wrap:anywhere]">
          {node.warnings.map((warning, index) => <li key={index}>{warning}</li>)}
        </ul>
      )}
    </li>
  )
}

function loadFailure(error: unknown) {
  if (error instanceof IpcClientError && error.code === 'registry_refused') {
    return { title: 'The registry refused the request', body: error.message }
  }
  if (error instanceof IpcClientError && error.code === 'registry_offline') {
    return {
      title: 'The registry could not be reached',
      body: `${error.message} No cached results exist for this query.`,
    }
  }
  return { title: 'Registry results could not be loaded', body: errorMessage(error) }
}

export function RegistryBrowser({ installDisabledReason, onInstall }: {
  /** Why installs are unavailable (repair mode, busy work), or null. */
  installDisabledReason: string | null
  onInstall: (node: RegistryNodeDto) => void
}) {
  const controls = useRegistryControls()
  const registry = useRegistry(controls.query)
  const data = registry.data
  // Results of an earlier query are visible while the new one loads, but never installable.
  const reason = registry.isPlaceholderData
    ? 'Loading the new results'
    : installDisabledReason
  const failure = registry.error ? loadFailure(registry.error) : null
  const refreshFailure = registry.refreshError ? loadFailure(registry.refreshError) : null
  return (
    <section aria-label="Comfy Registry" className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center gap-2">
        <div className="relative min-w-56 flex-1">
          <Search
            className="pointer-events-none absolute top-2.5 left-2.5 size-4 text-muted-foreground"
            aria-hidden="true"
          />
          <Input
            aria-label="Search the Comfy Registry"
            placeholder="Search by name, publisher or description"
            className="pl-8"
            value={controls.draft}
            onChange={(event) => controls.setDraft(event.target.value)}
          />
        </div>
        <select
          aria-label="Results per page"
          className={SELECT_CLASS}
          value={controls.query.limit}
          onChange={(event) => controls.setLimit(Number(event.target.value))}
        >
          {REGISTRY_LIMITS.map((limit) => (
            <option key={limit} value={limit}>{limit} per page</option>
          ))}
        </select>
        <Button variant="outline" disabled={registry.refreshing} onClick={registry.refresh}>
          {registry.refreshing
            ? <LoaderCircle className="animate-spin" aria-hidden="true" />
            : <RefreshCw aria-hidden="true" />}
          Refresh
        </Button>
      </div>
      {data && (
        <p className="font-mono text-xs text-muted-foreground">
          Fetched {formatFetched(data.fetchedAt)} · {formatCount(data.total)} nodes
          {registry.isFetching && ' · updating…'}
        </p>
      )}
      {data?.stale && (
        <Alert>
          <TriangleAlert aria-hidden="true" />
          <AlertTitle>Showing cached registry results</AlertTitle>
          <AlertDescription>
            The registry could not be reached, so these results may be out of date.
          </AlertDescription>
        </Alert>
      )}
      {refreshFailure && (
        <Alert variant="destructive">
          <TriangleAlert aria-hidden="true" />
          <AlertTitle>Refresh failed: {refreshFailure.title}</AlertTitle>
          <AlertDescription>
            {refreshFailure.body} The results below were not refreshed.
          </AlertDescription>
        </Alert>
      )}
      {failure && (
        <Alert variant="destructive">
          <TriangleAlert aria-hidden="true" />
          <AlertTitle>{failure.title}</AlertTitle>
          <AlertDescription>
            {failure.body}
            <Button variant="outline" size="sm" onClick={registry.retry}>Try again</Button>
          </AlertDescription>
        </Alert>
      )}
      {registry.isPending && !failure ? (
        <p role="status" className="text-sm text-muted-foreground">Loading the Comfy Registry…</p>
      ) : data && data.nodes.length === 0 ? (
        <EmptyState title="No registry nodes match" description="Change the search text." />
      ) : data ? (
        <ul aria-label="Registry nodes" className="flex flex-col gap-3">
          {data.nodes.map((node) => (
            <RegistryRow key={node.id} node={node} installDisabledReason={reason} onInstall={onInstall} />
          ))}
        </ul>
      ) : null}
      {data && (
        <nav aria-label="Registry pages" className="flex items-center justify-between gap-2">
          <Button
            variant="outline"
            size="sm"
            disabled={registry.isPlaceholderData || controls.query.page <= 1}
            onClick={() => controls.setPage(controls.query.page - 1)}
          >
            Previous page
          </Button>
          <span className="font-mono text-xs text-muted-foreground">
            Page {data.page} of {Math.max(1, data.totalPages)}
          </span>
          <Button
            variant="outline"
            size="sm"
            disabled={registry.isPlaceholderData || data.page >= data.totalPages}
            onClick={() => controls.setPage(controls.query.page + 1)}
          >
            Next page
          </Button>
        </nav>
      )}
    </section>
  )
}
