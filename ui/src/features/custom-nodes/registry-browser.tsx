import { Archive, ExternalLink, LoaderCircle, RefreshCw, Search, Star, TriangleAlert } from 'lucide-react'
import { toast } from 'sonner'

import { t as translate, useTranslation } from '@/shared/i18n'
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
  const { t } = useTranslation()
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
              aria-label={t('customNodes.registry.openRepositoryOf', { name: node.name })}
              onClick={() => void openRepository(node.repository ?? '')}
            >
              {t('customNodes.registry.repository')}
              <ExternalLink aria-hidden="true" />
            </Button>
          )}
          <Button
            size="sm"
            disabled={installDisabledReason !== null}
            title={installDisabledReason ?? undefined}
            aria-label={t('customNodes.installName', { name: node.name })}
            onClick={() => onInstall(node)}
          >
            {t('common.install')}
          </Button>
        </div>
      </div>
      <p className="text-sm leading-relaxed break-words [overflow-wrap:anywhere]">
        {node.description === null || node.description.trim() === ''
          ? <span className="text-muted-foreground">{t('customNodes.registry.noDescription')}</span>
          : node.description}
      </p>
      <dl className="grid grid-cols-2 gap-x-4 gap-y-1 font-mono text-xs sm:grid-cols-4">
        <div>
          <dt className="text-muted-foreground">{t('nav.downloads')}</dt>
          <dd>{formatCount(node.downloads)}</dd>
        </div>
        <div>
          <dt className="text-muted-foreground">{t('customNodes.registry.githubStars')}</dt>
          <dd className="flex items-center gap-1">
            <Star className="size-3" aria-hidden="true" />
            {formatCount(node.githubStars)}
          </dd>
        </div>
        <div>
          <dt className="text-muted-foreground">{t('customNodes.registry.latestVersion')}</dt>
          <dd>{unknownText(node.latestVersion)}</dd>
        </div>
        <div>
          <dt className="text-muted-foreground">{t('customNodes.registry.registryStatus')}</dt>
          <dd className="break-words [overflow-wrap:anywhere]">{unknownText(node.status)}</dd>
        </div>
      </dl>
      <div className="flex flex-wrap gap-2">
        <SupportChip name={t('customNodes.registry.os')} value={node.os} />
        <SupportChip name={t('customNodes.registry.accelerator')} value={node.accelerator} />
        <SupportChip name="ComfyUI" value={node.comfyui} />
        {node.flagged && <Chip icon={TriangleAlert} className="bg-brand-pink">{t('customNodes.registry.flagged')}</Chip>}
        {node.deprecated && <Chip icon={Archive} className="bg-brand-pink">{t('customNodes.registry.deprecated')}</Chip>}
      </div>
      <dl className="grid gap-x-4 gap-y-1 font-mono text-xs sm:grid-cols-3">
        <div>
          <dt className="text-muted-foreground">{t('customNodes.registry.supportedOs')}</dt>
          <dd>{formatList(node.supportedOs)}</dd>
        </div>
        <div>
          <dt className="text-muted-foreground">{t('customNodes.registry.supportedAccelerators')}</dt>
          <dd>{formatList(node.supportedAccelerators)}</dd>
        </div>
        <div>
          <dt className="text-muted-foreground">{t('customNodes.registry.supportedComfyui')}</dt>
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
    return { title: translate('customNodes.registry.refused'), body: error.message }
  }
  if (error instanceof IpcClientError && error.code === 'registry_offline') {
    return {
      title: translate('customNodes.registry.unreachable'),
      body: translate('customNodes.registry.noCache', { error: error.message }),
    }
  }
  return { title: translate('customNodes.registry.loadFailed'), body: errorMessage(error) }
}

export function RegistryBrowser({ installDisabledReason, onInstall }: {
  /** Why installs are unavailable (repair mode, busy work), or null. */
  installDisabledReason: string | null
  onInstall: (node: RegistryNodeDto) => void
}) {
  const { t } = useTranslation()
  const controls = useRegistryControls()
  const registry = useRegistry(controls.query)
  const data = registry.data
  // Results of an earlier query are visible while the new one loads, but never installable.
  const reason = registry.isPlaceholderData
    ? t('customNodes.registry.loadingNew')
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
            aria-label={t('customNodes.registry.search')}
            placeholder={t('customNodes.registry.searchHint')}
            className="pl-8"
            value={controls.draft}
            onChange={(event) => controls.setDraft(event.target.value)}
          />
        </div>
        <select
          aria-label={t('customNodes.registry.pageLimit')}
          className={SELECT_CLASS}
          value={controls.query.limit}
          onChange={(event) => controls.setLimit(Number(event.target.value))}
        >
          {REGISTRY_LIMITS.map((limit) => (
            <option key={limit} value={limit}>{t('customNodes.registry.perPage', { limit })}</option>
          ))}
        </select>
        <Button variant="outline" disabled={registry.refreshing} onClick={registry.refresh}>
          {registry.refreshing
            ? <LoaderCircle className="animate-spin" aria-hidden="true" />
            : <RefreshCw aria-hidden="true" />}
          {t('common.refresh')}
        </Button>
      </div>
      {data && (
        <p className="font-mono text-xs text-muted-foreground">
          {t(registry.isFetching ? 'customNodes.registry.fetchedUpdating' : 'customNodes.registry.fetched', { count: data.total ?? 0, date: formatFetched(data.fetchedAt), total: formatCount(data.total) })}
        </p>
      )}
      {data?.stale && (
        <Alert>
          <TriangleAlert aria-hidden="true" />
          <AlertTitle>{t('customNodes.registry.cachedTitle')}</AlertTitle>
          <AlertDescription>
            {t('customNodes.registry.cachedHint')}
          </AlertDescription>
        </Alert>
      )}
      {refreshFailure && (
        <Alert variant="destructive">
          <TriangleAlert aria-hidden="true" />
          <AlertTitle>{t('customNodes.registry.refreshFailed', { title: refreshFailure.title })}</AlertTitle>
          <AlertDescription>
            {t('customNodes.registry.notRefreshed', { error: refreshFailure.body })}
          </AlertDescription>
        </Alert>
      )}
      {failure && (
        <Alert variant="destructive">
          <TriangleAlert aria-hidden="true" />
          <AlertTitle>{failure.title}</AlertTitle>
          <AlertDescription>
            {failure.body}
            <Button variant="outline" size="sm" onClick={registry.retry}>{t('common.tryAgain')}</Button>
          </AlertDescription>
        </Alert>
      )}
      {registry.isPending && !failure ? (
        <p role="status" className="text-sm text-muted-foreground">{t('customNodes.registry.loading')}</p>
      ) : data && data.nodes.length === 0 ? (
        <EmptyState title={t('customNodes.registry.noMatch')} description={t('customNodes.registry.changeSearch')} />
      ) : data ? (
        <ul aria-label={t('customNodes.registry.nodes')} className="flex flex-col gap-3">
          {data.nodes.map((node) => (
            <RegistryRow key={node.id} node={node} installDisabledReason={reason} onInstall={onInstall} />
          ))}
        </ul>
      ) : null}
      {data && (
        <nav aria-label={t('customNodes.registry.pages')} className="flex items-center justify-between gap-2">
          <Button
            variant="outline"
            size="sm"
            disabled={registry.isPlaceholderData || controls.query.page <= 1}
            onClick={() => controls.setPage(controls.query.page - 1)}
          >
            {t('customNodes.registry.previousPage')}
          </Button>
          <span className="font-mono text-xs text-muted-foreground">
            {t('customNodes.registry.pageOf', { page: data.page, total: Math.max(1, data.totalPages) })}
          </span>
          <Button
            variant="outline"
            size="sm"
            disabled={registry.isPlaceholderData || data.page >= data.totalPages}
            onClick={() => controls.setPage(controls.query.page + 1)}
          >
            {t('customNodes.registry.nextPage')}
          </Button>
        </nav>
      )}
    </section>
  )
}
