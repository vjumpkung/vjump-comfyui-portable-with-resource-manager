import { useQuery } from '@tanstack/react-query'
import { Boxes, LoaderCircle, RefreshCw, Search, TriangleAlert } from 'lucide-react'
import { useMemo, useState } from 'react'
import { toast } from 'sonner'

import { t as translate, useTranslation } from '@/shared/i18n'
import {
  gateway,
  type ModelEntryDto,
  type ModelLocationDto,
} from '@/shared/ipc/gateway'
import { Alert, AlertDescription, AlertTitle } from '@/shared/ui/alert'
import { Button } from '@/shared/ui/button'
import { EmptyState } from '@/shared/ui/empty-state'
import { Input } from '@/shared/ui/input'
import { cn } from '@/shared/lib/utils'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shared/ui/dialog'

import { DeleteDialog } from './delete-dialog'
import { DetailsPane } from './details-pane'
import { libraryStore, type LibraryStore } from './library-store'
import { MoveDialog } from './move-dialog'
import {
  categoriesOf,
  DEFAULT_FILTERS,
  filterModels,
  formatBytes,
  keyOf,
  type Filters,
  type SortKey,
} from './model-list'
import { ModelTable } from './model-table'
import { RenameDialog } from './rename-dialog'
import { useGuarded } from './use-guarded'
import { useLibrary } from './use-library'

const SELECT_CLASS =
  'h-9 rounded-md border border-input bg-card px-2.5 text-sm outline-none focus-visible:border-brand-cyan focus-visible:ring-2 focus-visible:ring-brand-cyan/35'

type Dialogs =
  | { kind: 'rename'; model: ModelEntryDto }
  | { kind: 'move'; model: ModelEntryDto }
  | { kind: 'delete'; locations: ModelLocationDto[] }
  | { kind: 'replace'; model: ModelEntryDto }
  | null

interface Props {
  /** Stops ComfyUI so a file it holds open can be changed. Wired by the app shell. */
  onStopComfy: () => Promise<void>
  store?: LibraryStore
}

export function LibraryPage({ onStopComfy, store = libraryStore }: Props) {
  const { t } = useTranslation()
  const library = useLibrary(store)
  const guard = useGuarded(onStopComfy)
  const [filters, setFilters] = useState<Filters>(DEFAULT_FILTERS)
  const [selected, setSelected] = useState<ReadonlySet<string>>(new Set())
  const [activeKey, setActiveKey] = useState<string | null>(null)
  const [dialog, setDialog] = useState<Dialogs>(null)
  const [identifying, setIdentifying] = useState(false)

  const status = library.status ?? library.snapshot?.status ?? null
  const all = useMemo(() => library.snapshot?.models ?? [], [library.snapshot])
  const visible = useMemo(() => filterModels(all, filters), [all, filters])
  const categories = useMemo(() => categoriesOf(all), [all])
  const active = visible.find((m) => keyOf(m.location) === activeKey) ?? null
  const selectedVisible = visible.filter((m) => selected.has(keyOf(m.location)))
  const scanning = library.rescanning || (status?.scanning ?? false)

  const operations = useQuery({
    queryKey: ['model-operations', status?.sequence ?? 0],
    queryFn: () => gateway.modelOperationStatus(),
    enabled: library.snapshot !== null,
  })
  const needsRecovery = (operations.data ?? []).some(
    (operation) => operation.phase === 'recovery_required',
  )

  const patch = (change: Partial<Filters>) =>
    setFilters((current) => ({ ...current, ...change }))

  function toggle(key: string) {
    setSelected((current) => {
      const next = new Set(current)
      if (!next.delete(key)) next.add(key)
      return next
    })
  }
  function toggleAll() {
    const keys = visible.map((m) => keyOf(m.location))
    setSelected((current) =>
      keys.every((key) => current.has(key)) ? new Set() : new Set(keys),
    )
  }

  async function rescan() {
    try {
      await store.rescan()
    } catch {
      toast.error(translate('library.scanFailed'))
    }
  }

  function askDelete() {
    const targets =
      selectedVisible.length > 0 ? selectedVisible : active ? [active] : []
    if (targets.length > 0)
      setDialog({ kind: 'delete', locations: targets.map((m) => m.location) })
  }

  async function identify(model: ModelEntryDto, replaceExisting: boolean) {
    setIdentifying(true)
    await guard.run(async () => {
      const outcome = await gateway.modelIdentify({
        location: model.location,
        replaceExisting,
      })
      if (outcome.kind === 'sidecars_exist') {
        setDialog({ kind: 'replace', model })
      } else if (outcome.kind === 'not_identified') {
        toast.info(translate('library.notIdentified'))
      } else {
        toast.success(
          translate(outcome.preview === 'written' ? 'library.identified' : 'library.identifiedNoPreview', { name: outcome.version.modelName }),
        )
      }
      void store.refresh()
    })
    setIdentifying(false)
  }

  const roots = status?.roots ?? []
  const unavailableRoots = roots.filter((root) => root.state !== 'ready')
  const warnings = status?.warnings ?? []

  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h1 className="text-2xl font-extrabold tracking-tighter sm:text-3xl">
            {t('library.title')}
          </h1>
          <p className="font-mono text-xs text-muted-foreground">
            {all.length === 0
              ? t('library.summaryEmpty', { count: all.length })
              : selectedVisible.length > 0
                ? t('library.summarySelected', { count: all.length, size: formatBytes(all.reduce((sum, m) => sum + (m.size ?? 0), 0)), selected: selectedVisible.length })
                : t('library.summary', { count: all.length, size: formatBytes(all.reduce((sum, m) => sum + (m.size ?? 0), 0)) })}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          {selectedVisible.length > 0 ? (
            <Button variant="outline" onClick={askDelete}>
              {t('library.deleteSelected', { count: selectedVisible.length })}
            </Button>
          ) : null}
          <Button variant="outline" disabled={scanning} onClick={() => void rescan()}>
            {scanning ? (
              <LoaderCircle className="animate-spin" aria-hidden="true" />
            ) : (
              <RefreshCw aria-hidden="true" />
            )}
            {t('library.rescan')}
          </Button>
        </div>
      </div>

      {status && status.watcher !== 'native' ? (
        <Alert>
          <TriangleAlert aria-hidden="true" />
          <AlertTitle>{t('library.watcher.title')}</AlertTitle>
          <AlertDescription>
            {t(status.watcher === 'polling' ? 'library.watcher.polling' : 'library.watcher.disabled')}
          </AlertDescription>
        </Alert>
      ) : null}
      {needsRecovery ? (
        <Alert variant="destructive">
          <TriangleAlert aria-hidden="true" />
          <AlertTitle>{t('library.recovery.title')}</AlertTitle>
          <AlertDescription>
            {t('library.recovery.hint')}
          </AlertDescription>
        </Alert>
      ) : null}
      {unavailableRoots.length > 0 ? (
        <Alert>
          <TriangleAlert aria-hidden="true" />
          <AlertTitle>{t('library.unavailable.title')}</AlertTitle>
          <AlertDescription>
            {t('library.unavailable.hint', { folders: unavailableRoots.map((root) => root.label).join(', ') })}
          </AlertDescription>
        </Alert>
      ) : null}
      {warnings.length > 0 ? (
        <details className="text-sm text-muted-foreground">
          <summary className="cursor-pointer">
            {t('library.scanWarnings', { count: warnings.length })}
          </summary>
          <ul className="mt-2 flex flex-col gap-1 font-mono text-xs">
            {warnings.slice(0, 50).map((warning, index) => (
              <li key={index}>
                {warning.code.replaceAll('_', ' ')}: {warning.detail}
              </li>
            ))}
          </ul>
        </details>
      ) : null}

      <div className="flex flex-wrap items-center gap-2">
        <div className="relative min-w-56 flex-1">
          <Search
            className="pointer-events-none absolute top-2.5 left-2.5 size-4 text-muted-foreground"
            aria-hidden="true"
          />
          <Input
            aria-label={t('library.search')}
            placeholder={t('library.searchHint')}
            className="pl-8"
            value={filters.search}
            onChange={(event) => patch({ search: event.target.value })}
          />
        </div>
        <select
          aria-label={t('common.category')}
          className={SELECT_CLASS}
          value={filters.category}
          onChange={(event) => patch({ category: event.target.value })}
        >
          <option value="all">{t('library.allCategories')}</option>
          {categories.map((category) => (
            <option key={category} value={category}>
              {category}
            </option>
          ))}
        </select>
        {roots.length > 1 ? (
          <select
            aria-label={t('library.modelFolder')}
            className={SELECT_CLASS}
            value={filters.root}
            onChange={(event) => patch({ root: event.target.value })}
          >
            <option value="all">{t('library.allFolders')}</option>
            {roots.map((root) => (
              <option key={root.id} value={root.id}>
                {root.label}
              </option>
            ))}
          </select>
        ) : null}
        <select
          aria-label={t('library.sortBy')}
          className={SELECT_CLASS}
          value={filters.sort}
          onChange={(event) => patch({ sort: event.target.value as SortKey })}
        >
          <option value="name">{t('common.name')}</option>
          <option value="category">{t('common.category')}</option>
          <option value="size">{t('common.size')}</option>
          <option value="modified">{t('library.modified')}</option>
        </select>
        <Button
          variant="outline"
          aria-label={filters.descending ? t('library.descending') : t('library.ascending')}
          onClick={() => patch({ descending: !filters.descending })}
        >
          {filters.descending ? t('library.descending') : t('library.ascending')}
        </Button>
      </div>

      {library.snapshot === null ? (
        <p role="status" className="text-sm text-muted-foreground">
          {t('library.loading')}
        </p>
      ) : all.length === 0 ? (
        <EmptyState
          icon={<Boxes className="size-8" aria-hidden="true" />}
          title={t('library.empty')}
          description={t('library.emptyHint')}
        />
      ) : visible.length === 0 ? (
        <EmptyState title={t('library.noMatch')} description={t('library.changeFilters')} />
      ) : (
        <div
          className={cn(
            'grid items-start gap-4',
            active ? 'lg:grid-cols-[minmax(0,1fr)_24rem]' : '',
          )}
        >
          <div className="min-w-0 overflow-x-auto">
            <div className="min-w-[52rem]">
              <ModelTable
                models={visible}
                selected={selected}
                activeKey={activeKey}
                onActivate={setActiveKey}
                onToggle={toggle}
                onToggleAll={toggleAll}
                onDelete={askDelete}
              />
            </div>
          </div>
          {active ? (
            <DetailsPane
              model={active}
              identifying={identifying}
              onIdentify={() => void identify(active, false)}
              onRename={() => setDialog({ kind: 'rename', model: active })}
              onMove={() => setDialog({ kind: 'move', model: active })}
              onDelete={() =>
                setDialog({ kind: 'delete', locations: [active.location] })
              }
            />
          ) : null}
        </div>
      )}

      {dialog?.kind === 'rename' ? (
        <RenameDialog
          model={dialog.model}
          onClose={() => setDialog(null)}
          onSubmit={(newName, allowExtensionChange) =>
            guard.run(
              async () => {
                const placed = await gateway.modelRename({
                  location: dialog.model.location,
                  newName,
                  allowExtensionChange,
                })
                setActiveKey(keyOf(placed))
                await store.refresh()
              },
              () => setDialog(null),
            )
          }
        />
      ) : null}
      {dialog?.kind === 'move' ? (
        <MoveDialog
          model={dialog.model}
          roots={roots}
          categories={categories}
          onClose={() => setDialog(null)}
          onSubmit={(rootId, directory) =>
            guard.run(
              async () => {
                const placed = await gateway.modelMove({
                  location: dialog.model.location,
                  targetRootId: rootId,
                  targetDirectory: directory,
                })
                setActiveKey(keyOf(placed))
                await store.refresh()
              },
              () => setDialog(null),
            )
          }
        />
      ) : null}
      {dialog?.kind === 'delete' ? (
        <DeleteDialog
          locations={dialog.locations}
          onStopComfy={onStopComfy}
          onChanged={() => {
            setSelected(new Set())
            void store.refresh()
          }}
          onClose={() => setDialog(null)}
        />
      ) : null}
      {dialog?.kind === 'replace' ? (
        <Dialog open onOpenChange={(open) => !open && setDialog(null)}>
          <DialogContent>
            <DialogHeader>
              <DialogTitle>{t('library.replace.title')}</DialogTitle>
              <DialogDescription>
                {t('library.replace.hint', { name: dialog.model.name })}
              </DialogDescription>
            </DialogHeader>
            <DialogFooter>
              <Button variant="outline" onClick={() => setDialog(null)}>
                {t('library.replace.keep')}
              </Button>
              <Button
                onClick={() => {
                  const model = dialog.model
                  setDialog(null)
                  void identify(model, true)
                }}
              >
                {t('library.replace.replace')}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      ) : null}
      {guard.prompt}
    </div>
  )
}
