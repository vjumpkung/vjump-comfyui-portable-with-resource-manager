import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'

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

export function PackageChanges({
  envId,
  maintenanceBusy,
}: {
  envId: string
  maintenanceBusy: boolean
}) {
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
          <CardTitle>Change packages</CardTitle>
          <CardDescription>
            Resolve a preview before changing the selected environment. ComfyUI
            must be stopped; the backend rechecks the plan under its maintenance
            gate.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <FieldGroup>
            <Field>
              <FieldLabel>Operation</FieldLabel>
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
                <ToggleGroupItem value="install">Install</ToggleGroupItem>
                <ToggleGroupItem value="upgrade">Upgrade</ToggleGroupItem>
                <ToggleGroupItem value="uninstall">Uninstall</ToggleGroupItem>
              </ToggleGroup>
            </Field>
            <Field>
              <FieldLabel htmlFor="package-spec">
                {kind === 'uninstall'
                  ? 'Distribution name'
                  : 'Package requirement'}
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
                Enter one requirement, including an optional version or extras.
                Source-distribution build hooks are disabled.
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
              ? 'Resolving preview…'
              : 'Preview package change'}
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
                Package operation finished{' '}
                <StatusBadge status={apply.data.envState} />
              </AlertTitle>
              <AlertDescription>
                {apply.data.envState === 'verified'
                  ? 'Packages and protected pins were rechecked successfully.'
                  : 'The environment remains selected but requires repair. It is not ready to launch.'}
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
            <DialogTitle>Review exact package plan</DialogTitle>
            <DialogDescription>
              Changes apply only to the displayed environment and plan. Editing
              the request requires a fresh preview.
            </DialogDescription>
          </DialogHeader>
          {plan && (
            <>
              <p className="break-all font-mono text-xs">
                Environment: {plan.envId} · Plan: {plan.planId}
                <br />
                {plan.operation.kind}: {plan.operation.specs.join(', ')} ·
                resolution: {plan.resolution}
              </p>
              <div className="grid gap-4 sm:grid-cols-2">
                <div>
                  <h3 className="font-semibold">Installs / upgrades</h3>
                  <ul className="break-all font-mono text-xs">
                    {plan.installs.map((pkg) => (
                      <li key={pkg.name}>
                        {pkg.name} {pkg.version}
                      </li>
                    ))}
                  </ul>
                  {plan.installs.length === 0 && <p>No installs</p>}
                </div>
                <div>
                  <h3 className="font-semibold">Removals</h3>
                  <ul className="break-all font-mono text-xs">
                    {plan.removals.map((pkg) => (
                      <li key={pkg.name}>
                        {pkg.name} {pkg.version}
                      </li>
                    ))}
                  </ul>
                  {plan.removals.length === 0 && <p>No removals</p>}
                </div>
              </div>
              {plan.requiresConfirmation && (
                <Alert variant="destructive">
                  <AlertTitle>Protected packages will change</AlertTitle>
                  <AlertDescription>
                    <p>
                      Removing a mandatory core package or failing verification
                      leaves the selected environment in RepairRequired. There
                      is no package rollback.
                    </p>
                    <ul className="break-all font-mono text-xs">
                      {plan.protectedChanges.map((change) => (
                        <li key={change.name}>
                          {change.name}: {change.from ?? 'absent'} →{' '}
                          {change.to ?? 'removed'}
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
                        I approve exactly these protected-package changes for
                        this plan
                      </FieldLabel>
                    </Field>
                  </AlertDescription>
                </Alert>
              )}
              {plan.envId !== envId && (
                <p role="alert">
                  The selected environment changed. Close this dialog and
                  request a fresh preview.
                </p>
              )}
              {apply.isError && <ErrorNotice error={apply.error} />}
              {attemptedPlan === plan.planId && !apply.isPending && (
                <p>Request a fresh preview before another apply attempt.</p>
              )}
              <DialogFooter>
                <Button
                  variant="outline"
                  disabled={apply.isPending}
                  onClick={() => setOpen(false)}
                >
                  Close preview
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
                    ? 'Applying and verifying…'
                    : 'Apply this plan'}
                </Button>
              </DialogFooter>
            </>
          )}
        </DialogContent>
      </Dialog>
    </>
  )
}
