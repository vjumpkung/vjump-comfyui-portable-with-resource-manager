import { IpcClientError } from '@/shared/ipc/gateway'

export function errorMessage(error: unknown): string {
  return error instanceof IpcClientError ? error.message : 'Something went wrong.'
}
