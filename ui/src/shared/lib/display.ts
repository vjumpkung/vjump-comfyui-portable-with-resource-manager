import { IpcClientError } from '@/shared/ipc/gateway'

export function errorMessage(error: unknown): string {
  return error instanceof IpcClientError
    ? error.message
    : 'The operation could not be completed.'
}

export function bytes(value: number | null): string {
  return value === null ? 'Unknown' : `${(value / 1024 ** 3).toFixed(1)} GiB`
}

export function label(value: string): string {
  return value.replaceAll('_', ' ')
}
