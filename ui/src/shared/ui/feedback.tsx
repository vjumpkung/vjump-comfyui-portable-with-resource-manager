import { TriangleAlert } from 'lucide-react'

import { errorMessage } from '@/shared/lib/display'
import { Alert, AlertDescription, AlertTitle } from './alert'
import { Button } from './button'

export function ErrorNotice({
  error,
  retry,
}: {
  error: unknown
  retry?: () => void
}) {
  return (
    <Alert variant="destructive">
      <TriangleAlert />
      <AlertTitle>Unable to complete the request</AlertTitle>
      <AlertDescription>
        {errorMessage(error)}
        {retry && (
          <Button variant="outline" size="sm" onClick={retry}>
            Try again
          </Button>
        )}
      </AlertDescription>
    </Alert>
  )
}
