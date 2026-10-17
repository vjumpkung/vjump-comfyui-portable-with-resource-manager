import { useState, type FormEvent, type ReactNode } from 'react'
import { toast } from 'sonner'

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
  { value: 'hf_cli', label: 'hf CLI (default)' },
  { value: 'native', label: 'Native downloader' },
] as const satisfies readonly { value: HfEngineDto; label: string }[]

const CLOSE_BEHAVIORS = [
  { value: 'ask', label: 'Ask every time' },
  { value: 'stop_comfy', label: 'Stop ComfyUI and quit' },
  { value: 'keep_running_headless', label: 'Keep ComfyUI running in the background' },
] as const satisfies readonly { value: CloseBehaviorDto; label: string }[]

const THEMES = [
  { value: 'system', label: 'System' },
  { value: 'light', label: 'Light' },
  { value: 'dark', label: 'Dark' },
] as const satisfies readonly { value: ThemeDto; label: string }[]

const U32_MAX = 4_294_967_295

interface Draft {
  downloadConcurrency: string
  hfEngine: HfEngineDto
  closeBehavior: CloseBehaviorDto
  theme: ThemeDto
  registryCacheTtl: string
  bandwidthCap: string
}

type DraftErrors = Partial<Record<keyof Draft, string>>

function toDraft(settings: SettingsDto): Draft {
  return {
    downloadConcurrency: String(settings.downloadConcurrency),
    hfEngine: settings.hfEngine,
    closeBehavior: settings.closeBehavior,
    theme: settings.theme,
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
      nextErrors.downloadConcurrency = 'Enter a whole number of 1 or more.'
    }
    if (ttl === null) {
      nextErrors.registryCacheTtl = 'Enter a whole number of hours, 1 or more.'
    }
    if (capText !== '' && cap === null) {
      nextErrors.bandwidthCap = 'Enter KiB/s as 1 or more, or leave it empty.'
    }
    setErrors(nextErrors)
    if (concurrency === null || ttl === null || (capText !== '' && cap === null)) return

    update.mutate(
      {
        downloadConcurrency: concurrency,
        hfEngine: draft.hfEngine,
        closeBehavior: draft.closeBehavior,
        theme: draft.theme,
        registryCacheTtl: ttl,
        bandwidthCapKibS: cap,
      },
      {
        onSuccess: () => toast.success('Settings saved.'),
        onError: (error) => toast.error(errorMessage(error)),
      },
    )
  }

  return (
    <form onSubmit={onSubmit} className="flex flex-col gap-5" noValidate>
      <div className="grid gap-5 md:grid-cols-2">
        <Field
          id="download-concurrency"
          label="Parallel downloads"
          error={errors.downloadConcurrency}
        >
          <Input
            id="download-concurrency"
            inputMode="numeric"
            value={draft.downloadConcurrency}
            onChange={(event) => patch('downloadConcurrency', event.target.value)}
            aria-invalid={errors.downloadConcurrency ? true : undefined}
          />
        </Field>
        <Field id="hf-engine" label="Hugging Face download engine">
          <SelectField
            id="hf-engine"
            value={draft.hfEngine}
            options={HF_ENGINES}
            onChange={(value) => patch('hfEngine', value)}
          />
        </Field>
        <Field id="close-behavior" label="When closing the window">
          <SelectField
            id="close-behavior"
            value={draft.closeBehavior}
            options={CLOSE_BEHAVIORS}
            onChange={(value) => patch('closeBehavior', value)}
          />
        </Field>
        <Field id="theme" label="Theme">
          <SelectField
            id="theme"
            value={draft.theme}
            options={THEMES}
            onChange={(value) => patch('theme', value)}
          />
        </Field>
        <Field
          id="registry-ttl"
          label="Registry cache lifetime (hours)"
          error={errors.registryCacheTtl}
        >
          <Input
            id="registry-ttl"
            inputMode="numeric"
            value={draft.registryCacheTtl}
            onChange={(event) => patch('registryCacheTtl', event.target.value)}
            aria-invalid={errors.registryCacheTtl ? true : undefined}
          />
        </Field>
        <Field id="bandwidth-cap" label="Bandwidth cap (KiB/s)" error={errors.bandwidthCap}>
          <Input
            id="bandwidth-cap"
            inputMode="numeric"
            placeholder="Unlimited"
            value={draft.bandwidthCap}
            onChange={(event) => patch('bandwidthCap', event.target.value)}
            aria-invalid={errors.bandwidthCap ? true : undefined}
            aria-describedby="bandwidth-cap-note"
          />
          <p id="bandwidth-cap-note" className="text-xs text-muted-foreground">
            Shared by all downloads. Leave empty for unlimited speed.
          </p>
        </Field>
      </div>
      <div>
        <Button type="submit" disabled={update.isPending}>
          Save settings
        </Button>
      </div>
    </form>
  )
}

export function SettingsForm() {
  const settings = useSettings()

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-lg font-bold tracking-tight">Preferences</CardTitle>
        <CardDescription>Stored locally in the application database.</CardDescription>
      </CardHeader>
      <CardContent>
        {settings.data ? (
          // Re-key on the stored values so the draft resets after each successful save.
          <SettingsFormFields key={JSON.stringify(settings.data)} settings={settings.data} />
        ) : settings.isError ? (
          <p className="text-sm text-destructive">{errorMessage(settings.error)}</p>
        ) : (
          <p className="text-sm text-muted-foreground">Loading settings...</p>
        )}
      </CardContent>
    </Card>
  )
}
