import { useTranslation } from '@/shared/i18n'
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
  const { t } = useTranslation()
  const isKnown = known.includes(category)
  const selectValue = category === '' ? '' : isKnown ? category : CUSTOM
  return (
    <div className="grid gap-4 sm:grid-cols-2">
      <div className="flex flex-col gap-1.5">
        <Label htmlFor={`${idPrefix}-category`}>{t('downloads.modelFolder')}</Label>
        <select
          id={`${idPrefix}-category`}
          value={selectValue}
          onChange={(event) =>
            onCategory(event.target.value === CUSTOM ? ' ' : event.target.value)
          }
          className="h-9 w-full rounded-md border border-input bg-card px-2.5 text-sm outline-none focus-visible:border-brand-cyan focus-visible:ring-2 focus-visible:ring-brand-cyan/35"
        >
          <option value="">{t('downloads.chooseFolder')}</option>
          {known.map((name) => (
            <option key={name} value={name}>
              {name === suggested ? t('downloads.suggestedFolder', { folder: name }) : name}
            </option>
          ))}
          <option value={CUSTOM}>{t('downloads.otherFolder')}</option>
        </select>
        {selectValue === CUSTOM ? (
          <Input
            aria-label={t('downloads.customFolder')}
            autoComplete="off"
            placeholder={t('downloads.folderName')}
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
            {t('downloads.useSuggestedFolder', { folder: suggested })}
          </button>
        ) : null}
      </div>
      <div className="flex flex-col gap-1.5">
        <Label htmlFor={`${idPrefix}-subdirectory`}>
          {t('downloads.subfolder')}
        </Label>
        <Input
          id={`${idPrefix}-subdirectory`}
          autoComplete="off"
          placeholder={t('downloads.subfolderExample')}
          value={subdirectory}
          onChange={(event) => onSubdirectory(event.target.value)}
        />
      </div>
    </div>
  )
}
