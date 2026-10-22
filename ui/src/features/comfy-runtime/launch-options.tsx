import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { TriangleAlert } from 'lucide-react'
import { useState, type FormEvent, type ReactNode } from 'react'
import { toast } from 'sonner'

import { t as translate, useTranslation } from '@/shared/i18n'
import {
  gateway,
  type AttentionDto,
  type LaunchOptionsDto,
  type PreviewMethodDto,
  type SessionDto,
  type VramModeDto,
} from '@/shared/ipc/gateway'
import { errorMessage } from '@/shared/lib/display'
import { Alert, AlertDescription, AlertTitle } from '@/shared/ui/alert'
import { Button } from '@/shared/ui/button'
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/shared/ui/card'
import { Checkbox } from '@/shared/ui/checkbox'
import { ErrorNotice } from '@/shared/ui/feedback'
import { Input } from '@/shared/ui/input'
import { Label } from '@/shared/ui/label'
import { Skeleton } from '@/shared/ui/skeleton'

import { NativeSelect } from './native-select'
import { comfyKeys } from './use-runtime'

const VRAM = [
  { value: 'auto', labelKey: 'runtime.launch.automatic' },
  { value: 'low', labelKey: 'runtime.launch.lowVram' },
  { value: 'high', labelKey: 'runtime.launch.highVram' },
  { value: 'no_vram', labelKey: 'runtime.launch.noVram' },
] as const satisfies readonly { value: VramModeDto; labelKey: keyof typeof import('@/shared/i18n/translations.json').en }[]

const ATTENTION = [
  { value: 'default', labelKey: 'runtime.launch.default' },
  { value: 'sage', labelKey: 'runtime.launch.sage' },
  { value: 'flash', labelKey: 'runtime.launch.flash' },
  { value: 'pytorch', labelKey: 'runtime.launch.pytorch' },
  { value: 'split', labelKey: 'runtime.launch.split' },
  { value: 'quad', labelKey: 'runtime.launch.quad' },
] as const satisfies readonly { value: AttentionDto; labelKey: keyof typeof import('@/shared/i18n/translations.json').en }[]

const PREVIEW = [
  { value: 'auto', labelKey: 'runtime.launch.automatic' },
  { value: 'latent2_rgb', labelKey: 'runtime.launch.latent' },
  { value: 'taesd', labelKey: 'runtime.launch.taesd' },
  { value: 'none', labelKey: 'common.none' },
] as const satisfies readonly { value: PreviewMethodDto; labelKey: keyof typeof import('@/shared/i18n/translations.json').en }[]

interface Draft {
  port: string
  vram: VramModeDto
  attention: AttentionDto
  preview: PreviewMethodDto
  enableManager: boolean
  listenLan: boolean
  /** One launch argument per line, so a path with spaces stays one token. */
  extraArgs: string
  lanAcknowledged: boolean
}

function toDraft(options: LaunchOptionsDto): Draft {
  return {
    port: String(options.port),
    vram: options.vram,
    attention: options.attention,
    preview: options.preview,
    enableManager: options.enableManager,
    listenLan: options.listenLan,
    extraArgs: options.extraArgs.join('\n'),
    lanAcknowledged: options.listenLan,
  }
}

export function parseExtraArgs(text: string): string[] {
  return text
    .split('\n')
    .map((line) => line.replace(/\r$/, ''))
    .filter((line) => line.trim() !== '')
}

function Row({
  id,
  label,
  hint,
  children,
}: {
  id: string
  label: string
  hint?: string
  children: ReactNode
}) {
  return (
    <div className="flex flex-col gap-1.5">
      <Label htmlFor={id}>{label}</Label>
      {children}
      {hint ? <p className="text-xs text-muted-foreground">{hint}</p> : null}
    </div>
  )
}

function Form({
  options,
  active,
}: {
  options: LaunchOptionsDto
  active: SessionDto | null
}) {
  const { t } = useTranslation()
  const client = useQueryClient()
  const [draft, setDraft] = useState<Draft>(() => toDraft(options))
  const [portInvalid, setPortInvalid] = useState(false)
  const save = useMutation({
    mutationFn: (next: LaunchOptionsDto) => gateway.comfySetLaunchOptions(next),
    onSuccess: (saved) => {
      client.setQueryData(comfyKeys.launchOptions, saved)
      setDraft(toDraft(saved))
      toast.success(translate('runtime.launch.saved'))
    },
    onError: (error) => toast.error(errorMessage(error)),
  })
  function patch<K extends keyof Draft>(key: K, value: Draft[K]) {
    setDraft((current) => ({ ...current, [key]: value }))
  }
  const lanBlocked =
    draft.listenLan && !options.listenLan && !draft.lanAcknowledged

  function onSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const text = draft.port.trim()
    const port = /^\d+$/.test(text) ? Number(text) : 0
    if (port < 1 || port > 65535) {
      setPortInvalid(true)
      return
    }
    setPortInvalid(false)
    save.mutate({
      port,
      vram: draft.vram,
      attention: draft.attention,
      preview: draft.preview,
      enableManager: draft.enableManager,
      listenLan: draft.listenLan,
      extraArgs: parseExtraArgs(draft.extraArgs),
    })
  }

  return (
    <form onSubmit={onSubmit} className="flex flex-col gap-4">
      {active ? (
        <p className="text-xs text-muted-foreground">
          {t(active.options.listenLan ? 'runtime.launch.activeAll' : 'runtime.launch.activeLoopback', { port: active.options.port })}
        </p>
      ) : null}
      <div className="grid gap-4 sm:grid-cols-2">
        <Row id="launch-port" label={t('runtime.launch.port')} hint={t('runtime.launch.portHint')}>
          <Input
            id="launch-port"
            inputMode="numeric"
            value={draft.port}
            aria-invalid={portInvalid}
            onChange={(event) => patch('port', event.target.value)}
          />
          {portInvalid ? (
            <p className="text-xs text-destructive">{t('runtime.launch.portError')}</p>
          ) : null}
        </Row>
        <Row id="launch-vram" label={t('runtime.launch.vram')}>
          <NativeSelect
            id="launch-vram"
            value={draft.vram}
            options={VRAM.map(({ value, labelKey }) => ({ value, label: t(labelKey) }))}
            onChange={(value) => patch('vram', value)}
          />
        </Row>
        <Row
          id="launch-attention"
          label={t('runtime.launch.attention')}
          hint={t('runtime.launch.attentionHint')}
        >
          <NativeSelect
            id="launch-attention"
            value={draft.attention}
            options={ATTENTION.map(({ value, labelKey }) => ({ value, label: t(labelKey) }))}
            onChange={(value) => patch('attention', value)}
          />
        </Row>
        <Row id="launch-preview" label={t('runtime.launch.preview')}>
          <NativeSelect
            id="launch-preview"
            value={draft.preview}
            options={PREVIEW.map(({ value, labelKey }) => ({ value, label: t(labelKey) }))}
            onChange={(value) => patch('preview', value)}
          />
        </Row>
      </div>
      <div className="flex items-center gap-2">
        <Checkbox
          id="launch-manager"
          checked={draft.enableManager}
          onCheckedChange={(checked) =>
            patch('enableManager', checked === true)
          }
        />
        <Label htmlFor="launch-manager">
          {t('runtime.launch.manager')}
        </Label>
      </div>
      <div className="flex items-center gap-2">
        <Checkbox
          id="launch-lan"
          checked={draft.listenLan}
          onCheckedChange={(checked) => {
            patch('listenLan', checked === true)
            if (checked !== true) patch('lanAcknowledged', false)
          }}
        />
        <Label htmlFor="launch-lan">
          {t('runtime.launch.lan')}
        </Label>
      </div>
      {draft.listenLan ? (
        <Alert variant="destructive">
          <TriangleAlert />
          <AlertTitle>{t('runtime.launch.exposure')}</AlertTitle>
          <AlertDescription>
            <p>
              {t('runtime.launch.exposureHint')}
            </p>
            {!options.listenLan ? (
              <div className="flex items-center gap-2">
                <Checkbox
                  id="launch-lan-ack"
                  checked={draft.lanAcknowledged}
                  onCheckedChange={(checked) =>
                    patch('lanAcknowledged', checked === true)
                  }
                />
                <Label htmlFor="launch-lan-ack">{t('runtime.launch.riskAck')}</Label>
              </div>
            ) : null}
          </AlertDescription>
        </Alert>
      ) : null}
      <Row
        id="launch-extra"
        label={t('runtime.launch.extra')}
        hint={t('runtime.launch.extraHint')}
      >
        <textarea
          id="launch-extra"
          rows={3}
          spellCheck={false}
          value={draft.extraArgs}
          onChange={(event) => patch('extraArgs', event.target.value)}
          className="w-full rounded-md border border-input bg-card px-2.5 py-2 font-mono text-xs outline-none focus-visible:border-brand-cyan focus-visible:ring-2 focus-visible:ring-brand-cyan/35"
        />
      </Row>
      <div>
        <Button type="submit" disabled={save.isPending || lanBlocked}>
          {t('runtime.launch.save')}
        </Button>
      </div>
    </form>
  )
}

export function LaunchOptionsCard({ active }: { active: SessionDto | null }) {
  const { t } = useTranslation()
  const options = useQuery({
    queryKey: comfyKeys.launchOptions,
    queryFn: () => gateway.comfyGetLaunchOptions(),
  })
  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-lg font-bold tracking-tight">
          {t('runtime.launch.title')}
        </CardTitle>
        <CardDescription>
          {t('runtime.launch.hint')}
        </CardDescription>
      </CardHeader>
      <CardContent>
        {options.data ? (
          <Form
            // Remount when the saved record changes from outside the form.
            key={JSON.stringify(options.data)}
            options={options.data}
            active={active}
          />
        ) : options.isError ? (
          <ErrorNotice error={options.error} retry={() => void options.refetch()} />
        ) : (
          <Skeleton className="h-48" />
        )}
      </CardContent>
    </Card>
  )
}
