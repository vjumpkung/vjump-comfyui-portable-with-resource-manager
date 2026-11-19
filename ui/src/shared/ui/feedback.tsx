import { TriangleAlert } from 'lucide-react'

import { useTranslation } from '@/shared/i18n'
import { errorMessage } from '@/shared/lib/display'
import { Alert, AlertDescription, AlertTitle } from './alert'
import { Button } from './button'

export function ErrorNotice({ error, retry }: { error: unknown; retry?: () => void }) {
  const { t } = useTranslation()
  return (
    <Alert variant="destructive">
      <TriangleAlert />
      <AlertTitle>{t('common.requestError')}</AlertTitle>
      <AlertDescription>
        <p>{errorMessage(error)}</p>
        {retry && (
          <div className="mt-3 flex flex-wrap gap-2">
            <Button variant="outline" size="sm" onClick={retry}>
              {t('common.tryAgain')}
            </Button>
          </div>
        )}
      </AlertDescription>
    </Alert>
  )
}
