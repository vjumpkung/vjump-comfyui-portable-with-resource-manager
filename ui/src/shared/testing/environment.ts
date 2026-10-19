import type {
  EnvironmentSummaryDto,
  ProfileDto,
  RunSnapshotDto,
  SetupStatusDto,
} from '@/shared/ipc/gateway'

export const environment: EnvironmentSummaryDto = {
  id: 'env-selected',
  path: 'D:/Comfy/envs/selected',
  ownership: 'app_owned',
  source: 'managed',
  pythonVersion: '3.13.16',
  profileId: 'default',
  profileRevision: 1,
  state: 'verified',
  pins: [{ name: 'torch', version: '2.14.1' }],
  verification: 'CUDA and Triton verified',
}

export const profile: ProfileDto = {
  id: 'default',
  revision: 1,
  label: 'Python 3.13 / CUDA 13',
  status: 'stable',
  recommended: true,
  os: 'windows',
  python: '3.13.16',
  torch: {
    torch: '2.14.1',
    torchvision: '0.29.1',
    torchaudio: '2.14.1',
    cuda: 'cu130',
  },
  triton: 'triton-windows==3.8.0',
  minDriverCuda: '13.0',
  minComputeCapability: '8.0',
  estimatedInstallBytes: 20 * 1024 ** 3,
  estimateCalibrated: true,
  enabled: true,
  reasons: [],
  provenance: ['Verified fixture'],
}

export const run: RunSnapshotDto = {
  id: 'run-1',
  mode: 'first_run',
  status: 'failed',
  profileId: 'default',
  targetEnvId: 'env-candidate',
  steps: [
    {
      step: 'manager',
      number: 10,
      label: 'Install Manager',
      policy: 'optional',
      status: 'failed',
      attempts: 1,
      message: null,
    },
  ],
  failure: {
    step: 'manager',
    kind: 'consent_required',
    message: 'Source consent required',
    command: null,
    exitCode: null,
    tail: ['sanitized tail'],
    consent: {
      source: 'https://github.com/Comfy-Org/ComfyUI-Manager.git',
      commit: 'a'.repeat(40),
    },
    canSkip: true,
  },
  cancelRequested: false,
  activationPhase: 'none',
  curated: [],
  curatedReviews: [],
  createdAt: 'now',
  updatedAt: 'now',
}

export const setupStatus: SetupStatusDto = {
  phase: 'needs_setup',
  recovery: null,
  installRoot: 'D:/Comfy',
  rootSource: 'default',
  activeEnv: null,
  run: null,
  draft: null,
}
