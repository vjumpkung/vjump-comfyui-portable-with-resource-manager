import { Input } from '@/shared/ui/input'
import { Label } from '@/shared/ui/label'

const CUSTOM = '__custom'

interface Props {
  category: string
  onCategory: (value: string) => void
  subdirectory: string
  onSubdirectory: (value: string) => void
  suggested: string | null
  known: readonly string[]
  idPrefix?: string
}

/** Destination folder selection: a ComfyUI models/ category plus an optional sub-folder. */
export function CategoryFields({
  category,
  onCategory,
  subdirectory,
  onSubdirectory,
  suggested,
  known,
  idPrefix = 'download',
}: Props) {
  const isKnown = known.includes(category)
  const selectValue = category === '' ? '' : isKnown ? category : CUSTOM
  return (
    <div className="grid gap-4 sm:grid-cols-2">
      <div className="flex flex-col gap-1.5">
        <Label htmlFor={`${idPrefix}-category`}>Model folder</Label>
        <select
          id={`${idPrefix}-category`}
          value={selectValue}
          onChange={(event) =>
            onCategory(event.target.value === CUSTOM ? ' ' : event.target.value)
          }
          className="h-9 w-full rounded-md border border-input bg-card px-2.5 text-sm outline-none focus-visible:border-brand-cyan focus-visible:ring-2 focus-visible:ring-brand-cyan/35"
        >
          <option value="">Choose a folder...</option>
          {known.map((name) => (
            <option key={name} value={name}>
              {name}
              {name === suggested ? ' (suggested)' : ''}
            </option>
          ))}
          <option value={CUSTOM}>Other folder...</option>
        </select>
        {selectValue === CUSTOM ? (
          <Input
            aria-label="Custom model folder"
            autoComplete="off"
            placeholder="folder name"
            value={category.trim() === '' ? '' : category}
            onChange={(event) => onCategory(event.target.value)}
          />
        ) : null}
        {suggested && !known.includes(suggested) ? (
          <button
            type="button"
            className="w-fit text-xs underline"
            onClick={() => onCategory(suggested)}
          >
            Use suggested folder {suggested}
          </button>
        ) : null}
      </div>
      <div className="flex flex-col gap-1.5">
        <Label htmlFor={`${idPrefix}-subdirectory`}>
          Sub-folder (optional)
        </Label>
        <Input
          id={`${idPrefix}-subdirectory`}
          autoComplete="off"
          placeholder="e.g. sdxl"
          value={subdirectory}
          onChange={(event) => onSubdirectory(event.target.value)}
        />
      </div>
    </div>
  )
}
