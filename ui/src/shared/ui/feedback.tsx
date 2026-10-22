import { TriangleAlert } from 'lucide-react'

import { useTranslation } from '@/shared/i18n'
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
  const { t } = useTranslation()
  return (
    <Alert variant="destructive">
      <TriangleAlert />
      <AlertTitle>{t('common.requestError')}</AlertTitle>
      <AlertDescription>
        {errorMessage(error)}
        {retry && (
          <Button variant="outline" size="sm" onClick={retry}>
            {t('common.tryAgain')}
          </Button>
        )}
      </AlertDescription>
    </Alert>
  )
}
