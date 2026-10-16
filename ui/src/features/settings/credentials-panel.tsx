import { useMutation, useQueryClient } from '@tanstack/react-query'
import { KeyRound, ShieldAlert } from 'lucide-react'
import { useRef, useState, type FormEvent } from 'react'
import { toast } from 'sonner'

import {
  gateway,
  type CredentialKindDto,
  type CredentialStatusDto,
  type CredentialTestResultDto,
} from '@/shared/ipc/gateway'
import { Badge } from '@/shared/ui/badge'
import { Button } from '@/shared/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/shared/ui/card'
import { Input } from '@/shared/ui/input'
import { Label } from '@/shared/ui/label'
import { Separator } from '@/shared/ui/separator'

import { errorMessage } from './error-message'
import { credentialStatusKey, useClearCredential, useCredentialStatus } from './queries'

const KINDS: readonly { kind: CredentialKindDto; label: string }[] = [
  { kind: 'civitAi', label: 'CivitAI' },
  { kind: 'huggingFace', label: 'Hugging Face' },
]

const TEST_RESULTS: Record<CredentialTestResultDto, string> = {
  valid: 'Token accepted',
  invalid: 'Token rejected or lacks access',
  absent: 'No token to test',
  backend_unavailable: 'Credential storage unavailable',
  backend_locked: 'Credential storage locked',
  network_failure: 'Could not reach the provider',
}

function describeTest(status: CredentialStatusDto | undefined): string | null {
  const test = status?.lastTest
  if (!test) return null
  const when = new Date((test.testedAt ?? 0) * 1000).toLocaleString()
  return `${TEST_RESULTS[test.result]} · tested ${when}`
}

function CredentialRow({
  kind,
  label,
  status,
}: {
  kind: CredentialKindDto
  label: string
  status: CredentialStatusDto | undefined
}) {
  const queryClient = useQueryClient()
  const clear = useClearCredential()
  const test = useMutation({
    mutationFn: () => gateway.secretTest(kind),
    onSuccess: (result) => {
      if (result.result === 'valid') toast.success(`${label} token accepted.`)
      else toast.error(TEST_RESULTS[result.result] + '.')
    },
    onError: (error) => toast.error(errorMessage(error)),
    onSettled: () =>
      queryClient.invalidateQueries({ queryKey: credentialStatusKey(kind) }),
  })
  const inputRef = useRef<HTMLInputElement>(null)
  const [saving, setSaving] = useState(false)
  const inputId = `credential-${kind}`

  async function onSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const input = inputRef.current
    if (!input) return
    const value = input.value.trim()
    // The secret lives only in this local variable and the uncontrolled input.
    input.value = ''
    if (value === '') return
    setSaving(true)
    try {
      await gateway.secretSet(kind, value)
      toast.success(`${label} token saved.`)
    } catch (error) {
      toast.error(errorMessage(error))
    } finally {
      setSaving(false)
      await queryClient.invalidateQueries({ queryKey: credentialStatusKey(kind) })
    }
  }

  const isSet = status?.isSet === true

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-2">
        <KeyRound className="size-4 text-muted-foreground" aria-hidden="true" />
        <Label htmlFor={inputId} className="text-sm font-semibold">
          {label} token
        </Label>
        <Badge
          variant={isSet ? 'default' : 'outline'}
          className="font-mono text-[9px] uppercase tracking-widest"
        >
          {status ? (isSet ? 'Set' : 'Not set') : '...'}
        </Badge>
        {status ? (
          <span className="font-mono text-xs text-muted-foreground">
            {status.backend === 'keyring' ? 'OS keyring' : 'Session only'}
          </span>
        ) : null}
      </div>
      <form
        onSubmit={(event) => void onSubmit(event)}
        className="flex flex-wrap items-center gap-2"
        autoComplete="off"
      >
        <Input
          id={inputId}
          ref={inputRef}
          type="password"
          name={inputId}
          autoComplete="off"
          spellCheck={false}
          placeholder={isSet ? 'Enter a new token to replace it' : 'Paste token'}
          className="max-w-md flex-1"
        />
        <Button type="submit" disabled={saving}>
          Set
        </Button>
        <Button
          type="button"
          variant="outline"
          disabled={!isSet || test.isPending}
          onClick={() => test.mutate()}
        >
          {test.isPending ? 'Testing...' : 'Test'}
        </Button>
        <Button
          type="button"
          variant="outline"
          disabled={!isSet || clear.isPending}
          onClick={() =>
            clear.mutate(kind, {
              onSuccess: () => toast.success(`${label} token cleared.`),
              onError: (error) => toast.error(errorMessage(error)),
            })
          }
        >
          Clear
        </Button>
      </form>
      {describeTest(status) ? (
        <p className="font-mono text-xs text-muted-foreground">
          {describeTest(status)}
        </p>
      ) : null}
    </div>
  )
}

export function CredentialsPanel() {
  const civitAi = useCredentialStatus('civitAi')
  const huggingFace = useCredentialStatus('huggingFace')
  const statuses = { civitAi: civitAi.data, huggingFace: huggingFace.data }
  const sessionOnly =
    civitAi.data?.sessionOnlyWarning === true || huggingFace.data?.sessionOnlyWarning === true

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-lg font-bold tracking-tight">Access tokens</CardTitle>
        <CardDescription>
          Tokens are stored in the OS keyring and are never written to the database or logs.
          Test checks a saved token with the provider and shows only whether it is accepted.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-5">
        {sessionOnly ? (
          <div
            role="alert"
            className="flex items-start gap-2 rounded-lg border border-brand-pink bg-brand-pink/20 p-3 text-sm"
          >
            <ShieldAlert className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
            <p>
              The OS keyring is unavailable, so tokens are kept in memory for this session only and
              will be forgotten when the app closes.
            </p>
          </div>
        ) : null}
        {KINDS.map(({ kind, label }, index) => (
          <div key={kind} className="flex flex-col gap-5">
            {index > 0 ? <Separator /> : null}
            <CredentialRow kind={kind} label={label} status={statuses[kind]} />
          </div>
        ))}
      </CardContent>
    </Card>
  )
}
