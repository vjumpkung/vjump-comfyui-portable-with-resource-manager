import { useQuery } from '@tanstack/react-query'
import { Boxes, LoaderCircle, RefreshCw, Search, TriangleAlert } from 'lucide-react'
import { useMemo, useState } from 'react'
import { toast } from 'sonner'

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
      toast.error('The library could not be scanned.')
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
        toast.info('This file is not known to CivitAI.')
      } else {
        toast.success(
          `Identified as ${outcome.version.modelName}${
            outcome.preview === 'written' ? '' : ' (no preview image saved)'
          }.`,
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
            Models
          </h1>
          <p className="font-mono text-xs text-muted-foreground">
            {all.length} models
            {all.length > 0
              ? ` · ${formatBytes(all.reduce((sum, m) => sum + (m.size ?? 0), 0))}`
              : ''}
            {selectedVisible.length > 0
              ? ` · ${selectedVisible.length} selected`
              : ''}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          {selectedVisible.length > 0 ? (
            <Button variant="outline" onClick={askDelete}>
              Delete {selectedVisible.length} selected
            </Button>
          ) : null}
          <Button variant="outline" disabled={scanning} onClick={() => void rescan()}>
            {scanning ? (
              <LoaderCircle className="animate-spin" aria-hidden="true" />
            ) : (
              <RefreshCw aria-hidden="true" />
            )}
            Rescan
          </Button>
        </div>
      </div>

      {status && status.watcher !== 'native' ? (
        <Alert>
          <TriangleAlert aria-hidden="true" />
          <AlertTitle>File watching is degraded</AlertTitle>
          <AlertDescription>
            {status.watcher === 'polling'
              ? 'Changes are checked every 30 seconds.'
              : 'Changes are not detected automatically.'}{' '}
            Files changed outside this app may appear late. Use Rescan to refresh
            now.
          </AlertDescription>
        </Alert>
      ) : null}
      {needsRecovery ? (
        <Alert variant="destructive">
          <TriangleAlert aria-hidden="true" />
          <AlertTitle>An earlier rename or move is unfinished</AlertTitle>
          <AlertDescription>
            Files involved are locked against new changes. Rescan to retry the
            recovery.
          </AlertDescription>
        </Alert>
      ) : null}
      {unavailableRoots.length > 0 ? (
        <Alert>
          <TriangleAlert aria-hidden="true" />
          <AlertTitle>Some model folders are unavailable</AlertTitle>
          <AlertDescription>
            {unavailableRoots.map((root) => root.label).join(', ')}. Their models
            stay listed until the folder is back.
          </AlertDescription>
        </Alert>
      ) : null}
      {warnings.length > 0 ? (
        <details className="text-sm text-muted-foreground">
          <summary className="cursor-pointer">
            {warnings.length} scan {warnings.length === 1 ? 'warning' : 'warnings'}
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
            aria-label="Search models"
            placeholder="Search by name, folder or category"
            className="pl-8"
            value={filters.search}
            onChange={(event) => patch({ search: event.target.value })}
          />
        </div>
        <select
          aria-label="Category"
          className={SELECT_CLASS}
          value={filters.category}
          onChange={(event) => patch({ category: event.target.value })}
        >
          <option value="all">All categories</option>
          {categories.map((category) => (
            <option key={category} value={category}>
              {category}
            </option>
          ))}
        </select>
        {roots.length > 1 ? (
          <select
            aria-label="Model folder"
            className={SELECT_CLASS}
            value={filters.root}
            onChange={(event) => patch({ root: event.target.value })}
          >
            <option value="all">All folders</option>
            {roots.map((root) => (
              <option key={root.id} value={root.id}>
                {root.label}
              </option>
            ))}
          </select>
        ) : null}
        <select
          aria-label="Sort by"
          className={SELECT_CLASS}
          value={filters.sort}
          onChange={(event) => patch({ sort: event.target.value as SortKey })}
        >
          <option value="name">Name</option>
          <option value="category">Category</option>
          <option value="size">Size</option>
          <option value="modified">Modified</option>
        </select>
        <Button
          variant="outline"
          aria-label={filters.descending ? 'Descending' : 'Ascending'}
          onClick={() => patch({ descending: !filters.descending })}
        >
          {filters.descending ? 'Descending' : 'Ascending'}
        </Button>
      </div>

      {library.snapshot === null ? (
        <p role="status" className="text-sm text-muted-foreground">
          Loading the model library...
        </p>
      ) : all.length === 0 ? (
        <EmptyState
          icon={<Boxes className="size-8" aria-hidden="true" />}
          title="No models found"
          description="Downloaded models and files in your ComfyUI model folders appear here."
        />
      ) : visible.length === 0 ? (
        <EmptyState title="No models match" description="Change the search or filters." />
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
              <DialogTitle>Replace existing metadata?</DialogTitle>
              <DialogDescription>
                {dialog.model.name} already has a CivitAI info or preview file.
                Replacing overwrites them with the data CivitAI returned now.
              </DialogDescription>
            </DialogHeader>
            <DialogFooter>
              <Button variant="outline" onClick={() => setDialog(null)}>
                Keep existing
              </Button>
              <Button
                onClick={() => {
                  const model = dialog.model
                  setDialog(null)
                  void identify(model, true)
                }}
              >
                Replace
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      ) : null}
      {guard.prompt}
    </div>
  )
}
