import { useMutation } from '@tanstack/react-query'
import { ClipboardCopy } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'

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
  // The report to select by hand when the clipboard could not be written.
  const [fallback, setFallback] = useState<string | null>(null)
  const copy = useMutation({
    mutationFn: () => gateway.appCopyDiagnostics(),
    onSuccess: (result) => {
      if (result.copied) {
        setFallback(null)
        toast.success('Diagnostics copied to the clipboard.')
      } else {
        setFallback(result.text)
        toast.error('The clipboard is unavailable. Select the report below.')
      }
    },
    onError: (error) => toast.error(errorMessage(error)),
  })

  return (
    <Card>
      <CardHeader>
        <CardTitle>Diagnostics</CardTitle>
        <CardDescription>
          Copy a report of this app, your hardware, the environment and recent
          logs for support. Tokens, signed links and the install folder path
          are removed.
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
            Copy diagnostics
          </Button>
        </div>
        {fallback !== null && (
          <textarea
            readOnly
            aria-label="Diagnostics report"
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
