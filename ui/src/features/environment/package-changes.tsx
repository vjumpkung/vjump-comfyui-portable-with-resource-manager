import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'

import { useTranslation } from '@/shared/i18n'
import {
  gateway,
  type PipOperationKindDto,
  type PipPlanDto,
} from '@/shared/ipc/gateway'
import { environmentKeys } from '@/shared/lib/environment-queries'
import { Alert, AlertTitle, AlertDescription } from '@/shared/ui/alert'
import { Button } from '@/shared/ui/button'
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/shared/ui/card'
import { Checkbox } from '@/shared/ui/checkbox'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shared/ui/dialog'
import { ErrorNotice } from '@/shared/ui/feedback'
import {
  Field,
  FieldDescription,
  FieldGroup,
  FieldLabel,
} from '@/shared/ui/field'
import { Input } from '@/shared/ui/input'
import { StatusBadge } from '@/shared/ui/status-badge'
import { ToggleGroup, ToggleGroupItem } from '@/shared/ui/toggle-group'

const OPERATION_KEYS = {
  install: 'environment.changes.installLower',
  upgrade: 'environment.changes.upgradeLower',
  uninstall: 'environment.changes.uninstallLower',
} as const

const RESOLUTION_KEYS = {
  constrained: 'environment.changes.constrained',
  unconstrained: 'environment.changes.unconstrained',
  removal: 'environment.changes.removal',
} as const

export function PackageChanges({
  envId,
  maintenanceBusy,
}: {
  envId: string
  maintenanceBusy: boolean
}) {
  const { t } = useTranslation()
  const client = useQueryClient()
  const [kind, setKind] = useState<PipOperationKindDto>('install')
  const [spec, setSpec] = useState('')
  const [open, setOpen] = useState(false)
  const [approvedPlan, setApprovedPlan] = useState<string | null>(null)
  const [attemptedPlan, setAttemptedPlan] = useState<string | null>(null)
  const preview = useMutation({
    mutationFn: gateway.envPipPreview,
    onSuccess: () => {
      setApprovedPlan(null)
      setOpen(true)
    },
  })
  const apply = useMutation({
    mutationFn: (plan: PipPlanDto) =>
      gateway.envPipApply({
        planId: plan.planId,
        confirmedChanges: plan.requiresConfirmation
          ? plan.protectedChanges
          : [],
      }),
    onMutate: (plan) => setAttemptedPlan(plan.planId),
    onSuccess: () => setOpen(false),
    onSettled: () => {
      void client.invalidateQueries({ queryKey: environmentKeys.status })
      void client.invalidateQueries({ queryKey: environmentKeys.info })
      void client.invalidateQueries({ queryKey: environmentKeys.packages })
    },
  })
  const plan = preview.data
  const pending = preview.isPending || apply.isPending
  const approved = !plan?.requiresConfirmation || approvedPlan === plan.planId
  const resetPreview = () => {
    preview.reset()
    setApprovedPlan(null)
    apply.reset()
  }
  return (
    <>
      <Card>
        <CardHeader>
          <CardTitle>{t('environment.changes.title')}</CardTitle>
          <CardDescription>
            {t('environment.changes.hint')}
          </CardDescription>
        </CardHeader>
        <CardContent>
          <FieldGroup>
            <Field>
              <FieldLabel>{t('environment.changes.operation')}</FieldLabel>
              <ToggleGroup
                type="single"
                variant="outline"
                value={kind}
                disabled={pending || maintenanceBusy}
                onValueChange={(value) => {
                  if (
                    value === 'install' ||
                    value === 'upgrade' ||
                    value === 'uninstall'
                  ) {
                    setKind(value)
                    resetPreview()
                  }
                }}
              >
                <ToggleGroupItem value="install">{t('common.install')}</ToggleGroupItem>
                <ToggleGroupItem value="upgrade">{t('environment.changes.upgrade')}</ToggleGroupItem>
                <ToggleGroupItem value="uninstall">{t('common.uninstall')}</ToggleGroupItem>
              </ToggleGroup>
            </Field>
            <Field>
              <FieldLabel htmlFor="package-spec">
                {kind === 'uninstall'
                  ? t('environment.changes.distribution')
                  : t('environment.changes.requirement')}
              </FieldLabel>
              <Input
                id="package-spec"
                value={spec}
                disabled={pending || maintenanceBusy}
                placeholder={
                  kind === 'uninstall' ? 'package-name' : 'package-name==1.2.3'
                }
                onChange={(event) => {
                  setSpec(event.target.value)
                  resetPreview()
                }}
              />
              <FieldDescription>
                {t('environment.changes.requirementHint')}
              </FieldDescription>
            </Field>
          </FieldGroup>
          <Button
            disabled={!spec.trim() || pending || maintenanceBusy}
            onClick={() => {
              apply.reset()
              preview.mutate({ kind, specs: [spec.trim()] })
            }}
          >
            {preview.isPending
              ? t('environment.changes.resolving')
              : t('environment.changes.preview')}
          </Button>
          {preview.isError && <ErrorNotice error={preview.error} />}
          {apply.isError && <ErrorNotice error={apply.error} />}
          {apply.isSuccess && (
            <Alert
              variant={
                apply.data.envState === 'verified' ? 'default' : 'destructive'
              }
            >
              <AlertTitle>
                {t('environment.changes.finished')}{' '}
                <StatusBadge status={apply.data.envState} />
              </AlertTitle>
              <AlertDescription>
                {apply.data.envState === 'verified'
                  ? t('environment.changes.verified')
                  : t('environment.changes.repair')}
              </AlertDescription>
            </Alert>
          )}
        </CardContent>
      </Card>
      <Dialog
        open={open}
        onOpenChange={(next) => {
          if (!apply.isPending) setOpen(next)
        }}
      >
        <DialogContent
          className="max-h-[85vh] overflow-y-auto sm:max-w-2xl"
          showCloseButton={!apply.isPending}
        >
          <DialogHeader>
            <DialogTitle>{t('environment.changes.review')}</DialogTitle>
            <DialogDescription>
              {t('environment.changes.planHint')}
            </DialogDescription>
          </DialogHeader>
          {plan && (
            <>
              <p className="break-all font-mono text-xs">
                {t('environment.changes.planIdentity', { envId: plan.envId, planId: plan.planId })}
                <br />
                {t('environment.changes.planOperation', { operation: t(OPERATION_KEYS[plan.operation.kind]), specs: plan.operation.specs.join(', '), resolution: t(RESOLUTION_KEYS[plan.resolution]) })}
              </p>
              <div className="grid gap-4 sm:grid-cols-2">
                <div>
                  <h3 className="font-semibold">{t('environment.changes.installs')}</h3>
                  <ul className="break-all font-mono text-xs">
                    {plan.installs.map((pkg) => (
                      <li key={pkg.name}>
                        {pkg.name} {pkg.version}
                      </li>
                    ))}
                  </ul>
                  {plan.installs.length === 0 && <p>{t('environment.changes.noInstalls')}</p>}
                </div>
                <div>
                  <h3 className="font-semibold">{t('environment.changes.removals')}</h3>
                  <ul className="break-all font-mono text-xs">
                    {plan.removals.map((pkg) => (
                      <li key={pkg.name}>
                        {pkg.name} {pkg.version}
                      </li>
                    ))}
                  </ul>
                  {plan.removals.length === 0 && <p>{t('environment.changes.noRemovals')}</p>}
                </div>
              </div>
              {plan.requiresConfirmation && (
                <Alert variant="destructive">
                  <AlertTitle>{t('environment.changes.protectedTitle')}</AlertTitle>
                  <AlertDescription>
                    <p>
                      {t('environment.changes.protectedHint')}
                    </p>
                    <ul className="break-all font-mono text-xs">
                      {plan.protectedChanges.map((change) => (
                        <li key={change.name}>
                          {change.name}: {change.from ?? t('environment.changes.absent')} →{' '}
                          {change.to ?? t('environment.changes.removed')}
                        </li>
                      ))}
                    </ul>
                    <Field orientation="horizontal">
                      <Checkbox
                        id="approve-pins"
                        checked={approvedPlan === plan.planId}
                        disabled={apply.isPending}
                        onCheckedChange={(value) =>
                          setApprovedPlan(value === true ? plan.planId : null)
                        }
                      />
                      <FieldLabel htmlFor="approve-pins">
                        {t('environment.changes.approve')}
                      </FieldLabel>
                    </Field>
                  </AlertDescription>
                </Alert>
              )}
              {plan.envId !== envId && (
                <p role="alert">
                  {t('environment.changes.envChanged')}
                </p>
              )}
              {apply.isError && <ErrorNotice error={apply.error} />}
              {attemptedPlan === plan.planId && !apply.isPending && (
                <p>{t('environment.changes.freshPreview')}</p>
              )}
              <DialogFooter>
                <Button
                  variant="outline"
                  disabled={apply.isPending}
                  onClick={() => setOpen(false)}
                >
                  {t('environment.changes.close')}
                </Button>
                <Button
                  variant={
                    plan.operation.kind === 'uninstall'
                      ? 'destructive'
                      : 'default'
                  }
                  disabled={
                    !approved ||
                    plan.envId !== envId ||
                    attemptedPlan === plan.planId ||
                    pending ||
                    maintenanceBusy
                  }
                  onClick={() => apply.mutate(plan)}
                >
                  {apply.isPending
                    ? t('environment.changes.applying')
                    : t('environment.changes.apply')}
                </Button>
              </DialogFooter>
            </>
          )}
        </DialogContent>
      </Dialog>
    </>
  )
}
