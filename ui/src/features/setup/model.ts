import type {
  EnvironmentSummaryDto,
  ProfileDto,
  PythonSourceDto,
  PythonValidationDto,
  RunSnapshotDto,
  SetupModeDto,
} from '@/shared/ipc/gateway'

export const modeLabels: Record<SetupModeDto, string> = {
  first_run: 'Set up ComfyUI',
  repair: 'Repair environment',
  change_torch: 'Change torch / CUDA',
  change_python: 'Change Python',
  switch_venv: 'Switch to custom venv',
}

export function minor(version: string): string {
  return version.split('.').slice(0, 2).join('.')
}

export function profileReasons(
  profile: ProfileDto,
  mode: SetupModeDto,
  active: EnvironmentSummaryDto | null,
  validation?: PythonValidationDto,
): string[] {
  const reasons = profile.reasons.map((reason) => reason.message)
  if (!profile.enabled && !reasons.length)
    reasons.push('This profile is unavailable on this machine.')
  if (
    active &&
    ['repair', 'change_torch'].includes(mode) &&
    minor(active.pythonVersion) !== minor(profile.python)
  ) {
    reasons.push(
      'This mode requires the selected environment’s Python minor version.',
    )
  }
  if (
    active &&
    mode === 'change_python' &&
    minor(active.pythonVersion) === minor(profile.python)
  ) {
    reasons.push('Choose a different Python minor version for this mode.')
  }
  if (
    validation?.pythonVersion &&
    validation.target !== 'empty_folder' &&
    minor(validation.pythonVersion) !== minor(profile.python)
  ) {
    reasons.push(
      `The validated interpreter uses Python ${minor(validation.pythonVersion)}.`,
    )
  }
  if (
    validation?.checks.some(
      (check) => !check.passed && check.blocks.includes('triton'),
    )
  ) {
    reasons.push(
      'This profile requires Triton; fix the failed Python development-file checks.',
    )
  }
  return reasons
}

export function canSkip(run: RunSnapshotDto): boolean {
  return (
    run.status === 'failed' &&
    !!run.failure &&
    run.steps.some(
      (step) =>
        step.step === run.failure?.step &&
        step.policy === 'optional' &&
        step.status === 'failed',
    )
  )
}

export type WizardDraft = {
  pythonSource: PythonSourceDto
  profileId: string
  tag: string
  useTag: boolean
  installManager: boolean
  cleanCache: boolean
}

/** Root migration is the only backend draft writer. Treat its opaque JSON as untrusted. */
export function readDraft(raw: string | null): Partial<WizardDraft> {
  if (!raw) return {}
  try {
    const value: unknown = JSON.parse(raw)
    if (
      !value ||
      typeof value !== 'object' ||
      !('version' in value) ||
      value.version !== 1
    )
      return {}
    const draft: Partial<WizardDraft> = {}
    if ('profileId' in value && typeof value.profileId === 'string')
      draft.profileId = value.profileId
    if ('tag' in value && typeof value.tag === 'string') draft.tag = value.tag
    if ('useTag' in value && typeof value.useTag === 'boolean')
      draft.useTag = value.useTag
    if ('installManager' in value && typeof value.installManager === 'boolean')
      draft.installManager = value.installManager
    if ('cleanCache' in value && typeof value.cleanCache === 'boolean')
      draft.cleanCache = value.cleanCache
    if (
      'pythonSource' in value &&
      value.pythonSource &&
      typeof value.pythonSource === 'object' &&
      'kind' in value.pythonSource
    ) {
      const source = value.pythonSource
      if (source.kind === 'managed') draft.pythonSource = { kind: 'managed' }
      else if (
        (source.kind === 'customVenv' || source.kind === 'customInterpreter') &&
        'path' in source &&
        typeof source.path === 'string'
      ) {
        draft.pythonSource = { kind: source.kind, path: source.path }
      }
    }
    return draft
  } catch {
    return {}
  }
}
