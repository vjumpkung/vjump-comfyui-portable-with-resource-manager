import { useQuery } from '@tanstack/react-query'
import {
  Fingerprint,
  FolderInput,
  ImageOff,
  LoaderCircle,
  Pencil,
  Trash2,
} from 'lucide-react'
import type { ReactNode } from 'react'

import { useTranslation } from '@/shared/i18n'
import { gateway, type ModelEntryDto } from '@/shared/ipc/gateway'
import { Badge } from '@/shared/ui/badge'
import { Button } from '@/shared/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/shared/ui/card'

import {
  formatBytes,
  formatModified,
  keyOf,
  sourceLabel,
  trashName,
} from './model-list'

interface Props {
  model: ModelEntryDto
  identifying: boolean
  onRename: () => void
  onMove: () => void
  onDelete: () => void
  onIdentify: () => void
}

function Field({ name, children }: { name: string; children: ReactNode }) {
  return (
    <div className="grid grid-cols-[6.5rem_minmax(0,1fr)] gap-2 text-sm">
      <dt className="text-muted-foreground">{name}</dt>
      <dd className="min-w-0 break-words [overflow-wrap:anywhere]">{children}</dd>
    </div>
  )
}

/** The selected model: preview, CivitAI details, hash and the actions on it. */
export function DetailsPane({
  model,
  identifying,
  onRename,
  onMove,
  onDelete,
  onIdentify,
}: Props) {
  const { t } = useTranslation()
  const hasSidecars = model.sidecars.preview || model.sidecars.civitaiInfo
  const details = useQuery({
    queryKey: [
      'model-details',
      keyOf(model.location),
      model.modifiedMs,
      model.civitaiVersionId,
      model.sidecars.preview,
      model.sidecars.civitaiInfo,
    ],
    queryFn: () => gateway.modelDetails(model.location),
    enabled: hasSidecars,
  })
  const civitai = details.data?.civitai ?? null
  const preview = details.data?.previewDataUrl ?? null

  return (
    <Card aria-label={t('library.details.title')}>
      <CardHeader>
        <CardTitle className="break-words [overflow-wrap:anywhere]">
          {model.name}
        </CardTitle>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        {preview ? (
          <img
            src={preview}
            alt={t('library.details.previewOf', { name: model.name })}
            className="max-h-64 w-full rounded-lg border object-contain"
          />
        ) : (
          <div className="flex h-28 items-center justify-center gap-2 rounded-lg border border-dashed text-sm text-muted-foreground">
            <ImageOff className="size-4" aria-hidden="true" />
            {t('library.details.noPreview')}
          </div>
        )}

        <dl className="flex flex-col gap-2">
          <Field name={t('common.category')}>{model.category}</Field>
          <Field name={t('library.details.folder')}>
            <span className="font-mono text-xs">
              {model.location.path}
            </span>
          </Field>
          <Field name={t('common.size')}>{formatBytes(model.size)}</Field>
          <Field name={t('library.modified')}>{formatModified(model.modifiedMs)}</Field>
          <Field name={t('common.source')}>{sourceLabel(model)}</Field>
          <Field name="SHA-256">
            {model.sha256 ? (
              <span className="font-mono text-xs">{model.sha256}</span>
            ) : (
              <span className="text-muted-foreground">
                {t('library.details.notHashed')}
              </span>
            )}
          </Field>
        </dl>

        {civitai ? (
          <section aria-label="CivitAI" className="flex flex-col gap-2">
            <h3 className="text-sm font-semibold">CivitAI</h3>
            <dl className="flex flex-col gap-2">
              <Field name={t('library.details.model')}>{civitai.modelName}</Field>
              <Field name={t('common.version')}>{civitai.versionName || '-'}</Field>
              <Field name={t('library.details.baseModel')}>{civitai.baseModel ?? t('common.unknownValue')}</Field>
              <Field name={t('library.details.triggerWords')}>
                {civitai.triggerWords.length === 0 ? (
                  <span className="text-muted-foreground">{t('common.none')}</span>
                ) : (
                  <span className="flex flex-wrap gap-1">
                    {civitai.triggerWords.map((word) => (
                      <Badge key={word} variant="secondary">
                        {word}
                      </Badge>
                    ))}
                  </span>
                )}
              </Field>
            </dl>
          </section>
        ) : null}

        <div className="flex flex-wrap gap-2">
          <Button variant="outline" disabled={identifying} onClick={onIdentify}>
            {identifying ? (
              <LoaderCircle className="animate-spin" aria-hidden="true" />
            ) : (
              <Fingerprint aria-hidden="true" />
            )}
            {t('library.details.identify')}
          </Button>
          <Button variant="outline" onClick={onRename}>
            <Pencil aria-hidden="true" />
            {t('common.rename')}
          </Button>
          <Button variant="outline" onClick={onMove}>
            <FolderInput aria-hidden="true" />
            {t('common.move')}
          </Button>
          <Button variant="outline" onClick={onDelete}>
            <Trash2 aria-hidden="true" />
            {t('common.delete')}
          </Button>
        </div>
        <p className="text-xs text-muted-foreground">
          {t('library.details.deleteHint', { trash: trashName() })}
        </p>
      </CardContent>
    </Card>
  )
}
