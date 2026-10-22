import { useMutation, useQuery } from '@tanstack/react-query'
import { useState, type ReactNode } from 'react'
import { Check, FolderOpen, Info, RefreshCw } from 'lucide-react'

import { useTranslation } from '@/shared/i18n'
import {
  gateway,
  type PythonSourceDto,
  type SetupModeDto,
  type SetupStatusDto,
  type ValidationTargetDto,
} from '@/shared/ipc/gateway'
import { bytes } from '@/shared/lib/display'
import { Alert, AlertDescription, AlertTitle } from '@/shared/ui/alert'
import { Badge } from '@/shared/ui/badge'
import { Button } from '@/shared/ui/button'
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from '@/shared/ui/card'
import { Checkbox } from '@/shared/ui/checkbox'
import { ErrorNotice } from '@/shared/ui/feedback'
import {
  Field,
  FieldDescription,
  FieldGroup,
  FieldLabel,
  FieldLegend,
  FieldSet,
} from '@/shared/ui/field'
import { Input } from '@/shared/ui/input'
import { Skeleton } from '@/shared/ui/skeleton'
import { RadioGroup, RadioGroupItem } from '@/shared/ui/radio-group'
import { StatusBadge } from '@/shared/ui/status-badge'
import { ToggleGroup, ToggleGroupItem } from '@/shared/ui/toggle-group'
import {
  modeLabels,
  profileReasons,
  readDraft,
  type ComfySource,
  type CuratedReviewProps,
} from './model'
import { RunProgress } from './run-progress'
import { useSetupRun } from './use-setup-run'

// Display only: the backend resolves and installs the real Manager source.
const MANAGER_SOURCE = 'https://github.com/Comfy-Org/ComfyUI-Manager'
const THIRD_PARTY_DISCLOSURE = 'setup.thirdPartyDisclosure'
const STAGES = [
  'setup.installRoot', 'setup.hardware', 'setup.pythonSource',
  'setup.profile', 'setup.options', 'setup.review',
] as const satisfies readonly (keyof typeof import('@/shared/i18n/translations.json').en)[]

const VALIDATION_TARGETS = {
  empty_folder: 'setup.validationTarget.emptyFolder',
  existing_venv: 'setup.validationTarget.existingVenv',
  interpreter: 'setup.validationTarget.interpreter',
  rejected: 'setup.validationTarget.rejected',
} as const satisfies Record<ValidationTargetDto, keyof typeof import('@/shared/i18n/translations.json').en>

export function SetupWizard({
  status,
  mode,
  resume = false,
  onClose,
  refresh,
  renderCuratedReview,
  languageControl,
  preferencesPending = false,
}: {
  status: SetupStatusDto
  mode: SetupModeDto
  resume?: boolean
  onClose: () => void
  refresh: () => void
  renderCuratedReview: (props: CuratedReviewProps) => ReactNode
  languageControl?: (disabled: boolean) => ReactNode
  preferencesPending?: boolean
}) {
  const { t } = useTranslation()
  const [draft] = useState(() => readDraft(status.draft))
  const [stage, setStage] = useState(mode === 'first_run' ? 0 : 1)
  const [root, setRoot] = useState(status.installRoot)
  const [source, setSource] = useState<PythonSourceDto>(() => {
    if (mode === 'switch_venv') return { kind: 'customVenv', path: '' }
    if (mode === 'change_torch' && status.activeEnv?.source === 'custom_venv')
      return { kind: 'customVenv', path: '' }
    if (
      mode === 'change_torch' &&
      status.activeEnv?.source === 'custom_interpreter'
    )
      return { kind: 'customInterpreter', path: '' }
    return mode === 'first_run'
      ? (draft.pythonSource ?? { kind: 'managed' })
      : { kind: 'managed' }
  })
  const [profileId, setProfileId] = useState(
    draft.profileId ??
      (mode === 'repair' || mode === 'change_torch'
        ? (status.activeEnv?.profileId ?? '')
        : ''),
  )
  const [tag, setTag] = useState(draft.tag ?? '')
  const [comfySource, setComfySource] = useState<ComfySource>(
    draft.comfySource ?? 'latest',
  )
  const [installManager, setInstallManager] = useState(
    draft.installManager ?? mode === 'first_run',
  )
  const [cleanCache, setCleanCache] = useState(draft.cleanCache ?? false)
  const [acceleratorSelection, setAcceleratorSelection] = useState<{
    profileId: string
    ids: string[]
  } | null>(null)
  const [curatedSelection, setCuratedSelection] = useState<string[] | null>(null)
  const [adoptedConfirmed, setAdoptedConfirmed] = useState(false)
  const [showRun, setShowRun] = useState(resume)
  const [dismissedRun, setDismissedRun] = useState<string | null>(null)
  const controller = useSetupRun()
  const rootSelection = useMutation({
    mutationFn: () =>
      gateway.setupSelectRoot(
        root.trim(),
        JSON.stringify({
          version: 1,
          pythonSource: source,
          profileId,
          tag,
          comfySource,
          installManager,
          cleanCache,
        }),
      ),
  })
  const restartApp = useMutation({ mutationFn: () => gateway.setupRestart() })
  const picker = useMutation({
    mutationFn: (kind: 'root' | 'python') =>
      kind === 'root' || source.kind === 'customVenv'
        ? gateway.pickFolder(
            kind === 'root'
              ? t('setup.chooseRoot')
              : t('setup.chooseVenv'),
          )
        : gateway.pickFile(t('setup.chooseInterpreter')),
    onSuccess: (path, kind) => {
      if (!path) return
      if (kind === 'root') setRoot(path)
      else if (source.kind !== 'managed') {
        setSource({ ...source, path })
        setAdoptedConfirmed(false)
      }
    },
  })
  const validation = useMutation({ mutationFn: gateway.setupValidatePython })
  const report =
    validation.data &&
    JSON.stringify(validation.variables) === JSON.stringify(source)
      ? validation.data
      : undefined
  const customEnv =
    source.kind === 'customVenv' && report ? report.resolvedPath : null
  const hardware = useQuery({
    queryKey: ['setup-hardware', status.installRoot, customEnv],
    queryFn: () => gateway.setupDetectHardware(null, customEnv),
    enabled: stage >= 1 && !showRun,
  })
  const profiles = useQuery({
    queryKey: ['setup-profiles', status.installRoot, customEnv],
    queryFn: () => gateway.setupListProfiles(null, customEnv),
    enabled: stage >= 1 && !showRun,
  })
  const curated = useQuery({
    queryKey: ['setup-curated-nodes'],
    queryFn: () => gateway.setupListCuratedNodes(),
    enabled: mode === 'first_run' && !showRun,
  })
  // The curated list is offered unticked: the user ticks the nodes they want.
  if (curated.data && curatedSelection === null) setCuratedSelection([])
  const offers = profiles.data ?? []
  // Recommended profiles first; the sort is stable, so the backend order is kept otherwise.
  const listed = [...offers].sort(
    (a, b) => Number(b.recommended) - Number(a.recommended),
  )
  const eligible = offers.filter(
    (profile) =>
      profileReasons(profile, mode, status.activeEnv, report).length === 0,
  )
  const chosen =
    offers.find((profile) => profile.id === profileId) ??
    eligible.find((profile) => profile.recommended) ??
    eligible[0]
  if (chosen && acceleratorSelection?.profileId !== chosen.id)
    setAcceleratorSelection({
      profileId: chosen.id,
      ids: chosen.accelerators
        .filter((option) => option.defaultSelected)
        .map((option) => option.id),
    })
  const accelerators =
    acceleratorSelection?.profileId === chosen?.id
      ? (acceleratorSelection?.ids ?? [])
      : []
  const profileOK =
    !!chosen &&
    profileReasons(chosen, mode, status.activeEnv, report).length === 0
  const pythonOK =
    source.kind === 'managed' ||
    (!!report?.usable &&
      (report.target !== 'existing_venv' || adoptedConfirmed))
  const optionsOK =
    mode !== 'first_run' ||
    ((comfySource !== 'tag' || tag.trim().length > 0) &&
      curated.isSuccess &&
      curatedSelection !== null)
  const ready =
    profileOK &&
    pythonOK &&
    optionsOK &&
    !hardware.isFetching &&
    !profiles.isFetching
  const changeSource = (kind: string) => {
    if (
      kind !== 'managed' &&
      kind !== 'customVenv' &&
      kind !== 'customInterpreter'
    )
      return
    setSource(kind === 'managed' ? { kind } : { kind, path: '' })
    setAdoptedConfirmed(false)
  }
  const sourceLocked = mode === 'change_torch'
  // Only a first run carries the review-step consent; other modes keep the per-install prompts.
  const thirdPartyConsent =
    mode === 'first_run' &&
    (installManager || (curatedSelection ?? []).length > 0)
  const thirdPartySources = thirdPartyConsent
    ? [
        ...(installManager ? [MANAGER_SOURCE] : []),
        ...(curated.data ?? [])
          .filter((entry) => curatedSelection?.includes(entry.id))
          .map((entry) => entry.repository),
      ]
    : []

  if (showRun || (resume && dismissedRun !== status.run?.id))
    return (
      <div className="flex flex-col gap-6">
        <h1 className="text-2xl font-extrabold tracking-tighter">
          {t(modeLabels[mode])}
        </h1>
        <RunProgress
          run={status.run}
          controller={controller}
          refresh={refresh}
          renderCuratedReview={renderCuratedReview}
          onDone={
            status.activeEnv
              ? onClose
              : () => {
                  setShowRun(false)
                  setDismissedRun(status.run?.id ?? null)
                  controller.action.reset()
                  setStage(1)
                }
          }
        />
      </div>
    )

  return (
    <div className="mx-auto flex w-full max-w-5xl flex-col gap-6">
      <header className="flex flex-col gap-2">
        <h1 className="text-2xl font-extrabold tracking-tighter sm:text-3xl">
          {t(modeLabels[mode])}
        </h1>
        <p className="text-sm text-muted-foreground">
          {t('setup.intro')}
        </p>
      </header>
      <ol
        aria-label={t('setup.stages')}
        className="flex flex-wrap gap-3 font-mono text-xs"
      >
        {STAGES.map((name, index) => (
          <li key={name} aria-current={stage === index ? 'step' : undefined}>
            <Badge variant={stage === index ? 'secondary' : 'outline'}>
              {index + 1}. {t(name)}
            </Badge>
          </li>
        ))}
      </ol>
      {mode !== 'first_run' && (
        <Alert>
          <Info />
          <AlertTitle>{t('setup.preserveTitle')}</AlertTitle>
          <AlertDescription>
            {t('setup.preserveDescription')}
          </AlertDescription>
        </Alert>
      )}
      <Card>
        <CardHeader>
          <CardTitle>{t(STAGES[stage])}</CardTitle>
          <CardDescription>
            {t('setup.stepOf', { n: stage + 1, total: STAGES.length })}
          </CardDescription>
        </CardHeader>
        <CardContent>
          {stage === 0 && (
            <FieldGroup>
              {languageControl?.(rootSelection.isPending || rootSelection.isSuccess)}
              <Field data-disabled={status.rootSource === 'debug_override'}>
                <FieldLabel htmlFor="install-root">{t('setup.installRoot')}</FieldLabel>
                <div className="flex gap-2">
                  <Input
                    id="install-root"
                    className="font-mono"
                    value={root}
                    disabled={
                      status.rootSource === 'debug_override' ||
                      rootSelection.isSuccess
                    }
                    onChange={(event) => setRoot(event.target.value)}
                  />
                  <Button
                    variant="outline"
                    disabled={
                      status.rootSource === 'debug_override' ||
                      rootSelection.isSuccess ||
                      picker.isPending
                    }
                    onClick={() => picker.mutate('root')}
                  >
                    <FolderOpen data-icon="inline-start" />
                    {t('common.browse')}
                  </Button>
                </div>
                <FieldDescription>
                  {t('setup.rootHint')}
                </FieldDescription>
              </Field>
              {status.rootSource === 'debug_override' && (
                <p>
                  {t('setup.debugRootHint')}
                </p>
              )}
              {rootSelection.isSuccess && (
                <Alert>
                  <Check />
                  <AlertTitle>{t('setup.restartRequired')}</AlertTitle>
                  <AlertDescription>
                    {t('setup.rootRestartHint', { root: root.trim() })}
                    <Button
                      size="sm"
                      disabled={restartApp.isPending || restartApp.isSuccess}
                      onClick={() => restartApp.mutate()}
                    >
                      {restartApp.isPending || restartApp.isSuccess
                        ? t('setup.restarting')
                        : t('setup.restartNow')}
                    </Button>
                  </AlertDescription>
                </Alert>
              )}
              {restartApp.isError && <ErrorNotice error={restartApp.error} />}
              {rootSelection.isError && (
                <ErrorNotice error={rootSelection.error} />
              )}
            </FieldGroup>
          )}
          {stage === 1 && (
            <>
              {hardware.isPending && <Skeleton className="h-32" />}
              {hardware.isError && (
                <ErrorNotice
                  error={hardware.error}
                  retry={() => {
                    void hardware.refetch()
                  }}
                />
              )}
              {hardware.data && (
                <>
                  <dl className="grid grid-cols-1 gap-3 text-sm sm:grid-cols-2">
                    <div>
                      <dt>{t('setup.platform')}</dt>
                      <dd className="font-mono">
                        {hardware.data.os} · {hardware.data.osVersion}
                      </dd>
                    </div>
                    <div>
                      <dt>{t('setup.nvidiaDetection')}</dt>
                      <dd>
                        {hardware.data.gpuStatus === 'unavailable'
                          ? t('setup.probeUnavailable')
                          : hardware.data.gpuStatus === 'no_nvidia_gpu'
                            ? t('setup.noNvidia')
                            : t('setup.detected')}
                      </dd>
                    </div>
                    <div>
                      <dt>{t('setup.driverCuda')}</dt>
                      <dd className="font-mono">
                        {hardware.data.driverVersion ?? t('common.unknownValue')} /{' '}
                        {hardware.data.driverCuda ?? t('common.unknownValue')}
                      </dd>
                    </div>
                    <div>
                      <dt>{t('setup.installFree')}</dt>
                      <dd className="font-mono">
                        {bytes(hardware.data.installVolumeFreeBytes)}
                      </dd>
                    </div>
                    {hardware.data.envOnSeparateVolume && (
                      <div>
                        <dt>{t('setup.environmentFree')}</dt>
                        <dd className="font-mono">
                          {bytes(hardware.data.envVolumeFreeBytes)}
                        </dd>
                      </div>
                    )}
                    {hardware.data.os === 'windows' && (
                      <div>
                        <dt>{t('setup.longPaths')}</dt>
                        <dd>
                          {hardware.data.longPathsEnabled === null
                            ? t('common.unknownValue')
                            : hardware.data.longPathsEnabled
                              ? t('common.enabled')
                              : t('setup.longPathsDisabled')}
                        </dd>
                      </div>
                    )}
                  </dl>
                  <ul className="flex flex-col gap-2">
                    {hardware.data.gpus.map((gpu) => (
                      <li key={gpu.index} className="font-mono text-xs">
                        GPU {gpu.index}: {gpu.name} · CC{' '}
                        {gpu.computeCapability ?? t('common.unknownLowercase')} ·{' '}
                        {gpu.vramMib === null
                          ? t('setup.unknownVram')
                          : `${gpu.vramMib} MiB VRAM`}
                      </li>
                    ))}
                  </ul>
                  <p className="text-sm text-muted-foreground">
                    {t('setup.hardwareHint')}
                  </p>
                  <Button
                    variant="outline"
                    disabled={hardware.isFetching || profiles.isFetching}
                    onClick={() => {
                      void hardware.refetch()
                      void profiles.refetch()
                    }}
                  >
                    <RefreshCw data-icon="inline-start" />
                    {t('setup.checkHardware')}
                  </Button>
                </>
              )}
            </>
          )}
          {stage === 2 && (
            <FieldGroup>
              <Field>
                <FieldLabel>{t('setup.pythonSource')}</FieldLabel>
                <ToggleGroup
                  type="single"
                  variant="outline"
                  value={source.kind}
                  onValueChange={changeSource}
                  className="flex flex-wrap justify-start"
                >
                  <ToggleGroupItem
                    value="managed"
                    disabled={
                      mode === 'switch_venv' ||
                      (sourceLocked && source.kind !== 'managed')
                    }
                  >
                    {t('setup.appManaged')}
                  </ToggleGroupItem>
                  <ToggleGroupItem
                    value="customVenv"
                    disabled={sourceLocked && source.kind !== 'customVenv'}
                  >
                    {t('setup.customVenv')}
                  </ToggleGroupItem>
                  <ToggleGroupItem
                    value="customInterpreter"
                    disabled={
                      mode === 'switch_venv' ||
                      (sourceLocked && source.kind !== 'customInterpreter')
                    }
                  >
                    {t('setup.customInterpreter')}
                  </ToggleGroupItem>
                </ToggleGroup>
                <FieldDescription>
                  {t('setup.pythonSourceHint')}
                  {sourceLocked &&
                    t('setup.sourceLockedHint')}
                </FieldDescription>
              </Field>
              {source.kind !== 'managed' && (
                <>
                  <Field data-invalid={!!report && !report.usable}>
                    <FieldLabel htmlFor="python-path">
                      {source.kind === 'customVenv'
                        ? t('setup.venvFolder')
                        : t('setup.pythonExecutable')}
                    </FieldLabel>
                    <div className="flex gap-2">
                      <Input
                        id="python-path"
                        className="font-mono"
                        value={source.path}
                        aria-invalid={!!report && !report.usable}
                        onChange={(event) => {
                          setSource({ ...source, path: event.target.value })
                          setAdoptedConfirmed(false)
                        }}
                      />
                      <Button
                        variant="outline"
                        disabled={picker.isPending}
                        onClick={() => picker.mutate('python')}
                      >
                        {t('common.browse')}
                      </Button>
                    </div>
                  </Field>
                  <Button
                    variant="outline"
                    disabled={!source.path.trim() || validation.isPending}
                    onClick={() => validation.mutate(source)}
                  >
                    {validation.isPending ? t('setup.validating') : t('setup.validatePython')}
                  </Button>
                  {validation.isError && (
                    <ErrorNotice error={validation.error} />
                  )}
                  {report && (
                    <>
                      <p className="break-all font-mono text-xs">
                        {report.resolvedPath} · Python{' '}
                        {report.pythonVersion ?? t('setup.notCreated')} ·{' '}
                        {Object.hasOwn(VALIDATION_TARGETS, report.target)
                          ? t(VALIDATION_TARGETS[report.target])
                          : report.target}
                      </p>
                      <ul className="flex flex-col gap-2">
                        {report.checks.map((check) => (
                          <li key={check.id} className="flex items-start gap-2">
                            <StatusBadge
                              status={check.passed ? 'verified' : 'failed'}
                            />
                            <span>
                              {check.detail}
                              {!check.passed && check.blocks.length > 0
                                ? t('setup.blocks', { blocks: check.blocks.join(', ') })
                                : ''}
                            </span>
                          </li>
                        ))}
                      </ul>
                      {report.target === 'existing_venv' && (
                        <FieldSet>
                          <FieldLegend>{t('setup.adoptConsent')}</FieldLegend>
                          <FieldDescription>
                            {t('setup.adoptHint')}
                          </FieldDescription>
                          <FieldGroup>
                            <Field orientation="horizontal">
                              <Checkbox
                                id="adopt-consent"
                                checked={adoptedConfirmed}
                                onCheckedChange={(checked) =>
                                  setAdoptedConfirmed(checked === true)
                                }
                              />
                              <FieldLabel htmlFor="adopt-consent">
                                {t('setup.approveAdopted', { path: report.resolvedPath })}
                              </FieldLabel>
                            </Field>
                          </FieldGroup>
                        </FieldSet>
                      )}
                    </>
                  )}
                </>
              )}
            </FieldGroup>
          )}
          {stage === 3 && (
            <>
              {profiles.isPending && <Skeleton className="h-32" />}
              {profiles.isError && (
                <ErrorNotice
                  error={profiles.error}
                  retry={() => {
                    void profiles.refetch()
                  }}
                />
              )}
              <FieldSet>
                <FieldLegend>{t('setup.setupProfile')}</FieldLegend>
                <FieldDescription>
                  {t('setup.profileHint')}
                </FieldDescription>
                <FieldGroup>
                  <RadioGroup
                    aria-label={t('setup.setupProfile')}
                    value={chosen?.id ?? ''}
                    onValueChange={setProfileId}
                  >
                    {listed.map((profile) => {
                      const reasons = profileReasons(
                        profile,
                        mode,
                        status.activeEnv,
                        report,
                      )
                      const disabled = reasons.length > 0
                      return (
                        <Field key={profile.id} data-disabled={disabled}>
                          <FieldLabel htmlFor={`profile-${profile.id}`}>
                            <RadioGroupItem
                              value={profile.id}
                              id={`profile-${profile.id}`}
                              disabled={disabled}
                            />
                            {profile.label}
                            {profile.recommended && <Badge>{t('setup.recommended')}</Badge>}
                            {profile.status === 'experimental' && (
                              <Badge variant="outline">{t('setup.experimental')}</Badge>
                            )}
                          </FieldLabel>
                          <FieldDescription>
                            Python {profile.python} · torch{' '}
                            {profile.torch.torch} · CUDA {profile.torch.cuda} ·{' '}
                            {profile.triton}
                            <br />
                            {profile.torch.torchaudio
                              ? `torchaudio ${profile.torch.torchaudio}`
                              : t('setup.noTorchaudio')}
                            <br />
                            {t('setup.estimatedPeak', {
                              size: bytes(profile.estimatedInstallBytes),
                              calibration: t(profile.estimateCalibrated ? 'setup.calibrated' : 'setup.notCalibrated'),
                            })}
                          </FieldDescription>
                          {reasons.length > 0 && (
                            <ul className="list-inside list-disc text-sm text-muted-foreground">
                              {reasons.map((reason) => (
                                <li key={reason}>{reason}</li>
                              ))}
                            </ul>
                          )}
                          <details>
                            <summary className="text-xs">
                              {t('setup.compatibilityEvidence')}
                            </summary>
                            <ul className="break-all font-mono text-xs">
                              {profile.provenance.map((entry) => (
                                <li key={entry}>{entry}</li>
                              ))}
                            </ul>
                          </details>
                        </Field>
                      )
                    })}
                  </RadioGroup>
                </FieldGroup>
              </FieldSet>
              {!profiles.isPending && offers.length === 0 && (
                <p>
                  {t('setup.noProfiles')}
                </p>
              )}
            </>
          )}
          {stage === 4 && (
            <FieldGroup>
              {mode === 'first_run' ? (
                <>
                  <Field>
                    <FieldLabel>{t('setup.comfySource')}</FieldLabel>
                    <ToggleGroup
                      type="single"
                      variant="outline"
                      value={comfySource}
                      onValueChange={(value) => {
                        if (
                          value === 'latest' ||
                          value === 'master' ||
                          value === 'tag'
                        )
                          setComfySource(value)
                      }}
                    >
                      <ToggleGroupItem value="latest">
                        {t('setup.latestRelease')}
                      </ToggleGroupItem>
                      <ToggleGroupItem value="master">
                        {t('setup.latestMaster')}
                      </ToggleGroupItem>
                      <ToggleGroupItem value="tag">
                        {t('setup.specificTag')}
                      </ToggleGroupItem>
                    </ToggleGroup>
                    <FieldDescription>
                      {comfySource === 'master'
                        ? t('setup.masterHint')
                        : t('setup.tagHint')}
                    </FieldDescription>
                  </Field>
                  {comfySource === 'tag' && (
                    <Field>
                      <FieldLabel htmlFor="comfy-tag">{t('setup.comfyTag')}</FieldLabel>
                      <Input
                        id="comfy-tag"
                        value={tag}
                        onChange={(event) => setTag(event.target.value)}
                        placeholder="v…"
                      />
                    </Field>
                  )}
                </>
              ) : (
                <p>
                  {t('setup.keepCheckoutHint')}
                </p>
              )}
              <FieldSet>
                <FieldLegend>{t('setup.optionalComponents')}</FieldLegend>
                <FieldGroup>
                  {mode === 'repair' ? (
                    <p>
                      {t('setup.repairComponents')}
                    </p>
                  ) : (
                    <>
                      <Field orientation="horizontal">
                        <Checkbox
                          id="manager"
                          checked={installManager}
                          onCheckedChange={(value) =>
                            setInstallManager(value === true)
                          }
                        />
                        <FieldLabel htmlFor="manager">
                          {t('setup.installManager')}
                        </FieldLabel>
                      </Field>
                      <FieldDescription>
                        {mode === 'first_run'
                          ? t('setup.managerFirstRunHint')
                          : t('setup.managerConsentHint')}
                      </FieldDescription>
                      <FieldSet>
                        <FieldLegend>{t('setup.accelerators')}</FieldLegend>
                        <FieldDescription>
                          {t('setup.acceleratorsHint')}
                        </FieldDescription>
                        <FieldGroup>
                          {chosen?.accelerators.map((option) => (
                            <Field key={option.id}>
                              <Field orientation="horizontal">
                                <Checkbox
                                  id={`accelerator-${option.id}`}
                                  checked={accelerators.includes(option.id)}
                                  onCheckedChange={(checked) =>
                                    setAcceleratorSelection({
                                      profileId: chosen.id,
                                      ids: checked === true
                                        ? [...accelerators, option.id]
                                        : accelerators.filter((id) => id !== option.id),
                                    })
                                  }
                                />
                                <FieldLabel htmlFor={`accelerator-${option.id}`}>
                                  {option.name}
                                </FieldLabel>
                              </Field>
                              <FieldDescription className="break-words">
                                {option.provenance}
                              </FieldDescription>
                            </Field>
                          ))}
                          {chosen?.accelerators.length === 0 && (
                            <p>{t('setup.noAccelerators')}</p>
                          )}
                        </FieldGroup>
                      </FieldSet>
                    </>
                  )}
                  <Field orientation="horizontal">
                    <Checkbox
                      id="clean-cache"
                      checked={cleanCache}
                      onCheckedChange={(value) => setCleanCache(value === true)}
                    />
                    <FieldLabel htmlFor="clean-cache">
                      {t('setup.cleanCacheAfter')}
                    </FieldLabel>
                  </Field>
                </FieldGroup>
              </FieldSet>
              {mode === 'first_run' && (
                <FieldSet>
                  <FieldLegend>{t('setup.curatedNodes')}</FieldLegend>
                  <FieldDescription>
                    {t('setup.curatedHint')}
                  </FieldDescription>
                  {curated.isPending && <p role="status">{t('setup.loadingCurated')}</p>}
                  {curated.isError && (
                    <ErrorNotice error={curated.error} retry={() => { void curated.refetch() }} />
                  )}
                  <FieldGroup>
                    {curated.data?.map((entry) => (
                      <Field key={entry.id}>
                        <Field orientation="horizontal">
                          <Checkbox
                            id={`curated-${entry.id}`}
                            checked={entry.unavailable === null
                              && (curatedSelection?.includes(entry.id) ?? false)}
                            disabled={entry.unavailable !== null}
                            onCheckedChange={(checked) =>
                              setCuratedSelection((current) => checked === true
                                ? [...(current ?? []), entry.id]
                                : (current ?? []).filter((id) => id !== entry.id))
                            }
                          />
                          <FieldLabel htmlFor={`curated-${entry.id}`}>
                            {entry.name}
                          </FieldLabel>
                        </Field>
                        <FieldDescription className="break-all font-mono">
                          {t('setup.curatedRepository', { repository: entry.repository })}
                        </FieldDescription>
                        {entry.unavailable !== null && (
                          <FieldDescription>{t('setup.curatedUnavailable', { reason: entry.unavailable })}</FieldDescription>
                        )}
                      </Field>
                    ))}
                    {curated.isSuccess && curated.data.length === 0 && (
                      <p>{t('setup.noCurated')}</p>
                    )}
                  </FieldGroup>
                </FieldSet>
              )}
            </FieldGroup>
          )}
          {stage === 5 && (
            <>
              <dl className="grid grid-cols-1 gap-3 text-sm sm:grid-cols-2">
                <div>
                  <dt>{t('setup.installRoot')}</dt>
                  <dd className="break-all font-mono">{status.installRoot}</dd>
                </div>
                <div>
                  <dt>{t('setup.mode')}</dt>
                  <dd>{t(modeLabels[mode])}</dd>
                </div>
                <div>
                  <dt>Python</dt>
                  <dd className="break-all font-mono">
                    {source.kind === 'managed'
                      ? t('setup.managedVersion', { version: chosen?.python ?? '' })
                      : source.path}
                  </dd>
                </div>
                <div>
                  <dt>{t('setup.profile')}</dt>
                  <dd>{chosen?.label ?? t('setup.noEligibleProfile')}</dd>
                </div>
                <div>
                  <dt>{t('nav.comfyui')}</dt>
                  <dd>
                    {mode === 'first_run'
                      ? comfySource === 'tag'
                        ? tag
                        : comfySource === 'master'
                          ? t('setup.latestOnMaster')
                          : t('setup.latestPublished')
                      : t('setup.keepCheckout')}
                  </dd>
                </div>
                <div>
                  <dt>{t('setup.managerCache')}</dt>
                  <dd>
                    {mode === 'repair' ? t('setup.recordedComponents') : installManager
                      ? mode === 'first_run'
                        ? t('setup.managerSelected')
                        : t('setup.managerSelectedConsent')
                      : t('setup.managerOff')}{' '}
                    / {cleanCache ? t('setup.cleanCache') : t('setup.keepCache')}
                  </dd>
                </div>
                <div>
                  <dt>{t('setup.accelerators')}</dt>
                  <dd>{mode === 'repair' ? t('setup.recordedChoices') : chosen?.accelerators
                    .filter((option) => accelerators.includes(option.id))
                    .map((option) => option.name).join(', ') || t('common.noneSelected')}</dd>
                </div>
                {mode === 'first_run' && <div>
                  <dt>{t('setup.curatedNodes')}</dt>
                  <dd>{curated.data?.filter((entry) => curatedSelection?.includes(entry.id))
                    .map((entry) => entry.name).join(', ') || t('common.noneSelected')}</dd>
                </div>}
              </dl>
              <p>
                {t('setup.reviewHint')}
              </p>
              {thirdPartyConsent && (
                <Alert>
                  <Info />
                  <AlertTitle>{t('setup.thirdParty')}</AlertTitle>
                  <AlertDescription>
                    <p>{t(THIRD_PARTY_DISCLOSURE)}</p>
                    <ul
                      aria-label={t('setup.thirdPartySources')}
                      className="break-all font-mono text-xs"
                    >
                      {thirdPartySources.map((source) => (
                        <li key={source}>{source}</li>
                      ))}
                    </ul>
                  </AlertDescription>
                </Alert>
              )}
              {!ready && (
                <Alert variant="destructive">
                  <AlertTitle>{t('setup.notReady')}</AlertTitle>
                  <AlertDescription>
                    {t('setup.notReadyHint')}
                  </AlertDescription>
                </Alert>
              )}
              {controller.action.isError && (
                <ErrorNotice error={controller.action.error} />
              )}
            </>
          )}
          {picker.isError && <ErrorNotice error={picker.error} />}
        </CardContent>
        <CardFooter className="flex flex-wrap justify-between gap-2">
          <div className="flex gap-2">
            {stage > (mode === 'first_run' ? 0 : 1) && (
              <Button variant="outline" onClick={() => setStage(stage - 1)}>
                {t('common.back')}
              </Button>
            )}
            {status.activeEnv && (
              <Button variant="ghost" onClick={onClose}>
                {t('common.cancel')}
              </Button>
            )}
          </div>
          {stage === 0 ? (
            <Button
              disabled={
                !root.trim() ||
                preferencesPending ||
                rootSelection.isPending ||
                rootSelection.isSuccess
              }
              onClick={() => {
                if (root.trim() !== status.installRoot) rootSelection.mutate()
                else setStage(1)
              }}
            >
              {rootSelection.isPending
                ? t('setup.savingRoot')
                : root.trim() !== status.installRoot
                  ? t('setup.saveRoot')
                  : t('common.continue')}
            </Button>
          ) : stage < 5 ? (
            <Button
              disabled={
                (stage === 1 && !hardware.data) ||
                (stage === 2 && !pythonOK) ||
                (stage === 3 && !profileOK) ||
                (stage === 4 && !optionsOK)
              }
              onClick={() => setStage(stage + 1)}
            >
              {t('common.continue')}
            </Button>
          ) : (
            <Button
              disabled={!ready || controller.action.isPending}
              onClick={() => {
                if (!chosen) return
                controller.action.mutate(
                  {
                    kind: 'new',
                    request: {
                      mode,
                      pythonSource: source,
                      profileId: chosen.id,
                      comfyuiRef:
                        mode !== 'first_run'
                          ? { kind: 'keepCurrent' }
                          : comfySource === 'tag'
                            ? { kind: 'tag', value: tag.trim() }
                            : comfySource === 'master'
                              ? { kind: 'branch', value: 'master' }
                              : { kind: 'latest' },
                      installManager,
                      accelerators: mode === 'repair' ? [] : accelerators,
                      curatedNodes: mode === 'first_run' ? curatedSelection ?? [] : [],
                      cleanCache,
                      confirmAdoptedChanges: adoptedConfirmed,
                      allowSourceBuilds: false,
                      thirdPartyConsent,
                    },
                  },
                  { onSuccess: () => setShowRun(true) },
                )
              }}
            >
              {controller.action.isPending
                ? t('setup.registering')
                : t('setup.start')}
            </Button>
          )}
        </CardFooter>
      </Card>
    </div>
  )
}
