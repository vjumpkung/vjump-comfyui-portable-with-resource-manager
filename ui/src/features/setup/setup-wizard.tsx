import { useMutation, useQuery } from '@tanstack/react-query'
import { useState, type ReactNode } from 'react'
import { Check, FolderOpen, Info, RefreshCw } from 'lucide-react'

import {
  gateway,
  type PythonSourceDto,
  type SetupModeDto,
  type SetupStatusDto,
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

const stages = [
  'Install root',
  'Hardware',
  'Python source',
  'Profile',
  'Options',
  'Review',
]

// Display only: the backend resolves and installs the real Manager source.
const MANAGER_SOURCE = 'https://github.com/Comfy-Org/ComfyUI-Manager'
const THIRD_PARTY_DISCLOSURE =
  "Installing ComfyUI-Manager and selected custom nodes runs third-party code on this computer; a Python environment is not a sandbox. Setup installs the latest commit of each repository’s default branch. For selected custom nodes, setup allows shipped Python cache files and top-level native programs or libraries, runs install.py if present, then installs the reviewed requirements.txt. It does not ask again. Clicking Start setup confirms this."

export function SetupWizard({
  status,
  mode,
  resume = false,
  onClose,
  refresh,
  renderCuratedReview,
}: {
  status: SetupStatusDto
  mode: SetupModeDto
  resume?: boolean
  onClose: () => void
  refresh: () => void
  renderCuratedReview: (props: CuratedReviewProps) => ReactNode
}) {
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
  const picker = useMutation({
    mutationFn: (kind: 'root' | 'python') =>
      kind === 'root' || source.kind === 'customVenv'
        ? gateway.pickFolder(
            kind === 'root'
              ? 'Choose install root'
              : 'Choose an existing venv or empty folder',
          )
        : gateway.pickFile('Choose Python interpreter'),
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
          {modeLabels[mode]}
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
          {modeLabels[mode]}
        </h1>
        <p className="text-sm text-muted-foreground">
          NVIDIA on Windows / Linux x64. Activation follows verification;
          replacement environments are built alongside the selected one.
        </p>
      </header>
      <ol
        aria-label="Setup stages"
        className="flex flex-wrap gap-3 font-mono text-xs"
      >
        {stages.map((name, index) => (
          <li key={name} aria-current={stage === index ? 'step' : undefined}>
            <Badge variant={stage === index ? 'secondary' : 'outline'}>
              {index + 1}. {name}
            </Badge>
          </li>
        ))}
      </ol>
      {mode !== 'first_run' && (
        <Alert>
          <Info />
          <AlertTitle>Re-setup preserves user data</AlertTitle>
          <AlertDescription>
            ComfyUI must be stopped before starting. Setup checks for running
            processes; stop an external ComfyUI process yourself. The current
            checkout is retained. Repair verifies first and creates a
            replacement only if needed. Models, user files and custom node
            folders are preserved.
          </AlertDescription>
        </Alert>
      )}
      <Card>
        <CardHeader>
          <CardTitle>{stages[stage]}</CardTitle>
          <CardDescription>
            Step {stage + 1} of {stages.length}
          </CardDescription>
        </CardHeader>
        <CardContent>
          {stage === 0 && (
            <FieldGroup>
              <Field data-disabled={status.rootSource === 'debug_override'}>
                <FieldLabel htmlFor="install-root">Install root</FieldLabel>
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
                    Browse
                  </Button>
                </div>
                <FieldDescription>
                  ComfyUI, app-owned environments, tools and data live here. A
                  changed root takes effect after restarting the app.
                </FieldDescription>
              </Field>
              {status.rootSource === 'debug_override' && (
                <p>
                  The debug environment override fixes this root. Remove the
                  override outside the app to choose another location.
                </p>
              )}
              {rootSelection.isSuccess && (
                <Alert>
                  <Check />
                  <AlertTitle>Restart required</AlertTitle>
                  <AlertDescription>
                    Close and reopen the desktop app to use {root.trim()}.
                    Settings and the wizard draft have been transferred. Setup
                    cannot start in this session.
                  </AlertDescription>
                </Alert>
              )}
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
                      <dt>Platform</dt>
                      <dd className="font-mono">
                        {hardware.data.os} · {hardware.data.osVersion}
                      </dd>
                    </div>
                    <div>
                      <dt>NVIDIA detection</dt>
                      <dd>
                        {hardware.data.gpuStatus === 'unavailable'
                          ? 'Probe unavailable — hardware is unknown'
                          : hardware.data.gpuStatus === 'no_nvidia_gpu'
                            ? 'No NVIDIA GPU detected'
                            : 'Detected'}
                      </dd>
                    </div>
                    <div>
                      <dt>Driver / maximum CUDA</dt>
                      <dd className="font-mono">
                        {hardware.data.driverVersion ?? 'Unknown'} /{' '}
                        {hardware.data.driverCuda ?? 'Unknown'}
                      </dd>
                    </div>
                    <div>
                      <dt>Install volume free</dt>
                      <dd className="font-mono">
                        {bytes(hardware.data.installVolumeFreeBytes)}
                      </dd>
                    </div>
                    {hardware.data.envOnSeparateVolume && (
                      <div>
                        <dt>Environment volume free</dt>
                        <dd className="font-mono">
                          {bytes(hardware.data.envVolumeFreeBytes)}
                        </dd>
                      </div>
                    )}
                    {hardware.data.os === 'windows' && (
                      <div>
                        <dt>Windows long paths</dt>
                        <dd>
                          {hardware.data.longPathsEnabled === null
                            ? 'Unknown'
                            : hardware.data.longPathsEnabled
                              ? 'Enabled'
                              : 'Disabled — use a short install path or enable long paths in Windows'}
                        </dd>
                      </div>
                    )}
                  </dl>
                  <ul className="flex flex-col gap-2">
                    {hardware.data.gpus.map((gpu) => (
                      <li key={gpu.index} className="font-mono text-xs">
                        GPU {gpu.index}: {gpu.name} · CC{' '}
                        {gpu.computeCapability ?? 'unknown'} ·{' '}
                        {gpu.vramMib === null
                          ? 'unknown VRAM'
                          : `${gpu.vramMib} MiB VRAM`}
                      </li>
                    ))}
                  </ul>
                  <p className="text-sm text-muted-foreground">
                    All detected GPUs count toward eligibility. Unresolved
                    compatibility or space estimates can disable a profile; the
                    profile step shows each reason.
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
                    Check hardware again
                  </Button>
                </>
              )}
            </>
          )}
          {stage === 2 && (
            <FieldGroup>
              <Field>
                <FieldLabel>Python source</FieldLabel>
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
                    App-managed
                  </ToggleGroupItem>
                  <ToggleGroupItem
                    value="customVenv"
                    disabled={sourceLocked && source.kind !== 'customVenv'}
                  >
                    Custom venv
                  </ToggleGroupItem>
                  <ToggleGroupItem
                    value="customInterpreter"
                    disabled={
                      mode === 'switch_venv' ||
                      (sourceLocked && source.kind !== 'customInterpreter')
                    }
                  >
                    Custom interpreter
                  </ToggleGroupItem>
                </ToggleGroup>
                <FieldDescription>
                  Managed Python uses the selected profile’s pinned version. An
                  empty custom folder becomes app-owned; an existing venv is
                  adopted and never deleted. A custom interpreter creates a new
                  app-owned venv under envs/.
                  {sourceLocked &&
                    ' Changing torch retains the Python source kind and minor version. Select the original custom interpreter again, or a separate custom venv target, when applicable.'}
                </FieldDescription>
              </Field>
              {source.kind !== 'managed' && (
                <>
                  <Field data-invalid={!!report && !report.usable}>
                    <FieldLabel htmlFor="python-path">
                      {source.kind === 'customVenv'
                        ? 'Venv folder'
                        : 'Python executable'}
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
                        Browse
                      </Button>
                    </div>
                  </Field>
                  <Button
                    variant="outline"
                    disabled={!source.path.trim() || validation.isPending}
                    onClick={() => validation.mutate(source)}
                  >
                    {validation.isPending ? 'Validating…' : 'Validate Python'}
                  </Button>
                  {validation.isError && (
                    <ErrorNotice error={validation.error} />
                  )}
                  {report && (
                    <>
                      <p className="break-all font-mono text-xs">
                        {report.resolvedPath} · Python{' '}
                        {report.pythonVersion ?? 'not created yet'} ·{' '}
                        {report.target}
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
                                ? ` (blocks: ${check.blocks.join(', ')})`
                                : ''}
                            </span>
                          </li>
                        ))}
                      </ul>
                      {report.target === 'existing_venv' && (
                        <FieldSet>
                          <FieldLegend>Adopted environment consent</FieldLegend>
                          <FieldDescription>
                            Setup will change packages in this exact venv. The
                            app cannot roll back these changes and will never
                            delete this folder. Choose a separate target when
                            re-running setup.
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
                                I approve modifying the adopted venv at{' '}
                                {report.resolvedPath}
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
                <FieldLegend>Setup profile</FieldLegend>
                <FieldDescription>
                  Profiles pin Python, torch, torchvision and Triton, plus
                  torchaudio when a compatible build exists. Disabled profiles include the backend’s
                  eligibility reasons.
                </FieldDescription>
                <FieldGroup>
                  <RadioGroup
                    aria-label="Setup profile"
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
                            {profile.recommended && <Badge>Recommended</Badge>}
                            {profile.status === 'experimental' && (
                              <Badge variant="outline">Experimental</Badge>
                            )}
                          </FieldLabel>
                          <FieldDescription>
                            Python {profile.python} · torch{' '}
                            {profile.torch.torch} · CUDA {profile.torch.cuda} ·{' '}
                            {profile.triton}
                            <br />
                            {profile.torch.torchaudio
                              ? `torchaudio ${profile.torch.torchaudio}`
                              : 'No compatible torchaudio build is available; audio nodes that require it need another profile.'}
                            <br />
                            Estimated peak:{' '}
                            {bytes(profile.estimatedInstallBytes)}{' '}
                            {profile.estimateCalibrated
                              ? '(calibrated)'
                              : '(not calibrated)'}
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
                              Compatibility evidence
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
                  No profiles were returned. Refresh hardware detection to try
                  again.
                </p>
              )}
            </>
          )}
          {stage === 4 && (
            <FieldGroup>
              {mode === 'first_run' ? (
                <>
                  <Field>
                    <FieldLabel>ComfyUI source</FieldLabel>
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
                        Latest release
                      </ToggleGroupItem>
                      <ToggleGroupItem value="master">
                        Latest commit (master)
                      </ToggleGroupItem>
                      <ToggleGroupItem value="tag">
                        Specific tag
                      </ToggleGroupItem>
                    </ToggleGroup>
                    <FieldDescription>
                      {comfySource === 'master'
                        ? "Installs the newest commit on ComfyUI's master branch (development code, may be less stable than a release). The checkout tracks origin/master, so `git pull` and the app's Fast-forward update it."
                        : 'The backend resolves the release tag and commit once per run. Tag browsing is not available; enter the exact tag below.'}
                    </FieldDescription>
                  </Field>
                  {comfySource === 'tag' && (
                    <Field>
                      <FieldLabel htmlFor="comfy-tag">ComfyUI tag</FieldLabel>
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
                  The existing ComfyUI checkout and recorded commit are
                  retained. ComfyUI updates are available separately.
                </p>
              )}
              <FieldSet>
                <FieldLegend>Optional components</FieldLegend>
                <FieldGroup>
                  {mode === 'repair' ? (
                    <p>
                      Repair retains the recorded Manager and accelerator choices.
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
                          Install ComfyUI-Manager
                        </FieldLabel>
                      </Field>
                      <FieldDescription>
                        {mode === 'first_run'
                          ? 'Installed from the latest commit of its default branch; covered by the confirmation on the review step.'
                          : 'If selected, setup pauses for consent after resolving the exact Manager source and commit, before installing its dependencies.'}
                      </FieldDescription>
                      <FieldSet>
                        <FieldLegend>Accelerators</FieldLegend>
                        <FieldDescription>
                          Options belong to the selected profile. Read each
                          option’s provenance before selecting it.
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
                            <p>This profile offers no optional accelerators.</p>
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
                      Clean package cache after setup
                    </FieldLabel>
                  </Field>
                </FieldGroup>
              </FieldSet>
              {mode === 'first_run' && (
                <FieldSet>
                  <FieldLegend>Curated custom nodes</FieldLegend>
                  <FieldDescription>
                    Each entry installs the latest commit of its repository’s
                    default branch and is covered by the confirmation on the
                    review step. For selected custom nodes, setup allows shipped
                    Python cache files and top-level native programs or libraries,
                    runs install.py if present, then installs the reviewed
                    requirements.txt without asking again.
                  </FieldDescription>
                  {curated.isPending && <p role="status">Loading curated catalog…</p>}
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
                          {entry.repository} · latest commit of the default branch
                        </FieldDescription>
                        {entry.unavailable !== null && (
                          <FieldDescription>Unavailable: {entry.unavailable}</FieldDescription>
                        )}
                      </Field>
                    ))}
                    {curated.isSuccess && curated.data.length === 0 && (
                      <p>No curated custom nodes are available.</p>
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
                  <dt>Install root</dt>
                  <dd className="break-all font-mono">{status.installRoot}</dd>
                </div>
                <div>
                  <dt>Mode</dt>
                  <dd>{modeLabels[mode]}</dd>
                </div>
                <div>
                  <dt>Python</dt>
                  <dd className="break-all font-mono">
                    {source.kind === 'managed'
                      ? `App-managed ${chosen?.python ?? ''}`
                      : source.path}
                  </dd>
                </div>
                <div>
                  <dt>Profile</dt>
                  <dd>{chosen?.label ?? 'No eligible profile'}</dd>
                </div>
                <div>
                  <dt>ComfyUI</dt>
                  <dd>
                    {mode === 'first_run'
                      ? comfySource === 'tag'
                        ? tag
                        : comfySource === 'master'
                          ? 'Latest commit on master'
                          : 'Latest published release'
                      : 'Keep current checkout'}
                  </dd>
                </div>
                <div>
                  <dt>Manager / cache clean</dt>
                  <dd>
                    {mode === 'repair' ? 'Recorded component choices retained' : installManager
                      ? mode === 'first_run'
                        ? 'Manager selected'
                        : 'Manager selected; consent still required'
                      : 'Manager off'}{' '}
                    / {cleanCache ? 'Clean cache' : 'Keep cache'}
                  </dd>
                </div>
                <div>
                  <dt>Accelerators</dt>
                  <dd>{mode === 'repair' ? 'Recorded choices retained' : chosen?.accelerators
                    .filter((option) => accelerators.includes(option.id))
                    .map((option) => option.name).join(', ') || 'None selected'}</dd>
                </div>
                {mode === 'first_run' && <div>
                  <dt>Curated custom nodes</dt>
                  <dd>{curated.data?.filter((entry) => curatedSelection?.includes(entry.id))
                    .map((entry) => entry.name).join(', ') || 'None selected'}</dd>
                </div>}
              </dl>
              <p>
                The backend rechecks free space and compatibility immediately
                before work. Package operations are serialized and require
                ComfyUI to be stopped.
              </p>
              {thirdPartyConsent && (
                <Alert>
                  <Info />
                  <AlertTitle>Third-party code</AlertTitle>
                  <AlertDescription>
                    <p>{THIRD_PARTY_DISCLOSURE}</p>
                    <ul
                      aria-label="Third-party sources"
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
                  <AlertTitle>Setup is not ready</AlertTitle>
                  <AlertDescription>
                    Review the Python validation, profile eligibility and tag
                    selection before starting.
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
                Back
              </Button>
            )}
            {status.activeEnv && (
              <Button variant="ghost" onClick={onClose}>
                Cancel
              </Button>
            )}
          </div>
          {stage === 0 ? (
            <Button
              disabled={
                !root.trim() ||
                rootSelection.isPending ||
                rootSelection.isSuccess
              }
              onClick={() => {
                if (root.trim() !== status.installRoot) rootSelection.mutate()
                else setStage(1)
              }}
            >
              {rootSelection.isPending
                ? 'Saving root…'
                : root.trim() !== status.installRoot
                  ? 'Save install root'
                  : 'Continue'}
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
              Continue
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
                ? 'Registering setup…'
                : 'Start setup'}
            </Button>
          )}
        </CardFooter>
      </Card>
    </div>
  )
}
