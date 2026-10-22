import { useId, useState, type ReactNode } from 'react'

import { useTranslation } from '@/shared/i18n'
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
  const { t } = useTranslation()
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
        <dt className="text-muted-foreground">{t('customNodes.review.resolvedSource')}</dt>
        <dd>{sourceLabel(plan.source)}</dd>
      </div>
      {plan.priorSource && <div>
        <dt className="text-muted-foreground">{t('customNodes.review.previousSource')}</dt>
        <dd>{sourceLabel(plan.priorSource)}</dd>
      </div>}
      <div>
        <dt className="text-muted-foreground">{t('customNodes.review.treeIdentity')}</dt>
        <dd>{plan.treeDigest}</dd>
      </div>
      {plan.artifactDigest && <div>
        <dt className="text-muted-foreground">{t('customNodes.review.artifactHash')}</dt>
        <dd>{plan.artifactDigest}</dd>
      </div>}
      {plan.hookDigest && <div>
        <dt className="text-muted-foreground">{t('customNodes.review.scriptHash')}</dt>
        <dd>{plan.hookDigest}</dd>
      </div>}
      <div>
        <dt className="text-muted-foreground">{t('customNodes.review.requirements')}</dt>
        <dd>
          {plan.hasRequirements ? t('customNodes.review.includesRequirements') : t('common.none')}
          {plan.requirements.length > 0 && <ul className="mt-1 max-h-40 overflow-y-auto rounded-md border p-2">
            {plan.requirements.map((requirement, index) => <li key={index}>{requirement}</li>)}
          </ul>}
        </dd>
      </div>
    </dl>
    {plan.warnings.length > 0 && <Alert>
      <AlertTitle>{t('customNodes.review.warnings')}</AlertTitle>
      <AlertDescription><ul className="break-words">{plan.warnings.map((warning, index) => <li key={index}>{warning}</li>)}</ul></AlertDescription>
    </Alert>}
    {plan.protectedChanges.length > 0 && <Alert variant="destructive">
      <AlertTitle>{t('customNodes.review.protectedChanges')}</AlertTitle>
      <AlertDescription><ul>{plan.protectedChanges.map((change) => <li key={change.name} className="font-mono text-xs">
        {change.name}: {change.from ?? t('customNodes.review.absent')} → {change.to ?? t('customNodes.review.removed')}
      </li>)}</ul></AlertDescription>
    </Alert>}
    {plan.trustedSource && <p className="text-sm">
      {t('customNodes.review.trustedSource')}
    </p>}
    {plan.trustedInstallPy && <p className="text-xs text-muted-foreground">
      {t('customNodes.review.trustedScript')}
    </p>}
    <FieldGroup>
      {check('execute', t('customNodes.review.executeConsent'))}
      {plan.hasInstallPy && check('hook', t('customNodes.review.scriptConsent'))}
      {plan.warnings.length > 0 && check('warnings', t('customNodes.review.warningsConsent'))}
      {plan.protectedChanges.length > 0 && check('pins', t('customNodes.review.pinsConsent'))}
      {allowRememberSource && namespace && check('remember', t('customNodes.review.rememberSource'))}
      {allowRememberSource && namespace && plan.hasInstallPy && value.hook && value.remember &&
        check('rememberHook', t('customNodes.review.rememberScript'))}
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
  title, commitLabel, closeLabel,
  allowRememberSource = true }: ReviewDialogProps) {
  const { t } = useTranslation()
  const [consents, setConsents] = useState<Record<string, Consent>>({})
  const consentOf = (plan: PlanReviewDto) => consents[plan.id] ?? EMPTY_CONSENT
  const ready = plans.length > 0 && plans.every((plan) => canCommit(plan, consentOf(plan)))
  return <>
    <DialogHeader>
      <DialogTitle>{title ?? (plans.length > 1 ? t('customNodes.review.updates', { count: plans.length }) : t('customNodes.review.plan'))}</DialogTitle>
      <DialogDescription>
        {t('customNodes.review.disclosure')}
      </DialogDescription>
    </DialogHeader>
    {notice}
    <fieldset disabled={busy} className="flex min-w-0 flex-col gap-6">
      {plans.map((plan) => <PlanConsent key={reviewKey(plan)} plan={plan} value={consentOf(plan)} allowRememberSource={allowRememberSource}
        onChange={(value) => setConsents((current) => ({ ...current, [plan.id]: value }))} />)}
    </fieldset>
    <DialogFooter>
      {busy && onCancel && <Button variant="outline" onClick={onCancel}>{t('customNodes.review.requestCancel')}</Button>}
      <Button variant="outline" disabled={busy} onClick={onClose}>{closeLabel ?? t('customNodes.review.discard')}</Button>
      <Button disabled={busy || !ready} onClick={() => onCommit(plans.map((plan) => decision(plan,
        allowRememberSource ? consentOf(plan) : { ...consentOf(plan), remember: false, rememberHook: false })))}>
        {busy ? t('customNodes.review.applying') : (commitLabel ?? t('customNodes.review.apply'))}
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
