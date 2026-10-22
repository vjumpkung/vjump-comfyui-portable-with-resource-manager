import { useState, type FormEvent, type ReactNode } from 'react'
import { toast } from 'sonner'

import { LANGUAGES, setLanguage, t as translate, useTranslation, type Language } from '@/shared/i18n'
import type {
  CloseBehaviorDto,
  HfEngineDto,
  SettingsDto,
  ThemeDto,
} from '@/shared/ipc/gateway'
import { Button } from '@/shared/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/shared/ui/card'
import { Input } from '@/shared/ui/input'
import { Label } from '@/shared/ui/label'

import { errorMessage } from './error-message'
import { useSettings, useUpdateSettings } from './queries'
import { SelectField } from './select-field'

const HF_ENGINES = [
  { value: 'hf_cli', labelKey: 'settings.engine.hfCli' },
  { value: 'native', labelKey: 'settings.engine.native' },
] as const satisfies readonly { value: HfEngineDto; labelKey: keyof typeof import('@/shared/i18n/translations.json').en }[]

const CLOSE_BEHAVIORS = [
  { value: 'ask', labelKey: 'settings.close.ask' },
  { value: 'stop_comfy', labelKey: 'settings.close.stop' },
  { value: 'keep_running_headless', labelKey: 'settings.close.background' },
] as const satisfies readonly { value: CloseBehaviorDto; labelKey: keyof typeof import('@/shared/i18n/translations.json').en }[]

const THEMES = [
  { value: 'system', labelKey: 'settings.theme.system' },
  { value: 'light', labelKey: 'settings.theme.light' },
  { value: 'dark', labelKey: 'settings.theme.dark' },
] as const satisfies readonly { value: ThemeDto; labelKey: keyof typeof import('@/shared/i18n/translations.json').en }[]

const U32_MAX = 4_294_967_295

interface Draft {
  downloadConcurrency: string
  hfEngine: HfEngineDto
  closeBehavior: CloseBehaviorDto
  theme: ThemeDto
  language: Language
  registryCacheTtl: string
  bandwidthCap: string
}

type DraftErrors = Partial<Record<keyof Draft, `settings.validation.${'concurrency' | 'ttl' | 'bandwidth'}`>>

function toDraft(settings: SettingsDto): Draft {
  return {
    downloadConcurrency: String(settings.downloadConcurrency),
    hfEngine: settings.hfEngine,
    closeBehavior: settings.closeBehavior,
    theme: settings.theme,
    language: settings.language,
    registryCacheTtl: String(settings.registryCacheTtl),
    bandwidthCap: settings.bandwidthCapKibS === null ? '' : String(settings.bandwidthCapKibS),
  }
}

function parsePositive(value: string): number | null {
  const text = value.trim()
  if (!/^\d+$/.test(text)) return null
  const parsed = Number(text)
  return parsed >= 1 && parsed <= U32_MAX ? parsed : null
}

function Field({
  id,
  label,
  error,
  children,
}: {
  id: string
  label: string
  error?: string
  children: ReactNode
}) {
  return (
    <div className="flex flex-col gap-1.5">
      <Label htmlFor={id}>{label}</Label>
      {children}
      {error ? <p className="text-xs text-destructive">{error}</p> : null}
    </div>
  )
}

function SettingsFormFields({ settings }: { settings: SettingsDto }) {
  const { t } = useTranslation()
  const [draft, setDraft] = useState<Draft>(() => toDraft(settings))
  const [errors, setErrors] = useState<DraftErrors>({})
  const update = useUpdateSettings()

  function patch<K extends keyof Draft>(key: K, value: Draft[K]) {
    setDraft((current) => ({ ...current, [key]: value }))
  }

  function onSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const concurrency = parsePositive(draft.downloadConcurrency)
    const ttl = parsePositive(draft.registryCacheTtl)
    const capText = draft.bandwidthCap.trim()
    const cap = capText === '' ? null : parsePositive(capText)

    const nextErrors: DraftErrors = {}
    if (concurrency === null) {
      nextErrors.downloadConcurrency = 'settings.validation.concurrency'
    }
    if (ttl === null) {
      nextErrors.registryCacheTtl = 'settings.validation.ttl'
    }
    if (capText !== '' && cap === null) {
      nextErrors.bandwidthCap = 'settings.validation.bandwidth'
    }
    setErrors(nextErrors)
    if (concurrency === null || ttl === null || (capText !== '' && cap === null)) return

    update.mutate(
      {
        downloadConcurrency: concurrency,
        hfEngine: draft.hfEngine,
        closeBehavior: draft.closeBehavior,
        theme: draft.theme,
        language: draft.language,
        registryCacheTtl: ttl,
        bandwidthCapKibS: cap,
      },
      {
        onSuccess: (saved) => {
          setLanguage(saved.language)
          toast.success(translate('settings.saved'))
        },
        onError: (error) => toast.error(errorMessage(error)),
      },
    )
  }

  return (
    <form onSubmit={onSubmit} className="flex flex-col gap-5" noValidate>
      <div className="grid gap-5 md:grid-cols-2">
        <Field
          id="download-concurrency"
          label={t('settings.concurrency')}
          error={errors.downloadConcurrency && t(errors.downloadConcurrency)}
        >
          <Input
            id="download-concurrency"
            inputMode="numeric"
            value={draft.downloadConcurrency}
            onChange={(event) => patch('downloadConcurrency', event.target.value)}
            aria-invalid={errors.downloadConcurrency ? true : undefined}
          />
        </Field>
        <Field id="hf-engine" label={t('settings.hfEngine')}>
          <SelectField
            id="hf-engine"
            value={draft.hfEngine}
            options={HF_ENGINES.map((option) => ({ value: option.value, label: t(option.labelKey) }))}
            onChange={(value) => patch('hfEngine', value)}
          />
        </Field>
        <Field id="close-behavior" label={t('settings.closeBehavior')}>
          <SelectField
            id="close-behavior"
            value={draft.closeBehavior}
            options={CLOSE_BEHAVIORS.map((option) => ({ value: option.value, label: t(option.labelKey) }))}
            onChange={(value) => patch('closeBehavior', value)}
          />
        </Field>
        <Field id="theme" label={t('settings.theme')}>
          <SelectField
            id="theme"
            value={draft.theme}
            options={THEMES.map((option) => ({ value: option.value, label: t(option.labelKey) }))}
            onChange={(value) => patch('theme', value)}
          />
        </Field>
        <Field id="language" label={t('settings.language')}>
          <SelectField
            id="language"
            value={draft.language}
            options={LANGUAGES}
            onChange={(value) => patch('language', value)}
          />
        </Field>
        <Field
          id="registry-ttl"
          label={t('settings.registryTtl')}
          error={errors.registryCacheTtl && t(errors.registryCacheTtl)}
        >
          <Input
            id="registry-ttl"
            inputMode="numeric"
            value={draft.registryCacheTtl}
            onChange={(event) => patch('registryCacheTtl', event.target.value)}
            aria-invalid={errors.registryCacheTtl ? true : undefined}
          />
        </Field>
        <Field id="bandwidth-cap" label={t('settings.bandwidth')} error={errors.bandwidthCap && t(errors.bandwidthCap)}>
          <Input
            id="bandwidth-cap"
            inputMode="numeric"
            placeholder={t('common.unlimited')}
            value={draft.bandwidthCap}
            onChange={(event) => patch('bandwidthCap', event.target.value)}
            aria-invalid={errors.bandwidthCap ? true : undefined}
            aria-describedby="bandwidth-cap-note"
          />
          <p id="bandwidth-cap-note" className="text-xs text-muted-foreground">
            {t('settings.bandwidthHint')}
          </p>
        </Field>
      </div>
      <div>
        <Button type="submit" disabled={update.isPending}>
          {t('settings.save')}
        </Button>
      </div>
    </form>
  )
}

export function SettingsForm() {
  const { t } = useTranslation()
  const settings = useSettings()

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-lg font-bold tracking-tight">{t('settings.preferences')}</CardTitle>
        <CardDescription>{t('settings.storageHint')}</CardDescription>
      </CardHeader>
      <CardContent>
        {settings.data ? (
          // Re-key on the stored values so the draft resets after each successful save.
          <SettingsFormFields key={JSON.stringify(settings.data)} settings={settings.data} />
        ) : settings.isError ? (
          <p className="text-sm text-destructive">{errorMessage(settings.error)}</p>
        ) : (
          <p className="text-sm text-muted-foreground">{t('settings.loading')}</p>
        )}
      </CardContent>
    </Card>
  )
}
