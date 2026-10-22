import { t } from '@/shared/i18n'
import { IpcClientError } from '@/shared/ipc/gateway'

export function errorMessage(error: unknown): string {
  return error instanceof IpcClientError ? error.message : t('common.somethingWrong')
}
