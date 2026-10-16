import type { DownloadJobDto } from '@/shared/ipc/gateway'

/** A complete job DTO for tests; override only what a case cares about. */
export function job(overrides: Partial<DownloadJobDto> = {}): DownloadJobDto {
  return {
    id: 'job-1',
    state: 'queued',
    provider: 'direct',
    label: 'https://example.com/model.safetensors',
    destination: {
      rootId: 'models',
      category: 'checkpoints',
      subdirectory: '',
      filename: 'model.safetensors',
      relativePath: 'checkpoints/model.safetensors',
    },
    size: { kind: 'exact', bytes: 1000 },
    bytesDone: 0,
    requestedEngine: 'native',
    effectiveEngine: 'native',
    estimated: false,
    generation: 1,
    retryCount: 0,
    retryAt: null,
    error: null,
    decisionId: null,
    conflictCurrent: null,
    finalizing: false,
    expectedSha256: null,
    verified: false,
    createdAt: 1,
    updatedAt: 1,
    ...overrides,
  }
}
