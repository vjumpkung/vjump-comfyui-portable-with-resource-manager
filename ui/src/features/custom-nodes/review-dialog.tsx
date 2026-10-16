import { useState } from 'react'

import type { CommitInput, PlanReviewDto } from '@/shared/ipc/gateway'
import { Alert, AlertDescription, AlertTitle } from '@/shared/ui/alert'
import { Button } from '@/shared/ui/button'
import { Checkbox } from '@/shared/ui/checkbox'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/shared/ui/dialog'
import { Field, FieldGroup, FieldLabel, FieldLegend, FieldSet } from '@/shared/ui/field'

import { canCommit, decision, EMPTY_CONSENT, sourceLabel, type Consent } from './model'

function PlanConsent({ plan, value, onChange }: {
  plan: PlanReviewDto
  value: Consent
  onChange: (value: Consent) => void
}) {
  function check(key: keyof Consent, label: string) {
    const id = `${plan.id}-${key}`
    return <Field orientation="horizontal">
      <Checkbox id={id} checked={value[key]} onCheckedChange={(checked) => onChange({ ...value, [key]: checked === true })} />
      <FieldLabel htmlFor={id}>{label}</FieldLabel>
    </Field>
  }
  return <FieldSet>
    <FieldLegend>{plan.name}</FieldLegend>
    <p className="break-all font-mono text-xs">{sourceLabel(plan.source)}</p>
    {plan.priorSource && <p className="break-all font-mono text-xs text-muted-foreground">Previously: {sourceLabel(plan.priorSource)}</p>}
    <details className="text-sm">
      <summary>Source integrity and requirements</summary>
      <div className="flex flex-col gap-1 break-all font-mono text-xs">
        <p>Tree SHA-256: {plan.treeDigest}</p>
        {plan.artifactDigest && <p>Artifact SHA-256: {plan.artifactDigest}</p>}
        {plan.hookDigest && <p>install.py SHA-256: {plan.hookDigest}</p>}
        <p>{plan.hasRequirements ? 'Includes root requirements.txt' : 'No root requirements.txt'}</p>
        {plan.requirements.map((requirement, index) => <p key={index}>{requirement}</p>)}
      </div>
    </details>
    {plan.warnings.length > 0 && <Alert>
      <AlertTitle>Compatibility and source warnings</AlertTitle>
      <AlertDescription><ul>{plan.warnings.map((warning, index) => <li key={index}>{warning}</li>)}</ul></AlertDescription>
    </Alert>}
    {plan.protectedChanges.length > 0 && <Alert variant="destructive">
      <AlertTitle>Protected packages will change</AlertTitle>
      <AlertDescription><ul>{plan.protectedChanges.map((change) => <li key={change.name} className="font-mono text-xs">
        {change.name}: {change.from ?? 'absent'} → {change.to ?? 'removed'}
      </li>)}</ul></AlertDescription>
    </Alert>}
    <FieldGroup>
      {plan.trustedSource ? <p className="text-sm">You previously trusted this source. Changed targets and warnings are still checked.</p> :
        check('execute', 'I allow this exact source to execute third-party code')}
      {plan.hasInstallPy && check('hook', 'Run the root install.py script with the selected Python environment')}
      {plan.trustedInstallPy && <p className="text-xs text-muted-foreground">Script permission was previously remembered. This review still starts with script execution unchecked.</p>}
      {plan.warnings.length > 0 && check('warnings', 'I acknowledge all displayed warnings')}
      {plan.protectedChanges.length > 0 && check('pins', 'I approve exactly these protected-package changes')}
      {plan.source.kind !== 'manual' && (plan.source.repository !== null) && check('remember', 'Trust this source for future installations')}
      {plan.hasInstallPy && value.hook && value.remember && check('rememberHook', 'Also remember permission to run install.py for this source')}
    </FieldGroup>
  </FieldSet>
}

export function ReviewDialog({ plans, busy, onCommit, onClose }: {
  plans: PlanReviewDto[]
  busy: boolean
  onCommit: (decisions: CommitInput[]) => void
  onClose: () => void
}) {
  const [consents, setConsents] = useState<Record<string, Consent>>({})
  const ready = plans.length > 0 && plans.every((plan) => canCommit(plan, consents[plan.id] ?? EMPTY_CONSENT))
  return <Dialog open onOpenChange={(open) => { if (!open && !busy) onClose() }}>
    <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-2xl" showCloseButton={!busy}>
      <DialogHeader>
        <DialogTitle>Review {plans.length > 1 ? `${plans.length} node updates` : 'custom node plan'}</DialogTitle>
        <DialogDescription>
          Custom nodes, Python dependencies and install scripts execute third-party code. A venv is not a sandbox.
          Confirm only sources you trust. Dependency changes cannot be rolled back; a failure requires environment repair.
        </DialogDescription>
      </DialogHeader>
      <fieldset disabled={busy} className="flex min-w-0 flex-col gap-6">
        {plans.map((plan) => <PlanConsent key={`${plan.id}-${plan.binding}`} plan={plan} value={consents[plan.id] ?? EMPTY_CONSENT}
          onChange={(value) => setConsents((current) => ({ ...current, [plan.id]: value }))} />)}
      </fieldset>
      <DialogFooter>
        <Button variant="outline" disabled={busy} onClick={onClose}>Discard plans</Button>
        <Button disabled={busy || !ready} onClick={() => onCommit(plans.map((plan) => decision(plan, consents[plan.id] ?? EMPTY_CONSENT)))}>
          {busy ? 'Applying and verifying…' : 'Apply reviewed plans'}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
}
