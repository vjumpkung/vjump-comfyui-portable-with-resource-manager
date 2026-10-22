import type { ComponentProps } from 'react'
import { toast } from 'sonner'

import { useSettings, useSettingsSavePending, useUpdateSettings } from '@/features/settings'
import { SetupWizard } from '@/features/setup'
import { getLanguage, isLanguage, LANGUAGES, setLanguage, t as translate, useTranslation, type Language } from '@/shared/i18n'
import { IpcClientError } from '@/shared/ipc/gateway'
import { Field, FieldDescription, FieldLabel } from '@/shared/ui/field'

type Props = Omit<ComponentProps<typeof SetupWizard>, 'languageControl' | 'preferencesPending'>

export function SetupWizardPreferences(props: Props) {
  const settings = useSettings()
  const update = useUpdateSettings()
  const pending = useSettingsSavePending()
  const { language, t } = useTranslation()

  function changeLanguage(next: Language) {
    if (!settings.data || pending) return
    const previous = getLanguage()
    setLanguage(next)
    update.mutate(
      { ...settings.data, language: next },
      {
        onSuccess: (saved) => setLanguage(saved.language),
        onError: (error) => {
          setLanguage(previous)
          toast.error(error instanceof IpcClientError ? error.message : translate('setup.languageSaveError'))
        },
      },
    )
  }

  return <SetupWizard {...props} preferencesPending={pending}
    languageControl={(rootBusy) => {
      const disabled = rootBusy || pending || !settings.data
      return <Field data-disabled={disabled}>
        <FieldLabel htmlFor="setup-language">{t('setup.language')}</FieldLabel>
        <select
          id="setup-language"
          value={language}
          disabled={disabled}
          aria-describedby="setup-language-hint"
          onChange={(event) => {
            if (isLanguage(event.target.value)) changeLanguage(event.target.value)
          }}
          className="h-9 w-full rounded-md border border-input bg-card px-2.5 text-sm outline-none focus-visible:border-brand-cyan focus-visible:ring-2 focus-visible:ring-brand-cyan/35"
        >
          {LANGUAGES.map((option) => <option key={option.value} value={option.value}>
            {option.label}
          </option>)}
        </select>
        <FieldDescription id="setup-language-hint">{t('setup.languageHint')}</FieldDescription>
      </Field>
    }} />
}
