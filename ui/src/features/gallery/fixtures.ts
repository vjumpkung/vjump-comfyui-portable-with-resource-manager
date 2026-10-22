import type {
  GalleryAreaDto,
  GalleryBatchReportDto,
  GalleryEntryDto,
  GalleryItemResultDto,
  GalleryListingDto,
  GalleryMediaKindDto,
  GalleryWarningDto,
} from '@/shared/ipc/gateway'

const at = (directory: string, name: string) => (directory === '' ? name : `${directory}/${name}`)

/** A folder entry; the revision is derived from the name so tests can predict it. */
export function folder(name: string, directory = '', area: GalleryAreaDto = 'input'): GalleryEntryDto {
  return {
    location: { area, path: at(directory, name) },
    revision: `rev-${name}`,
    name,
    kind: 'directory',
    media: 'other',
    sizeBytes: null,
    modifiedMs: 1_700_000_000_000,
  }
}

export function file(
  name: string,
  media: GalleryMediaKindDto = 'image',
  directory = '',
  area: GalleryAreaDto = 'input',
): GalleryEntryDto {
  return {
    location: { area, path: at(directory, name) },
    revision: `rev-${name}`,
    name,
    kind: 'file',
    media,
    sizeBytes: 2048,
    modifiedMs: 1_700_000_000_000,
  }
}

export function listing(
  entries: GalleryEntryDto[],
  directory: { area: GalleryAreaDto; path: string } = { area: 'input', path: '' },
  warnings: GalleryWarningDto[] = [],
): GalleryListingDto {
  return { directory, entries, warnings }
}

export function result(
  entry: GalleryEntryDto,
  overrides: Partial<GalleryItemResultDto> = {},
): GalleryItemResultDto {
  return {
    source: entry.location,
    destination: null,
    outcome: 'succeeded',
    error: null,
    ...overrides,
  }
}

export function report(...results: GalleryItemResultDto[]): GalleryBatchReportDto {
  return { results }
}

/** `n` image files named `image-0000.png` and up. */
export function many(n: number): GalleryEntryDto[] {
  return Array.from({ length: n }, (_, index) => file(`image-${String(index).padStart(4, '0')}.png`))
}
