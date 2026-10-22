import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'

import { t as translate } from '@/shared/i18n'
import {
  gateway,
  IpcClientError,
  type GalleryAreaDto,
  type GalleryBatchReportDto,
  type GalleryItemResultDto,
  type GalleryLocationDto,
  type GalleryRenameInput,
  type GalleryTransferInput,
} from '@/shared/ipc/gateway'
import { errorMessage } from '@/shared/lib/display'

import { baseName } from './format'

export const GALLERY_KEY = ['gallery'] as const

export type TransferMode = 'move' | 'copy'
type BatchAction = TransferMode | 'delete'

/** How many failed items a report names before it summarizes the rest. */
const MAX_LISTED = 5

function hasCode(error: unknown, code: string): boolean {
  return error instanceof IpcClientError && error.code === code
}

export function isNotInstalled(error: unknown): boolean {
  return hasCode(error, 'gallery_not_installed')
}

export function isBusy(error: unknown): boolean {
  return hasCode(error, 'gallery_busy')
}

/** The sanitized message, with a stale reference turned into a request to refresh. */
export function galleryErrorMessage(error: unknown): string {
  return hasCode(error, 'gallery_stale')
    ? translate('gallery.error.stale')
    : errorMessage(error)
}

/** The message a dialog shows inline. A busy backend is toasted instead, so the dialog stays retryable. */
export function inlineError(error: unknown): string | null {
  return error === null || error === undefined || isBusy(error)
    ? null
    : galleryErrorMessage(error)
}

export function useGalleryListing(area: GalleryAreaDto, path: string) {
  return useQuery({
    queryKey: [...GALLERY_KEY, area, path],
    queryFn: () => gateway.galleryList({ area, path }),
  })
}

export function useRefreshGallery(): () => Promise<void> {
  const client = useQueryClient()
  return () => client.invalidateQueries({ queryKey: GALLERY_KEY })
}

function doneText(action: BatchAction, count: number): string {
  switch (action) {
    case 'move':
      return translate('gallery.report.moved', { count })
    case 'copy':
      return translate('gallery.report.copied', { count })
    case 'delete':
      return translate('gallery.report.deleted', { count })
  }
}

function problemText(result: GalleryItemResultDto): string {
  const reason =
    result.error?.message ??
    translate(
      result.outcome === 'source_retained'
        ? 'gallery.report.sourceRetained'
        : 'common.operationError',
    )
  return `${baseName(result.source.path)}: ${reason}`
}

/** Summarizes a batch: what worked, and by name what did not. */
function reportBatch(report: GalleryBatchReportDto, action: BatchAction): void {
  const succeeded = report.results.filter((result) => result.outcome === 'succeeded')
  const problems = report.results.filter(
    (result) => result.outcome === 'failed' || result.outcome === 'source_retained',
  )
  if (succeeded.length > 0) {
    toast.success(doneText(action, succeeded.length))
  } else if (problems.length === 0) {
    toast.info(translate('gallery.report.unchanged'))
  }
  if (problems.length > 0) {
    const listed = problems.slice(0, MAX_LISTED).map(problemText)
    if (problems.length > listed.length) {
      listed.push(translate('gallery.more', { count: problems.length - listed.length }))
    }
    toast.error(translate('gallery.report.problems', { count: problems.length }), {
      description: listed.join('; '),
    })
  }
}

/** Runs a change, then refetches every listing, whatever the outcome. */
function useGalleryMutation<Input, Output>(
  run: (input: Input) => Promise<Output>,
  onDone?: (output: Output) => void,
) {
  const client = useQueryClient()
  return useMutation({
    mutationFn: run,
    onSuccess: onDone,
    onError: (error) => {
      if (isBusy(error)) toast.error(errorMessage(error))
    },
    onSettled: () => client.invalidateQueries({ queryKey: GALLERY_KEY }),
  })
}

export function useRenameEntry() {
  return useGalleryMutation((input: GalleryRenameInput) => gateway.galleryRename(input))
}

export function useTransferEntries(mode: TransferMode) {
  return useGalleryMutation(
    (input: GalleryTransferInput) =>
      mode === 'move' ? gateway.galleryMove(input) : gateway.galleryCopy(input),
    (report) => reportBatch(report, mode),
  )
}

export function useDeleteEntries() {
  return useGalleryMutation(
    (planId: string) => gateway.galleryDelete(planId),
    (report) => reportBatch(report, 'delete'),
  )
}

/** Shows an entry or folder in the system file manager; a failure becomes a toast. */
export async function reveal(location: GalleryLocationDto): Promise<void> {
  try {
    await gateway.galleryReveal(location)
  } catch (error) {
    toast.error(errorMessage(error))
  }
}
