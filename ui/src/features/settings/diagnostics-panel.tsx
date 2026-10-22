import { useMutation } from '@tanstack/react-query'
import { ClipboardCopy } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'

import { t as translate, useTranslation } from '@/shared/i18n'
import { gateway } from '@/shared/ipc/gateway'
import { Button } from '@/shared/ui/button'
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/shared/ui/card'

import { errorMessage } from './error-message'

export function DiagnosticsPanel() {
  const { t } = useTranslation()
  // The report to select by hand when the clipboard could not be written.
  const [fallback, setFallback] = useState<string | null>(null)
  const copy = useMutation({
    mutationFn: () => gateway.appCopyDiagnostics(),
    onSuccess: (result) => {
      if (result.copied) {
        setFallback(null)
        toast.success(translate('settings.diagnostics.copied'))
      } else {
        setFallback(result.text)
        toast.error(translate('settings.diagnostics.clipboardError'))
      }
    },
    onError: (error) => toast.error(errorMessage(error)),
  })

  return (
    <Card>
      <CardHeader>
        <CardTitle>{t('settings.diagnostics.title')}</CardTitle>
        <CardDescription>
          {t('settings.diagnostics.hint')}
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        <div>
          <Button
            variant="outline"
            disabled={copy.isPending}
            onClick={() => copy.mutate()}
          >
            <ClipboardCopy aria-hidden="true" />
            {t('settings.diagnostics.copy')}
          </Button>
        </div>
        {fallback !== null && (
          <textarea
            readOnly
            aria-label={t('settings.diagnostics.report')}
            value={fallback}
            rows={12}
            onFocus={(event) => event.currentTarget.select()}
            className="w-full rounded-md border bg-muted p-3 font-mono text-xs"
          />
        )}
      </CardContent>
    </Card>
  )
}
