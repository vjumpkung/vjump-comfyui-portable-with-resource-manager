import { useTranslation } from '@/shared/i18n'
import { Input } from '@/shared/ui/input'
import { Label } from '@/shared/ui/label'
import { OptionsSelect } from '@/shared/ui/options-select'

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
        <OptionsSelect
          id={`${idPrefix}-category`}
          className="w-full"
          value={selectValue}
          placeholder={t('downloads.chooseFolder')}
          options={[
            ...known.map((name) => ({
              value: name,
              label: name === suggested ? t('downloads.suggestedFolder', { folder: name }) : name,
            })),
            { value: CUSTOM, label: t('downloads.otherFolder') },
          ]}
          onValueChange={(value) => onCategory(value === CUSTOM ? ' ' : value)}
        />
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
            className="w-fit text-xs underline decoration-pink-ink underline-offset-4 hover:decoration-2"
            onClick={() => onCategory(suggested)}
          >
            {t('downloads.useSuggestedFolder', { folder: suggested })}
          </button>
        ) : null}
      </div>
      <div className="flex flex-col gap-1.5">
        <Label htmlFor={`${idPrefix}-subdirectory`}>{t('downloads.subfolder')}</Label>
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
