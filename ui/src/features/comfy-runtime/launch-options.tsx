import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { TriangleAlert } from 'lucide-react'
import { useState, type FormEvent, type ReactNode } from 'react'
import { toast } from 'sonner'

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
  { value: 'auto', label: 'Automatic' },
  { value: 'low', label: 'Low VRAM' },
  { value: 'high', label: 'High VRAM' },
  { value: 'no_vram', label: 'No VRAM (offload everything)' },
] as const satisfies readonly { value: VramModeDto; label: string }[]

const ATTENTION = [
  { value: 'default', label: 'Default' },
  { value: 'sage', label: 'SageAttention (needs it installed)' },
  { value: 'flash', label: 'FlashAttention (needs it installed)' },
  { value: 'pytorch', label: 'PyTorch cross attention' },
  { value: 'split', label: 'Split cross attention' },
  { value: 'quad', label: 'Quad cross attention' },
] as const satisfies readonly { value: AttentionDto; label: string }[]

const PREVIEW = [
  { value: 'auto', label: 'Automatic' },
  { value: 'latent2_rgb', label: 'Latent to RGB' },
  { value: 'taesd', label: 'TAESD' },
  { value: 'none', label: 'None' },
] as const satisfies readonly { value: PreviewMethodDto; label: string }[]

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
  const client = useQueryClient()
  const [draft, setDraft] = useState<Draft>(() => toDraft(options))
  const [portError, setPortError] = useState<string | null>(null)
  const save = useMutation({
    mutationFn: (next: LaunchOptionsDto) => gateway.comfySetLaunchOptions(next),
    onSuccess: (saved) => {
      client.setQueryData(comfyKeys.launchOptions, saved)
      setDraft(toDraft(saved))
      toast.success('Launch options saved. They apply on the next start.')
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
      setPortError('Enter a port between 1 and 65535.')
      return
    }
    setPortError(null)
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
          The running session uses port {active.options.port}
          {active.options.listenLan ? ' on all interfaces' : ' on loopback'}.
          Changes apply on the next start.
        </p>
      ) : null}
      <div className="grid gap-4 sm:grid-cols-2">
        <Row id="launch-port" label="Port" hint="Default 8188.">
          <Input
            id="launch-port"
            inputMode="numeric"
            value={draft.port}
            aria-invalid={portError !== null}
            onChange={(event) => patch('port', event.target.value)}
          />
          {portError ? (
            <p className="text-xs text-destructive">{portError}</p>
          ) : null}
        </Row>
        <Row id="launch-vram" label="VRAM mode">
          <NativeSelect
            id="launch-vram"
            value={draft.vram}
            options={VRAM}
            onChange={(value) => patch('vram', value)}
          />
        </Row>
        <Row
          id="launch-attention"
          label="Attention backend"
          hint="Start is refused if the selected backend is not installed."
        >
          <NativeSelect
            id="launch-attention"
            value={draft.attention}
            options={ATTENTION}
            onChange={(value) => patch('attention', value)}
          />
        </Row>
        <Row id="launch-preview" label="Preview method">
          <NativeSelect
            id="launch-preview"
            value={draft.preview}
            options={PREVIEW}
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
          Enable the built-in ComfyUI Manager
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
          Listen on the local network (not only this computer)
        </Label>
      </div>
      {draft.listenLan ? (
        <Alert variant="destructive">
          <TriangleAlert />
          <AlertTitle>Network exposure</AlertTitle>
          <AlertDescription>
            <p>
              ComfyUI has no login. Anyone who can reach this computer on the
              network can run workflows and read files ComfyUI serves. Enable
              this only on a network you trust.
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
                <Label htmlFor="launch-lan-ack">I understand the risk</Label>
              </div>
            ) : null}
          </AlertDescription>
        </Alert>
      ) : null}
      <Row
        id="launch-extra"
        label="Extra arguments"
        hint="One argument per line, for example --fast or --reserve-vram=2. Reserved options such as the port, listen address and model paths are rejected."
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
          Save launch options
        </Button>
      </div>
    </form>
  )
}

export function LaunchOptionsCard({ active }: { active: SessionDto | null }) {
  const options = useQuery({
    queryKey: comfyKeys.launchOptions,
    queryFn: () => gateway.comfyGetLaunchOptions(),
  })
  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-lg font-bold tracking-tight">
          Launch options
        </CardTitle>
        <CardDescription>
          Saved for this installation and applied on the next start.
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
