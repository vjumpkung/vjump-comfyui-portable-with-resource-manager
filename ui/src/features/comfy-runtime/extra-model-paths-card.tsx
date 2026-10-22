import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'

import { useTranslation } from '@/shared/i18n'
import { gateway } from '@/shared/ipc/gateway'
import { errorMessage } from '@/shared/lib/display'
import { Button } from '@/shared/ui/button'
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/shared/ui/card'
import { Input } from '@/shared/ui/input'
import { Label } from '@/shared/ui/label'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/shared/ui/table'

import { comfyKeys } from './use-runtime'

/** The section of extra_model_paths.yaml that the app writes and removes. */
const APP_SECTION = 'vjumpkung_desktop'

export function ExtraModelPathsCard({ running }: { running: boolean }) {
  const { t } = useTranslation()
  const client = useQueryClient()
  const extraPaths = useQuery({
    queryKey: comfyKeys.extraPaths,
    queryFn: () => gateway.comfyReadExtraModelPaths(),
  })
  const [basePath, setBasePath] = useState('')
  const [error, setError] = useState<unknown>(null)
  // A change reaches ComfyUI only at its next start; the hint ends with the running session.
  const [changed, setChanged] = useState(false)
  if (!running && changed) setChanged(false)

  const settled = {
    onSuccess: () => {
      setError(null)
      setChanged(true)
    },
    onError: (failure: unknown) => setError(failure),
    // The backend rescans the library after a write; the list is refetched here.
    onSettled: () =>
      void client.invalidateQueries({ queryKey: comfyKeys.extraPaths }),
  }
  const generate = useMutation({
    mutationFn: (path: string) => gateway.comfyGenerateExtraModelPaths(path),
    ...settled,
  })
  const remove = useMutation({
    mutationFn: () => gateway.comfyRemoveExtraModelPaths(),
    ...settled,
  })
  const browse = useMutation({
    mutationFn: () => gateway.pickFolder(t('runtime.paths.choose')),
    onSuccess: (path) => {
      if (path) setBasePath(path)
    },
    onError: (failure) => setError(failure),
  })

  const busy = generate.isPending || remove.isPending
  const hasAppSection = (extraPaths.data ?? []).some(
    (root) => root.section === APP_SECTION,
  )

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-lg font-bold tracking-tight">
          {t('runtime.paths.title')}
        </CardTitle>
        <CardDescription>
          {t('runtime.paths.hint')}
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="extra-paths-base">{t('runtime.paths.base')}</Label>
          <div className="flex gap-2">
            <Input
              id="extra-paths-base"
              value={basePath}
              placeholder="D:\ComfyModels"
              spellCheck={false}
              onChange={(event) => setBasePath(event.target.value)}
            />
            <Button
              variant="outline"
              disabled={browse.isPending}
              onClick={() => browse.mutate()}
            >
              {t('common.browse')}
            </Button>
          </div>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button
            disabled={busy || basePath.trim() === ''}
            onClick={() => generate.mutate(basePath.trim())}
          >
            {t('runtime.paths.generate')}
          </Button>
          <Button
            variant="outline"
            disabled={busy || !hasAppSection}
            onClick={() => remove.mutate()}
          >
            {t('runtime.paths.remove')}
          </Button>
        </div>
        {error !== null ? (
          <p role="alert" className="text-sm text-destructive">
            {errorMessage(error)}
          </p>
        ) : null}
        {changed && running ? (
          <p role="status" className="text-sm text-muted-foreground">
            {t('runtime.paths.restart')}
          </p>
        ) : null}
        <p className="text-xs text-muted-foreground">
          {t('runtime.paths.readFrom')}
        </p>
        {extraPaths.data && extraPaths.data.length > 0 ? (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>{t('runtime.paths.section')}</TableHead>
                <TableHead>{t('common.category')}</TableHead>
                <TableHead>{t('common.path')}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {extraPaths.data.map((root) => (
                <TableRow key={`${root.section}/${root.category}/${root.path}`}>
                  <TableCell>
                    {root.isDefault ? t('runtime.paths.defaultSection', { section: root.section }) : root.section}
                  </TableCell>
                  <TableCell>{root.category}</TableCell>
                  <TableCell className="font-mono text-xs break-all">
                    {root.path}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        ) : extraPaths.isError ? (
          <p className="text-sm text-destructive">
            {errorMessage(extraPaths.error)}
          </p>
        ) : (
          <p className="text-sm text-muted-foreground">
            {extraPaths.isPending
              ? t('common.loading')
              : t('runtime.paths.empty')}
          </p>
        )}
      </CardContent>
    </Card>
  )
}
