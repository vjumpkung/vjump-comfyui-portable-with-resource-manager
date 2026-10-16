import type {
  LibrarySnapshotDto,
  LibraryStatusDto,
  ModelEntryDto,
} from '@/shared/ipc/gateway'

/** A complete model DTO for tests; override only what a case cares about. */
export function model(overrides: Partial<ModelEntryDto> = {}): ModelEntryDto {
  const name = overrides.name ?? 'model.safetensors'
  const category = overrides.category ?? 'checkpoints'
  return {
    location: { rootId: 'models', path: `${category}/${name}` },
    category,
    name,
    subfolder: '',
    size: 2 * 1024 ** 3,
    modifiedMs: 1_700_000_000_000,
    sha256: null,
    sourceJobId: null,
    civitaiVersionId: null,
    sidecars: { preview: false, civitaiInfo: false, json: false },
    ...overrides,
  }
}

export function status(
  overrides: Partial<LibraryStatusDto> = {},
): LibraryStatusDto {
  return {
    sequence: 1,
    roots: [
      {
        id: 'models',
        label: 'ComfyUI models',
        path: 'C:/ComfyUI/models',
        category: null,
        state: 'ready',
      },
    ],
    warnings: [],
    scanning: false,
    watcher: 'native',
    ...overrides,
  }
}

export function snapshot(
  models: ModelEntryDto[],
  overrides: Partial<LibraryStatusDto> = {},
): LibrarySnapshotDto {
  return { revision: 1, models, status: status(overrides) }
}

/** `count` distinct models spread over two categories. */
export function many(count: number): ModelEntryDto[] {
  return Array.from({ length: count }, (_, index) =>
    model({
      name: `model-${String(index).padStart(4, '0')}.safetensors`,
      category: index % 2 === 0 ? 'checkpoints' : 'loras',
      size: (index + 1) * 1024 * 1024,
    }),
  )
}
