import { useId, useState, type ReactNode } from 'react'

import type { CommitInput, PlanReviewDto } from '@/shared/ipc/gateway'
import { Alert, AlertDescription, AlertTitle } from '@/shared/ui/alert'
import { Button } from '@/shared/ui/button'
import { Checkbox } from '@/shared/ui/checkbox'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/shared/ui/dialog'
import { Field, FieldGroup, FieldLabel, FieldLegend, FieldSet } from '@/shared/ui/field'

import {
  canCommit,
  decision,
  EMPTY_CONSENT,
  hasTrustNamespace,
  normalizeConsent,
  reviewKey,
  sourceLabel,
  type Consent,
} from './model'

function PlanConsent({ plan, value, onChange, allowRememberSource }: {
  plan: PlanReviewDto
  value: Consent
  onChange: (value: Consent) => void
  allowRememberSource: boolean
}) {
  const prefix = useId()
  const namespace = hasTrustNamespace(plan.source)
  function check(key: keyof Consent, label: string) {
    const id = `${prefix}-${key}`
    return <Field orientation="horizontal">
      <Checkbox id={id} checked={value[key]}
        onCheckedChange={(checked) => onChange(normalizeConsent(plan, { ...value, [key]: checked === true }))} />
      <FieldLabel htmlFor={id}>{label}</FieldLabel>
    </Field>
  }
  return <FieldSet>
    <FieldLegend>{plan.name}</FieldLegend>
    <dl className="flex flex-col gap-1 break-all font-mono text-xs">
      <div>
        <dt className="text-muted-foreground">Resolved source</dt>
        <dd>{sourceLabel(plan.source)}</dd>
      </div>
      {plan.priorSource && <div>
        <dt className="text-muted-foreground">Previous source</dt>
        <dd>{sourceLabel(plan.priorSource)}</dd>
      </div>}
      <div>
        <dt className="text-muted-foreground">Tree identity</dt>
        <dd>{plan.treeDigest}</dd>
      </div>
      {plan.artifactDigest && <div>
        <dt className="text-muted-foreground">Artifact SHA-256</dt>
        <dd>{plan.artifactDigest}</dd>
      </div>}
      {plan.hookDigest && <div>
        <dt className="text-muted-foreground">install.py SHA-256</dt>
        <dd>{plan.hookDigest}</dd>
      </div>}
      <div>
        <dt className="text-muted-foreground">Registry dependencies and requirements.txt</dt>
        <dd>
          {plan.hasRequirements ? 'Includes Registry dependencies or requirements.txt' : 'None'}
          {plan.requirements.length > 0 && <ul className="mt-1 max-h-40 overflow-y-auto rounded-md border p-2">
            {plan.requirements.map((requirement, index) => <li key={index}>{requirement}</li>)}
          </ul>}
        </dd>
      </div>
    </dl>
    {plan.warnings.length > 0 && <Alert>
      <AlertTitle>Compatibility and source warnings</AlertTitle>
      <AlertDescription><ul className="break-words">{plan.warnings.map((warning, index) => <li key={index}>{warning}</li>)}</ul></AlertDescription>
    </Alert>}
    {plan.protectedChanges.length > 0 && <Alert variant="destructive">
      <AlertTitle>Protected packages will change</AlertTitle>
      <AlertDescription><ul>{plan.protectedChanges.map((change) => <li key={change.name} className="font-mono text-xs">
        {change.name}: {change.from ?? 'absent'} → {change.to ?? 'removed'}
      </li>)}</ul></AlertDescription>
    </Alert>}
    {plan.trustedSource && <p className="text-sm">
      This source was trusted before. Each plan still needs your explicit execution consent, and changed targets and warnings are still checked.
    </p>}
    {plan.trustedInstallPy && <p className="text-xs text-muted-foreground">
      Script permission was previously remembered. This review still starts with script execution unchecked.
    </p>}
    <FieldGroup>
      {check('execute', 'I allow this exact source to execute third-party code')}
      {plan.hasInstallPy && check('hook', 'Run the root install.py script with the selected Python environment')}
      {plan.warnings.length > 0 && check('warnings', 'I acknowledge all displayed warnings')}
      {plan.protectedChanges.length > 0 && check('pins', 'I approve exactly these protected-package changes')}
      {allowRememberSource && namespace && check('remember', 'Remember this source as trusted')}
      {allowRememberSource && namespace && plan.hasInstallPy && value.hook && value.remember &&
        check('rememberHook', 'Also remember permission to run install.py for this source')}
    </FieldGroup>
  </FieldSet>
}

export interface ReviewDialogProps {
  plans: PlanReviewDto[]
  busy: boolean
  onCommit: (items: CommitInput[]) => void
  onClose: () => void
  onCancel: (() => void) | null
  /** A failure to show inside the dialog, because the page behind a modal is not reachable. */
  notice?: ReactNode
  title?: string
  commitLabel?: string
  closeLabel?: string
  allowRememberSource?: boolean
}

/** All consent lives here; the session is keyed by the reviewed plans, so it never outlives them. */
function ReviewSession({ plans, busy, onCommit, onClose, onCancel, notice,
  title, commitLabel = 'Apply reviewed plans', closeLabel = 'Discard plans',
  allowRememberSource = true }: ReviewDialogProps) {
  const [consents, setConsents] = useState<Record<string, Consent>>({})
  const consentOf = (plan: PlanReviewDto) => consents[plan.id] ?? EMPTY_CONSENT
  const ready = plans.length > 0 && plans.every((plan) => canCommit(plan, consentOf(plan)))
  return <>
    <DialogHeader>
      <DialogTitle>{title ?? `Review ${plans.length > 1 ? `${plans.length} node updates` : 'custom node plan'}`}</DialogTitle>
      <DialogDescription>
        Custom nodes, Python dependencies and install scripts execute third-party code. A venv is not a sandbox.
        Confirm only sources you trust. Dependency changes cannot be rolled back; a failure requires environment repair.
      </DialogDescription>
    </DialogHeader>
    {notice}
    <fieldset disabled={busy} className="flex min-w-0 flex-col gap-6">
      {plans.map((plan) => <PlanConsent key={reviewKey(plan)} plan={plan} value={consentOf(plan)} allowRememberSource={allowRememberSource}
        onChange={(value) => setConsents((current) => ({ ...current, [plan.id]: value }))} />)}
    </fieldset>
    <DialogFooter>
      {busy && onCancel && <Button variant="outline" onClick={onCancel}>Request cancellation</Button>}
      <Button variant="outline" disabled={busy} onClick={onClose}>{closeLabel}</Button>
      <Button disabled={busy || !ready} onClick={() => onCommit(plans.map((plan) => decision(plan,
        allowRememberSource ? consentOf(plan) : { ...consentOf(plan), remember: false, rememberHook: false })))}>
        {busy ? 'Applying and verifying…' : commitLabel}
      </Button>
    </DialogFooter>
  </>
}

export function ReviewDialog(props: ReviewDialogProps) {
  const { plans, busy, onClose, allowRememberSource = true } = props
  // A changed plan ID or binding starts a new session: consent never carries over, and does not
  // return when an earlier binding reappears.
  const session = JSON.stringify([allowRememberSource, plans.map(reviewKey)])
  return <Dialog open onOpenChange={(open) => { if (!open && !busy) onClose() }}>
    <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-2xl" showCloseButton={!busy}>
      <ReviewSession key={session} {...props} />
    </DialogContent>
  </Dialog>
}
