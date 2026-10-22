import { Download, FileDown, FileUp, Plus } from 'lucide-react'
import { useState } from 'react'

import { useTranslation } from '@/shared/i18n'
import { Button } from '@/shared/ui/button'
import { EmptyState } from '@/shared/ui/empty-state'

import { AddDownloadDialog } from './add-download-dialog'
import { ExportDialog } from './export-dialog'
import { ImportPackDialog } from './import-pack-dialog'
import { QueueTable } from './queue-table'
import { useDownloads } from './use-downloads'
import { useEnqueue } from './use-enqueue'
import { isActive } from './format'

type Dialog = 'add' | 'import' | 'export' | null

export function DownloadsPage({
  onOpenSettings,
}: {
  onOpenSettings: () => void
}) {
  const { t } = useTranslation()
  const { jobs, progress, connected } = useDownloads()
  const [dialog, setDialog] = useState<Dialog>(null)
  const flow = useEnqueue(onOpenSettings)
  const list = Object.values(jobs)
  const active = list.filter(isActive).length

  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h1 className="text-2xl font-extrabold tracking-tighter sm:text-3xl">
            {t('nav.downloads')}
          </h1>
          <p className="font-mono text-xs text-muted-foreground">
            {t('downloads.counts', { active, total: list.length })}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button variant="outline" onClick={() => setDialog('import')}>
            <FileUp aria-hidden="true" />
            {t('downloads.import')}
          </Button>
          <Button variant="outline" onClick={() => setDialog('export')}>
            <FileDown aria-hidden="true" />
            {t('downloads.export')}
          </Button>
          <Button onClick={() => setDialog('add')}>
            <Plus aria-hidden="true" />
            {t('downloads.add')}
          </Button>
        </div>
      </div>

      {!connected ? (
        <p role="status" className="text-sm text-muted-foreground">
          {t('downloads.connecting')}
        </p>
      ) : null}

      {list.length === 0 ? (
        <EmptyState
          icon={<Download className="size-8" aria-hidden="true" />}
          title={t('downloads.emptyTitle')}
          description={t('downloads.emptyHint')}
        />
      ) : (
        <QueueTable
          jobs={list}
          progress={progress}
          onOpenSettings={onOpenSettings}
        />
      )}

      {dialog === 'add' ? (
        <AddDownloadDialog
          onClose={() => setDialog(null)}
          onSubmit={flow.submit}
          onOpenSettings={onOpenSettings}
        />
      ) : null}
      {dialog === 'import' ? (
        <ImportPackDialog
          onClose={() => setDialog(null)}
          onSubmit={flow.submit}
        />
      ) : null}
      {dialog === 'export' ? (
        <ExportDialog onClose={() => setDialog(null)} />
      ) : null}
      {flow.prompts}
    </div>
  )
}
