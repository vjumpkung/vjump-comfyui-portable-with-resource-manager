// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { gateway } from '@/shared/ipc/gateway'
import { profile, setupStatus } from '@/shared/testing/environment'
import { SetupWizard } from './setup-wizard'

vi.mock('@/shared/ui/log-view', () => ({ LogView: () => <div>Live log</div> }))
vi.mock('@/shared/ipc/gateway', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/ipc/gateway')>()),
  gateway: {
    setupDetectHardware: vi.fn(),
    setupListProfiles: vi.fn(),
    setupValidatePython: vi.fn(),
    setupSelectRoot: vi.fn(),
    setupRun: vi.fn(),
    setupCancel: vi.fn(),
    pickFolder: vi.fn(),
    pickFile: vi.fn(),
  },
}))

function mount(status = setupStatus) {
  return render(
    <QueryClientProvider
      client={
        new QueryClient({
          defaultOptions: {
            queries: { retry: false },
            mutations: { retry: false },
          },
        })
      }
    >
      <SetupWizard
        status={status}
        mode="first_run"
        onClose={vi.fn()}
        refresh={vi.fn()}
      />
    </QueryClientProvider>,
  )
}
function next() {
  fireEvent.click(screen.getByRole('button', { name: 'Continue' }))
}
async function toPython() {
  next()
  await screen.findByText('Detected')
  next()
}

beforeEach(() => {
  vi.resetAllMocks()
  vi.mocked(gateway.setupDetectHardware).mockResolvedValue({
    gpuStatus: 'detected',
    gpus: [],
    driverVersion: '591',
    driverCuda: '13.1',
    os: 'windows',
    osVersion: '11',
    installVolumeFreeBytes: 100 * 1024 ** 3,
    envVolumeFreeBytes: null,
    envOnSeparateVolume: false,
    longPathsEnabled: true,
  })
  vi.mocked(gateway.setupListProfiles).mockResolvedValue([profile])
})
afterEach(cleanup)

describe('setup wizard', () => {
  it('saves a changed root and blocks setup until restart', async () => {
    vi.mocked(gateway.setupSelectRoot).mockResolvedValue({
      restartRequired: true,
    })
    mount()
    fireEvent.change(screen.getByLabelText('Install root'), {
      target: { value: 'E:/Comfy' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Save install root' }))
    await screen.findByText('Restart required')
    expect(gateway.setupSelectRoot).toHaveBeenCalledWith(
      'E:/Comfy',
      expect.stringContaining('"version":1'),
    )
    expect(
      screen.getByRole('button', { name: 'Save install root' }),
    ).toHaveProperty('disabled', true)
    expect(gateway.setupRun).not.toHaveBeenCalled()
  })
  it('disables incompatible profiles and shows their reasons', async () => {
    vi.mocked(gateway.setupListProfiles).mockResolvedValue([
      {
        ...profile,
        enabled: false,
        reasons: [
          {
            code: 'driver',
            message: 'Driver CUDA is too old for this profile',
          },
        ],
      },
    ])
    mount()
    await toPython()
    next()
    await screen.findByText('Driver CUDA is too old for this profile')
    expect(screen.getByRole('radio')).toHaveProperty('disabled', true)
    expect(screen.getByRole('button', { name: 'Continue' })).toHaveProperty(
      'disabled',
      true,
    )
  })
  it('requires validation and exact adopted-path consent; editing invalidates it', async () => {
    vi.mocked(gateway.setupValidatePython).mockResolvedValue({
      target: 'existing_venv',
      resolvedPath: 'D:/external',
      pythonVersion: '3.13.16',
      usable: true,
      checks: [{ id: 'arch', passed: true, detail: '64 bit', blocks: [] }],
    })
    mount()
    await toPython()
    fireEvent.click(screen.getByRole('radio', { name: 'Custom venv' }))
    fireEvent.change(screen.getByLabelText('Venv folder'), {
      target: { value: 'D:/external' },
    })
    expect(screen.getByRole('button', { name: 'Continue' })).toHaveProperty(
      'disabled',
      true,
    )
    fireEvent.click(screen.getByRole('button', { name: 'Validate Python' }))
    await screen.findByText('Adopted environment consent')
    expect(screen.getByRole('button', { name: 'Continue' })).toHaveProperty(
      'disabled',
      true,
    )
    fireEvent.click(screen.getByRole('checkbox'))
    expect(screen.getByRole('button', { name: 'Continue' })).toHaveProperty(
      'disabled',
      false,
    )
    fireEvent.change(screen.getByLabelText('Venv folder'), {
      target: { value: 'D:/another' },
    })
    expect(screen.getByRole('button', { name: 'Continue' })).toHaveProperty(
      'disabled',
      true,
    )
  })
  it('starts a typed new run with the selected tag and Manager off by default', async () => {
    vi.mocked(gateway.setupRun).mockResolvedValue({ runId: 'run-new' })
    mount()
    await toPython()
    next()
    await screen.findByRole('radio', { name: /Python 3.13/ })
    next()
    fireEvent.click(screen.getByRole('radio', { name: 'Specific tag' }))
    fireEvent.change(screen.getByLabelText('ComfyUI tag'), {
      target: { value: 'v0.9.0' },
    })
    next()
    fireEvent.click(screen.getByRole('button', { name: 'Start setup' }))
    await waitFor(() =>
      expect(gateway.setupRun).toHaveBeenCalledWith(
        {
          kind: 'new',
          request: {
            mode: 'first_run',
            pythonSource: { kind: 'managed' },
            profileId: 'default',
            comfyuiRef: { kind: 'tag', value: 'v0.9.0' },
            installManager: false,
            cleanCache: false,
            confirmAdoptedChanges: false,
            allowSourceBuilds: false,
          },
        },
        expect.any(Function),
      ),
    )
  })
})
