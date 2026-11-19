import { lazy, Suspense, type ComponentProps } from 'react'
import { toast } from 'sonner'

import { useSettings, useSettingsSavePending, useUpdateSettings } from '@/features/settings/queries'
import {
  getLanguage,
  LANGUAGES,
  setLanguage,
  t as translate,
  useTranslation,
  type Language,
} from '@/shared/i18n'
import { IpcClientError } from '@/shared/ipc/gateway'
import { Field, FieldDescription, FieldLabel } from '@/shared/ui/field'
import { OptionsSelect } from '@/shared/ui/options-select'
import { Skeleton } from '@/shared/ui/skeleton'

const SetupWizard = lazy(() =>
  import('@/features/setup/setup-wizard').then((m) => ({ default: m.SetupWizard })),
)

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
          toast.error(
            error instanceof IpcClientError ? error.message : translate('setup.languageSaveError'),
          )
        },
      },
    )
  }

  return (
    <Suspense fallback={<Skeleton className="h-64" />}>
      <SetupWizard
        {...props}
        preferencesPending={pending}
        languageControl={(rootBusy) => {
          const disabled = rootBusy || pending || !settings.data
          return (
            <Field data-disabled={disabled}>
              <FieldLabel htmlFor="setup-language">{t('setup.language')}</FieldLabel>
              <OptionsSelect
                id="setup-language"
                className="w-full"
                value={language}
                options={LANGUAGES}
                disabled={disabled}
                aria-describedby="setup-language-hint"
                onValueChange={changeLanguage}
              />
              <FieldDescription id="setup-language-hint">
                {t('setup.languageHint')}
              </FieldDescription>
            </Field>
          )
        }}
      />
    </Suspense>
  )
}
